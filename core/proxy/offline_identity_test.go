package proxy

import (
	"bytes"
	"context"
	"encoding/json"
	"log/slog"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"golang.org/x/oauth2"
)

// Independent processes load the saved profile and send real offline login packets.
// A process-local identity cache cannot satisfy this reconnect regression.
func TestOfflineSessionIdentityAcrossCoreLifetimes(t *testing.T) {
	if os.Getenv("CINNABAR_TEST_OFFLINE_IDENTITY_CHILD") == "1" {
		profile, err := authcache.LoadDevice(os.Getenv("CINNABAR_TEST_OFFLINE_IDENTITY_PATH"))
		if err != nil {
			t.Fatal(err)
		}
		identity := observeOfflineLogin(t, &profile, os.Getenv("CINNABAR_TEST_OFFLINE_IDENTITY_NAME"))
		if err := json.NewEncoder(os.Stdout).Encode(identity); err != nil {
			t.Fatal(err)
		}
		os.Exit(0)
	}
	path := filepath.Join(t.TempDir(), "auth", "device.json")
	first := offlineIdentityInNewProcess(t, path, "Alex")
	saved, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	again := offlineIdentityInNewProcess(t, path, "Alex")
	if again != first {
		t.Fatalf("reconnect changed server-observed identity: %+v -> %+v", first, again)
	}
	otherName := offlineIdentityInNewProcess(t, path, "Sam")
	otherDevice := offlineIdentityInNewProcess(t, filepath.Join(t.TempDir(), "device.json"), "Alex")
	if otherName.Identity == first.Identity || otherDevice.Identity == first.Identity || otherName.Identity == otherDevice.Identity {
		t.Fatal("distinct personas or persisted devices share a UUID")
	}
	if final := offlineIdentityInNewProcess(t, path, "Alex"); final != first {
		t.Fatal("visiting another persona changed the original identity")
	}
	after, err := os.ReadFile(path)
	if err != nil || !bytes.Equal(after, saved) {
		t.Fatalf("reconnecting rewrote device identity: %v", err)
	}
}

// offlineIdentityInNewProcess restarts the compiled test executable without inherited identity state.
func offlineIdentityInNewProcess(t *testing.T, path, name string) login.IdentityData {
	t.Helper()
	command := exec.CommandContext(t.Context(), os.Args[0], "-test.run=^TestOfflineSessionIdentityAcrossCoreLifetimes$")
	for _, value := range os.Environ() {
		if !strings.HasPrefix(value, "CINNABAR_TEST_OFFLINE_IDENTITY_") {
			command.Env = append(command.Env, value)
		}
	}
	command.Env = append(command.Env,
		"CINNABAR_TEST_OFFLINE_IDENTITY_CHILD=1",
		"CINNABAR_TEST_OFFLINE_IDENTITY_PATH="+path,
		"CINNABAR_TEST_OFFLINE_IDENTITY_NAME="+name,
	)
	output, err := command.CombinedOutput()
	if err != nil {
		t.Fatalf("new core lifetime: %v: %s", err, output)
	}
	var identity login.IdentityData
	if err := json.Unmarshal(output, &identity); err != nil {
		t.Fatalf("decode subprocess identity: %v: %s", err, output)
	}
	return identity
}

// A bridge caller's device claims cannot stand in for an install-owned profile.
func TestOfflineSessionIdentityWithoutOwnedDeviceStaysEphemeral(t *testing.T) {
	first := observeOfflineLogin(t, nil, "Alex")
	again := observeOfflineLogin(t, nil, "Alex")
	if first.Identity == again.Identity {
		t.Fatal("sessions without an owned device share a default UUID")
	}
}

