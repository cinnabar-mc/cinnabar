package authcache

import (
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
)

func TestDeviceOSMatchesTheHost(t *testing.T) {
	for goos, want := range map[string]protocol.DeviceOS{
		"android": protocol.DeviceAndroid, "ios": protocol.DeviceIOS, "darwin": protocol.DeviceOSX,
		"windows": protocol.DeviceWin32, "linux": protocol.DeviceLinux, "unsupported": protocol.DeviceUnknown,
	} {
		if got := nativeDeviceOS(goos); got != want {
			t.Errorf("%s claims OS %v, want %v", goos, got, want)
		}
	}
}

// One install keeps one device across launches; a profile for another platform is replaced.
func TestLoadDeviceKeepsOneProfilePerInstall(t *testing.T) {
	path := filepath.Join(t.TempDir(), "device.json")
	first, err := LoadDevice(path)
	if err != nil || first.OS != nativeDeviceOS(runtime.GOOS) || first.ID == "" {
		t.Fatalf("first profile = %+v, %v", first, err)
	}
	again, err := LoadDevice(path)
	if err != nil || again != first {
		t.Fatalf("reloaded profile = %+v, %v; want %+v", again, err, first)
	}
	if err := os.WriteFile(path, []byte(`{"os":7,"model":"XboxOne","id":"x"}`), 0o600); err != nil {
		t.Fatal(err)
	}
	replaced, err := LoadDevice(path)
	if err != nil || replaced.OS != nativeDeviceOS(runtime.GOOS) || replaced.ID == "x" || replaced.ID == "" {
		t.Fatalf("replaced profile = %+v, %v", replaced, err)
	}
	if memory, err := LoadDevice(""); err != nil || memory.ID == "" || memory.OS != first.OS || memory.Model != first.Model {
		t.Fatalf("in-memory profile = %+v, %v", memory, err)
	}
}

func TestLoadDeviceReplacesSpoofedClaimsAndKeepsInstallID(t *testing.T) {
	path := filepath.Join(t.TempDir(), "device.json")
	const id = "00112233445566778899aabbccddeeff"
	if err := os.WriteFile(path, []byte(`{"os":1,"model":"SAMSUNG SM-G930W8","id":"`+id+`"}`), 0o600); err != nil {
		t.Fatal(err)
	}
	for _, host := range []device.Profile{
		{OS: protocol.DeviceOSX, Model: "Actual Mac hardware"},
		{OS: protocol.DeviceWin32, Model: "Actual PC hardware"},
		{OS: protocol.DeviceLinux, Model: ""},
	} {
		got, err := loadDevice(path, host)
		if err != nil || got.OS != host.OS || got.Model != host.Model || got.ID != id {
			t.Fatalf("profile = %+v, %v; want host %+v and stable ID", got, err, host)
		}
		saved, ok := readDevice(path)
		if !ok || saved != got {
			t.Fatalf("saved profile = %+v, %t; want %+v", saved, ok, got)
		}
	}
}

func TestLoadDeviceReplacesIncompatibleIDFormat(t *testing.T) {
	path := filepath.Join(t.TempDir(), "device.json")
	if err := os.WriteFile(path, []byte(`{"os":1,"model":"old phone","id":"00112233445566778899aabbccddeeff"}`), 0o600); err != nil {
		t.Fatal(err)
	}
	got, err := loadDevice(path, device.Profile{OS: protocol.DeviceIOS, Model: "Actual iPhone hardware"})
	if err != nil || got.ID.Format() != login.DeviceIDFormatUpperHexString {
		t.Fatalf("profile = %+v, %v; want iOS device ID format", got, err)
	}
}

// Cores starting together on a fresh install all claim the one device that is saved.
func TestLoadDeviceAgreesAcrossConcurrentFirstLoads(t *testing.T) {
	path := filepath.Join(t.TempDir(), "device.json")
	profiles := make(chan device.Profile, 16)
	var started sync.WaitGroup
	for range cap(profiles) {
		started.Add(1)
		go func() {
			defer started.Done()
			profile, err := LoadDevice(path)
			if err != nil {
				t.Error(err)
			}
			profiles <- profile
		}()
	}
	started.Wait()
	close(profiles)
	saved, err := LoadDevice(path)
	if err != nil {
		t.Fatal(err)
	}
	for profile := range profiles {
		if profile != saved {
			t.Fatalf("a core claimed %+v while %+v was saved", profile, saved)
		}
	}
}
