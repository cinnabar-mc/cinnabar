package proxy

import (
	"encoding/json"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestSessionLoginClaimsIOSMobileInput(t *testing.T) {
	profile := device.Profile{OS: protocol.DeviceIOS, Model: "", ID: "00112233445566778899AABBCCDDEEFF"}
	downstream, err := newSessionDownstream(testSessionConnect(t), &profile)
	if err != nil {
		t.Fatal(err)
	}
	data := downstream.ClientData()
	if data.DeviceOS != protocol.DeviceIOS || data.PlatformType != login.PlatformTypeMobile || data.DefaultInputMode != packet.InputModeTouch {
		t.Fatalf("iOS login claimed desktop input: %+v", data)
	}
}

func TestSessionLoginClaimsNativeDesktopDevice(t *testing.T) {
	t.Parallel()
	for _, os := range []protocol.DeviceOS{protocol.DeviceOSX, protocol.DeviceWin32, protocol.DeviceLinux} {
		profile := device.Profile{OS: os, Model: "Actual hardware outside the preset list", ID: "00112233445566778899aabbccddeeff"}
		request := testSessionConnect(t)
		var claims map[string]any
		if err := json.Unmarshal(request.ClientData, &claims); err != nil {
			t.Fatal(err)
		}
		claims["DeviceOS"] = protocol.DeviceAndroid
		claims["DeviceModel"] = "Old fabricated phone"
		claims["PlatformType"] = login.PlatformTypeMobile
		request.ClientData, _ = json.Marshal(claims)
		downstream, err := newSessionDownstream(request, &profile)
		if err != nil {
			t.Fatal(err)
		}
		encoded, err := json.Marshal(downstream.ClientData())
		if err != nil {
			t.Fatal(err)
		}
		var data login.ClientData
		if err := json.Unmarshal(encoded, &data); err != nil {
			t.Fatal(err)
		}
		if data.DeviceOS != os || data.DeviceModel != profile.Model || data.DeviceID != profile.ID ||
			data.PlatformType != login.PlatformTypeDesktop || data.DefaultInputMode != packet.InputModeMouse {
			t.Fatalf("OS %v serialized the wrong device: %+v", os, data)
		}
	}
}

// Session claims and the upstream login must carry the persisted Android profile's mobile category.
func TestSessionLoginClaimsAndroidPlatformType(t *testing.T) {
	t.Parallel()
	profile := device.Profile{
		OS:    protocol.DeviceAndroid,
		Model: "Fixture Android",
		ID:    "00112233445566778899aabbccddeeff",
	}
	for _, test := range []struct {
		name     string
		platform json.RawMessage
		want     int
	}{
		{name: "absent platform", want: login.PlatformTypeMobile},
		{name: "desktop zero", platform: json.RawMessage(`0`), want: login.PlatformTypeMobile},
		{name: "mobile platform", platform: json.RawMessage(`1`), want: login.PlatformTypeMobile},
		{name: "unknown nonzero", platform: json.RawMessage(`7`), want: 7},
	} {
		t.Run(test.name, func(t *testing.T) {
			request := testSessionConnect(t)
			claims := map[string]json.RawMessage{}
			if err := json.Unmarshal(request.ClientData, &claims); err != nil {
				t.Fatal(err)
			}
			delete(claims, "DeviceOS")
			delete(claims, "PlatformType")
			if test.platform != nil {
				claims["PlatformType"] = test.platform
			}
			claims["ClientRandomId"] = json.RawMessage(`-7`)
			claims["FixtureClaim"] = json.RawMessage(`"kept"`)
			var err error
			request.ClientData, err = json.Marshal(claims)
			if err != nil {
				t.Fatal(err)
			}
			downstream, err := newSessionDownstream(request, &profile)
			if err != nil {
				t.Fatal(err)
			}
			encoded, err := json.Marshal(downstream.ClientData())
			if err != nil {
				t.Fatal(err)
			}
			var data login.ClientData
			if err := json.Unmarshal(encoded, &data); err != nil {
				t.Fatal(err)
			}
			if data.PlatformType != test.want {
				t.Fatalf("serialized PlatformType = %d, want %d", data.PlatformType, test.want)
			}
			if data.DeviceOS != profile.OS || data.DeviceID != profile.ID || data.DeviceModel != profile.Model {
				t.Fatal("session login changed the persisted device profile")
			}
			if data.ClientRandomID != -7 || data.SkinID != "skin" || string(data.ExtraData["FixtureClaim"]) != `"kept"` {
				t.Fatal("session login changed caller identity, skin or unknown claims")
			}
			wantID := login.DeriveSelfSignedID(data.ClientRandomID, string(profile.ID))
			assertSessionDialerSelfSignedID(t, downstream, wantID)
		})
	}
}
