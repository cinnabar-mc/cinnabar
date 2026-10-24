package authcache

import (
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/lockfile"

	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
)

// maxDeviceProfileBytes bounds the saved profile, a few short JSON strings.
const maxDeviceProfileBytes = 4 << 10

// deviceLeaseTimeout bounds the wait for another core initializing the same profile.
const deviceLeaseTimeout = 10 * time.Second

// LoadDevice reports the host OS and hardware model, keeping the install's saved device ID.
// Cores sharing path hold a lease across profile updates. An empty path keeps a new ID in memory. The
// error reports a profile that could not be saved; the profile is still usable.
func LoadDevice(path string) (device.Profile, error) {
	return loadDevice(path, device.Profile{OS: nativeDeviceOS(runtime.GOOS), Model: strings.TrimSpace(nativeDeviceModel())})
}

// loadDevice refreshes host claims and reuses a saved ID when its format matches the host.
func loadDevice(path string, profile device.Profile) (device.Profile, error) {
	format := login.ClientData{DeviceOS: profile.OS}.ExpectedDeviceIDFormat()
	if format == login.DeviceIDFormatInvalid {
		format = login.DeviceIDFormatLowerHexString
	}
	profile.ID = format.Generate()
	if path == "" {
		return profile, nil
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return profile, err
	}
	lease, err := lockfile.Acquire(path+".lock", deviceLeaseTimeout)
	if err != nil {
		if saved, ok := readDevice(path); ok && saved.ID.Format() == format {
			profile.ID = saved.ID
			return profile, nil
		}
		return profile, err
	}
	defer lease.Close()
	if saved, ok := readDevice(path); ok && saved.ID.Format() == format {
		profile.ID = saved.ID
		if saved == profile {
			return profile, nil
		}
	}
	serialized, err := json.Marshal(profile)
	if err != nil {
		return profile, err
	}
	return profile, savePrivate(path, serialized)
}

// readDevice reads a saved profile without treating an old OS or fabricated model as authoritative.
func readDevice(path string) (device.Profile, bool) {
	saved, err := loadPrivate(path, maxDeviceProfileBytes)
	if err != nil {
		return device.Profile{}, false
	}
	var profile device.Profile
	if json.Unmarshal(saved, &profile) != nil || profile.ID == "" {
		return device.Profile{}, false
	}
	return profile, true
}

// nativeDeviceOS maps the core's build target to the Bedrock platform it runs on.
func nativeDeviceOS(goos string) protocol.DeviceOS {
	switch goos {
	case "android":
		return protocol.DeviceAndroid
	case "ios":
		return protocol.DeviceIOS
	case "darwin":
		return protocol.DeviceOSX
	case "windows":
		return protocol.DeviceWin32
	case "linux":
		return protocol.DeviceLinux
	default:
		return protocol.DeviceUnknown
	}
}
