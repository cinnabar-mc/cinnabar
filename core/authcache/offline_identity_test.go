package authcache

import (
	"encoding/base64"
	"testing"

	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
)

// The vector freezes the versioned identity contract across later releases.
func TestOfflineIdentityVersionedVector(t *testing.T) {
	profile := device.Profile{OS: protocol.DeviceWin32, ID: "00112233445566778899aabbccddeeff"}
	got := OfflineIdentity(profile, "Alex")
	if got.Identity != "aeae440e-00a4-5468-8850-15841ee7ec28" || got.DisplayName != "Alex" || got.XUID != "" || got.TitleID != "" {
		t.Fatalf("offline identity = %+v", got)
	}
	parsed, err := uuid.Parse(got.Identity)
	if err != nil || parsed == uuid.Nil || parsed.Version() != 5 || parsed.Variant() != uuid.RFC4122 {
		t.Fatalf("invalid versioned UUID %q: %v", got.Identity, err)
	}
}

func TestOfflineIdentityScopesDeviceAndPersona(t *testing.T) {
	first := device.Profile{OS: protocol.DeviceWin32, ID: "00112233445566778899aabbccddeeff"}
	second := device.Profile{OS: protocol.DeviceWin32, ID: "ffeeddccbbaa99887766554433221100"}
	ids := make(map[string]bool)
	for _, profile := range []device.Profile{first, second} {
		for _, name := range []string{"Alex", "Sam", "alex"} {
			got := OfflineIdentity(profile, name)
			if ids[got.Identity] {
				t.Fatalf("distinct profile/persona reused %s", got.Identity)
			}
			ids[got.Identity] = true
			if again := OfflineIdentity(profile, name); again != got {
				t.Fatalf("same profile/persona changed: %+v -> %+v", got, again)
			}
		}
	}
}

// Login-platform migrations and equivalent device encodings keep the same logical profile.
func TestOfflineIdentityIgnoresDevicePresentation(t *testing.T) {
	var first string
	for _, profile := range []device.Profile{
		{OS: protocol.DeviceLinux, Model: "old model", ID: "00112233445566778899aabbccddeeff"},
		{OS: protocol.DeviceWin32, Model: "new model", ID: "00112233445566778899aabbccddeeff"},
		{OS: protocol.DeviceIOS, ID: "00112233445566778899AABBCCDDEEFF"},
		{OS: protocol.DeviceOrbis, ID: "00112233-4455-6677-8899-aabbccddeeff"},
	} {
		got := OfflineIdentity(profile, "Alex").Identity
		if first == "" {
			first = got
		}
		if got != first {
			t.Fatalf("device presentation changed offline identity: %s -> %s", first, got)
		}
	}
}

func TestOfflineIdentitySupportsBase64Devices(t *testing.T) {
	material := make([]byte, 32)
	material[31] = 1
	first := device.Profile{OS: protocol.DeviceXBOX, ID: login.DeviceID(base64.StdEncoding.EncodeToString(material))}
	got := OfflineIdentity(first, "Alex")
	if again := OfflineIdentity(first, "Alex"); again != got {
		t.Fatal("base64 device changed identity")
	}
	material[31] = 2
	second := device.Profile{OS: protocol.DeviceXBOX, ID: login.DeviceID(base64.StdEncoding.EncodeToString(material))}
	if OfflineIdentity(second, "Alex").Identity == got.Identity {
		t.Fatal("distinct base64 devices share identity")
	}
}

func TestOfflineIdentityInvalidDevicesNeverShareDefault(t *testing.T) {
	seen := make(map[string]bool)
	for _, id := range []login.DeviceID{
		"", "not-a-device", "0011", " 00112233445566778899aabbccddeeff",
		"00000000000000000000000000000000", "00000000-0000-0000-0000-000000000000",
		login.DeviceID(base64.StdEncoding.EncodeToString(make([]byte, 32))),
	} {
		for range 2 {
			got := OfflineIdentity(device.Profile{OS: protocol.DeviceWin32, ID: id}, "Alex")
			parsed, err := uuid.Parse(got.Identity)
			if err != nil || parsed == uuid.Nil || seen[got.Identity] || got.XUID != "" {
				t.Fatalf("invalid/default device reused invalid identity: %+v, %v", got, err)
			}
			seen[got.Identity] = true
		}
	}
}