func TestOfflineSessionIdentityInvalidDevicesStayEphemeral(t *testing.T) {
	for _, id := range []login.DeviceID{"malformed", "00000000000000000000000000000000", "00000000-0000-0000-0000-000000000000"} {
		t.Run(string(id), func(t *testing.T) {
			profile := device.Profile{OS: protocol.DeviceWin32, ID: id}
			first := observeOfflineLogin(t, &profile, "Alex")
			again := observeOfflineLogin(t, &profile, "Alex")
			if first.Identity == again.Identity {
				t.Fatal("invalid/default device shares a deterministic UUID")
			}
		})
	}
	if _, err := newSessionDownstream(testSessionConnect(t), &device.Profile{}); err == nil {
		t.Fatal("an empty core device was accepted")
	}
}

func TestOfflineSessionIdentityDoesNotOverrideAuthenticatedAccount(t *testing.T) {
	profile, err := authcache.LoadDevice(filepath.Join(t.TempDir(), "device.json"))
	if err != nil {
		t.Fatal(err)
	}
	downstream := offlineIdentityDownstream(t, &profile, "Alex")
	before := downstream.IdentityData()
	source := oauth2.StaticTokenSource(&oauth2.Token{AccessToken: "test-token"})
	dialer := newUpstreamDialer(downstream, source)
	if dialer.TokenSource != source || dialer.IdentityData != (login.IdentityData{}) || dialer.ClientData.ThirdPartyName != "" {
		t.Fatal("the offline persona replaced token-owned identity")
	}
	if downstream.IdentityData() != before {
		t.Fatal("constructing an authenticated dialer mutated session identity")
	}
}

// offlineIdentityDownstream constructs the production session boundary with fixed untrusted client claims.
func offlineIdentityDownstream(t *testing.T, profile *device.Profile, name string) *sessionDownstream {
	t.Helper()
	request := testSessionConnect(t)
	var client login.ClientData
	if err := json.Unmarshal(request.ClientData, &client); err != nil {
		t.Fatal(err)
	}
	client.ThirdPartyName = name
	client.DeviceID = "112233445566778899aabbccddeeff00"
	client.MarkClaims("DeviceId", "ThirdPartyName")
	var err error
	request.ClientData, err = json.Marshal(client)
	if err != nil {
		t.Fatal(err)
	}
	downstream, err := newSessionDownstream(request, profile)
	if err != nil {
		t.Fatal(err)
	}
	return downstream
}

// observeOfflineLogin checks the parsed server identity after a real encoded login exchange.
func observeOfflineLogin(t *testing.T, profile *device.Profile, name string) login.IdentityData {
	t.Helper()
	downstream := offlineIdentityDownstream(t, profile, name)
	dialer := newUpstreamDialer(downstream, nil)
	network := newMemoryNetwork()
	listener, err := (minecraft.ListenConfig{AuthenticationDisabled: true, ErrorLog: slog.New(slog.DiscardHandler)}).ListenNetwork(network, "")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	type acceptedLogin struct {
		conn *minecraft.Conn
		err  error
	}
	accepted := make(chan acceptedLogin, 1)
	go func() {
		conn, err := listener.Accept()
		if err != nil {
			accepted <- acceptedLogin{err: err}
			return
		}
		server := conn.(*minecraft.Conn)
		err = server.WritePacketImmediate(relayFixtureStartup()...)
		accepted <- acceptedLogin{conn: server, err: err}
	}()
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	client, err := dialer.DialContextNetwork(ctx, network, "")
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close()
	select {
	case received := <-accepted:
		if received.conn != nil {
			defer received.conn.Close()
		}
		if received.err != nil {
			t.Fatal(received.err)
		}
		identity := received.conn.IdentityData()
		if identity != client.IdentityData() {
			t.Fatal("client and server disagree about login identity")
		}
		parsed, err := uuid.Parse(identity.Identity)
		if err != nil || parsed == uuid.Nil || identity.DisplayName != name || identity.XUID != "" || identity.TitleID != "" {
			t.Fatalf("invalid offline server identity: %+v, %v", identity, err)
		}
		if dialer.IdentityData.Identity != "" && identity.Identity != dialer.IdentityData.Identity {
			t.Fatal("the encoded login changed the profile UUID")
		}
		return identity
	case <-ctx.Done():
		t.Fatal(ctx.Err())
		return login.IdentityData{}
	}
}
