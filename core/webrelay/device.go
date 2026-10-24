package webrelay

import (
	"strings"

	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
)

// browserDevice uses the browser's reported OS without inventing a hardware model.
// Unrecognized agents keep their own login claims instead of adopting the relay host's device.
func browserDevice(userAgent string, appleMobile bool) *device.Profile {
	var os protocol.DeviceOS
	switch {
	case appleMobile:
		os = protocol.DeviceIOS
	case strings.Contains(userAgent, "Android"):
		os = protocol.DeviceAndroid
	case strings.Contains(userAgent, "iPhone"), strings.Contains(userAgent, "iPad"), strings.Contains(userAgent, "iPod"):
		os = protocol.DeviceIOS
	case strings.Contains(userAgent, "Windows NT"):
		os = protocol.DeviceWin32
	case strings.Contains(userAgent, "Macintosh"), strings.Contains(userAgent, "Mac OS X"):
		os = protocol.DeviceOSX
	case strings.Contains(userAgent, "Linux"), strings.Contains(userAgent, "CrOS"):
		os = protocol.DeviceLinux
	default:
		return nil
	}
	return &device.Profile{OS: os, ID: login.ClientData{DeviceOS: os}.ExpectedDeviceIDFormat().Generate()}
}
