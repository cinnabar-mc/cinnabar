package webrelay

import (
	"bytes"
	"encoding/json"
	"os"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestHeaderlessControlHandshakeSuppliesLoginDevice(t *testing.T) {
	data, err := os.ReadFile("../../crates/bridge/data/contract.json")
	if err != nil {
		t.Fatal(err)
	}
	var manifest struct {
		Relay struct {
			Fixtures []json.RawMessage `json:"fixtures"`
		} `json:"relay"`
	}
	if err := json.Unmarshal(data, &manifest); err != nil {
		t.Fatal(err)
	}
	checked := 0
	for _, fixture := range manifest.Relay.Fixtures {
		var prefix hello
		if json.Unmarshal(fixture, &prefix) != nil || prefix.UserAgent == "" {
			continue
		}
		conn := &bufferConn{reader: bytes.NewReader(frameJSON(prefix))}
		got, err := readHello(conn, bridgecontract.RelayChannelControl)
		if err != nil {
			t.Fatal(err)
		}
		profile := browserDevice(got.browserUserAgent(""), got.AppleMobile)
		if profile == nil {
			t.Fatal("headerless browser has no login device")
		}
		// Cinnabar supplies skin and gameplay claims; the relay supplies all device claims.
		claims := login.ClientData{GameVersion: minecraft.DefaultProtocol.Ver(), LanguageCode: "en_US", SkinID: "skin", ThirdPartyName: "Steve"}
		if claims.Validate() == nil {
			t.Fatal("device-free login unexpectedly validated")
		}
		profile.Apply(&claims)
		if err := claims.Validate(); err != nil {
			t.Fatalf("headerless login: %v", err)
		}
		want := protocol.DeviceWin32
		if prefix.AppleMobile {
			want = protocol.DeviceIOS
		}
		if claims.DeviceOS != want || claims.DeviceModel != "" {
			t.Fatal("headerless login used relay hardware")
		}
		checked++
	}
	if checked == 0 {
		t.Fatal("shared browser handshake fixture is missing")
	}
}

func TestBrowserDeviceUsesBrowserPlatformWithoutRelayModel(t *testing.T) {
	for agent, want := range map[string]protocol.DeviceOS{
		"Mozilla/5.0 (Linux; Android 15)":                        protocol.DeviceAndroid,
		"Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)": protocol.DeviceIOS,
		"Mozilla/5.0 (iPad; CPU OS 18_0 like Mac OS X)":          protocol.DeviceIOS,
		"Mozilla/5.0 (Windows NT 10.0; Win64; x64)":              protocol.DeviceWin32,
		"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)":        protocol.DeviceOSX,
		"Mozilla/5.0 (X11; Linux x86_64)":                        protocol.DeviceLinux,
		"Mozilla/5.0 (X11; CrOS x86_64)":                         protocol.DeviceLinux,
	} {
		got := browserDevice(agent, false)
		if got == nil || got.OS != want || got.Model != "" || got.ID == "" {
			t.Fatalf("browser device = %+v, want OS %v without a fabricated model", got, want)
		}
	}
	if got := browserDevice("unknown browser", false); got != nil {
		t.Fatalf("unknown browser adopted a device: %+v", got)
	}
}

func TestDesktopModeIPadUsesMobileClaims(t *testing.T) {
	profile := browserDevice("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", true)
	claims := login.ClientData{}
	profile.Apply(&claims)
	if claims.DeviceOS != protocol.DeviceIOS || claims.DefaultInputMode != packet.InputModeTouch || claims.DeviceModel != "" {
		t.Fatalf("desktop-mode iPad claimed a Mac: %+v", claims)
	}
}
