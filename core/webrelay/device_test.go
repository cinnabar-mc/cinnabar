package webrelay

import (
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/protocol"
)

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
		got := browserDevice(agent)
		if got == nil || got.OS != want || got.Model != "" || got.ID == "" {
			t.Fatalf("browser device = %+v, want OS %v without a fabricated model", got, want)
		}
	}
	if got := browserDevice("unknown browser"); got != nil {
		t.Fatalf("unknown browser adopted a device: %+v", got)
	}
}
