//go:build darwin || ios

package authcache

import (
	"runtime"

	"golang.org/x/sys/unix"
)

// nativeDeviceModel reads Apple's hardware model identifier without a hostname or serial number.
func nativeDeviceModel() string {
	property := "hw.model"
	if runtime.GOOS == "ios" {
		property = "hw.machine"
	}
	model, _ := unix.Sysctl(property)
	return model
}
