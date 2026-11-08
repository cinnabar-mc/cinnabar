package authcache

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
)

// A placeholder record is repaired once and then remains a usable persisted profile.
func TestLoadDeviceRepairsInvalidIdentityMaterial(t *testing.T) {
	for _, id := range []string{"", "malformed", strings.Repeat("0", 32), "00000000-0000-0000-0000-000000000000"} {
		t.Run("id="+id, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "device.json")
			if err := os.WriteFile(path, []byte(`{"os":8,"id":"`+id+`"}`), 0o600); err != nil {
				t.Fatal(err)
			}
			first, err := loadDevice(path, device.Profile{OS: protocol.DeviceWin32})
			if err != nil || first.ID == login.DeviceID(id) || first.ID == "" || first.ID == login.DeviceID(strings.Repeat("0", 32)) {
				t.Fatalf("invalid profile was not repaired: %+v, %v", first, err)
			}
			again, err := loadDevice(path, device.Profile{OS: protocol.DeviceWin32})
			if err != nil || again.ID != first.ID {
				t.Fatalf("repaired profile changed: %+v -> %+v, %v", first, again, err)
			}
		})
	}
}
