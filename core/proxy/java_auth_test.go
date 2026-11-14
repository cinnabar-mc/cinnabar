package proxy

import (
	"context"
	"encoding/json"
	"errors"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/javaedition/auth"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/conn"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/javatest"
	"github.com/sandertv/gophertunnel/minecraft"
	"golang.org/x/oauth2"
)

// switchStub is a Microsoft app whose device-code sign-in never finishes and whose identity
// token is fixed.
type switchStub struct{}

func (switchStub) DeviceAuth(context.Context) (*oauth2.DeviceAuthResponse, error) {
	return &oauth2.DeviceAuthResponse{UserCode: "TESTCODE", VerificationURI: "https://www.microsoft.com/link", Expiry: time.Now().Add(time.Minute)}, nil
}

func (switchStub) DeviceAccessToken(ctx context.Context, _ *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
	<-ctx.Done()
	return nil, ctx.Err()
}

func (switchStub) TokenSource(_ context.Context, token *oauth2.Token) oauth2.TokenSource {
	return oauth2.StaticTokenSource(token)
}

func (switchStub) IdentityToken(context.Context, oauth2.TokenSource) (string, error) {
	return "XBL3.0 x=hash;switch", nil
}

// javaServices fakes the Java Edition services and records the server hash a join names.
type javaServices struct {
	mu   sync.Mutex
	hash string
}

func (fake *javaServices) start(t *testing.T) auth.Services {
	t.Helper()
	mux := http.NewServeMux()
	mux.HandleFunc("/login", func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{"access_token": "java-access", "expires_in": 86400})
	})
	mux.HandleFunc("/profile", func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(auth.Profile{ID: "069a79f444e94726a5befca90e38aaf5", Name: "Notch"})
	})
	mux.HandleFunc("/join", func(w http.ResponseWriter, r *http.Request) {
		var body map[string]string
		_ = json.NewDecoder(r.Body).Decode(&body)
		fake.mu.Lock()
		fake.hash = body["serverId"]
		fake.mu.Unlock()
		w.WriteHeader(http.StatusNoContent)
	})
	server := httptest.NewServer(mux)
	t.Cleanup(server.Close)
	return auth.Services{HTTP: server.Client(), LoginURL: server.URL + "/login", ProfileURL: server.URL + "/profile", JoinURL: server.URL + "/join"}
}

func startJavaServer(t *testing.T, config javatest.Config) *javatest.Server {
	t.Helper()
	server, err := javatest.Start(config)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(server.Close)
	return server
}

// disconnectText returns the disconnect message a failed join shows the player.
func disconnectText(t *testing.T, err error) string {
	t.Helper()
	var disconnect *minecraft.DisconnectPacketError
	if !errors.As(err, &disconnect) {
		t.Fatalf("join failed without disconnect text: %v", err)
	}
	return disconnect.Message
}

func dialJavaForTest(t *testing.T, server *javatest.Server, keychain *auth.Keychain) (upstreamSession, error) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	return dialJavaUpstream(ctx, server.Addr(), keychain, "Steve", slog.New(slog.DiscardHandler))
}

// An online-mode server asks for the session of a player with no Java Edition sign-in: the
// join fails with the code to enter, and the sign-in goes on waiting for it.
func TestJavaJoinWithoutASignInShowsTheCode(t *testing.T) {
	server := startJavaServer(t, javatest.Config{OnlineMode: true})
	keychain := auth.NewKeychain(auth.KeychainConfig{Path: filepath.Join(t.TempDir(), auth.CacheFile), App: switchStub{}})
	t.Cleanup(keychain.Close)
	_, err := dialJavaForTest(t, server, keychain)
	text := disconnectText(t, err)
	if !strings.Contains(text, "TESTCODE") || !strings.Contains(text, "https://www.microsoft.com/link") {
		t.Fatalf("disconnect text = %q", text)
	}
}

// A saved Java Edition sign-in that no longer works asks for a new one, as no sign-in does.
func TestJavaJoinWithAnUnusableSignInShowsTheCode(t *testing.T) {
	server := startJavaServer(t, javatest.Config{OnlineMode: true})
	path := filepath.Join(t.TempDir(), auth.CacheFile)
	if err := os.WriteFile(path, []byte("not a token"), 0o600); err != nil {
		t.Fatal(err)
	}
	keychain := auth.NewKeychain(auth.KeychainConfig{Path: path, App: switchStub{}})
	t.Cleanup(keychain.Close)
	_, err := dialJavaForTest(t, server, keychain)
	if text := disconnectText(t, err); !strings.Contains(text, "TESTCODE") {
		t.Fatalf("disconnect text = %q", text)
	}
}

// A signed-out player cannot join an online-mode server, and is told to sign in.
func TestJavaJoinSignedOutAsksForAnAccount(t *testing.T) {
	server := startJavaServer(t, javatest.Config{OnlineMode: true})
	_, err := dialJavaForTest(t, server, nil)
	if text := disconnectText(t, err); !strings.Contains(text, "Sign in to Cinnabar") {
		t.Fatalf("disconnect text = %q", text)
	}
}

// With a Java Edition sign-in the player logs in as its Java profile and registers the join
// with the session service.
func TestJavaJoinSignedInUsesTheJavaProfile(t *testing.T) {
	server := startJavaServer(t, javatest.Config{OnlineMode: true})
	fake := &javaServices{}
	path := filepath.Join(t.TempDir(), auth.CacheFile)
	token, err := json.Marshal(&oauth2.Token{AccessToken: "switch-access", RefreshToken: "switch-refresh", Expiry: time.Now().Add(time.Hour)})
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, token, 0o600); err != nil {
		t.Fatal(err)
	}
	keychain := auth.NewKeychain(auth.KeychainConfig{Path: path, App: switchStub{}, Services: fake.start(t)})
	t.Cleanup(keychain.Close)

	joined := make(chan *javatest.Client, 1)
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		client, _ := server.Accept(ctx)
		joined <- client
	}()
	upstream, err := dialJavaForTest(t, server, keychain)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = upstream.Close() })
	client := <-joined
	if client == nil || client.Username != "Notch" {
		t.Fatalf("the server saw %+v", client)
	}
	fake.mu.Lock()
	defer fake.mu.Unlock()
	if fake.hash == "" {
		t.Fatal("the join was not registered with the session service")
	}
}

// A server that refuses the login shows the player its own reason.
func TestJavaJoinShowsTheServersRefusal(t *testing.T) {
	server := startJavaServer(t, javatest.Config{RefuseLogin: `{"text":"You are banned","color":"red"}`})
	_, err := dialJavaForTest(t, server, nil)
	if text := disconnectText(t, err); !strings.Contains(text, "You are banned") {
		t.Fatalf("disconnect text = %q", text)
	}
}

// A sign-in that finished only after the login named the player cannot authenticate that
// login, so the player is told to reconnect rather than shown the server's refusal.
func TestJavaJoinWithALateSignInAsksToReconnect(t *testing.T) {
	err := javaJoinError(&conn.SessionError{Err: errJavaSignInLate})
	if text := disconnectText(t, err); !strings.Contains(text, "Reconnect") {
		t.Fatalf("disconnect text = %q", text)
	}
}
