//go:build linux && !android

package authcache

import (
	"os"
	"strings"
)

// nativeDeviceModel reads the hardware product name when firmware exposes it.
func nativeDeviceModel() string {
	model, _ := os.ReadFile("/sys/class/dmi/id/product_name")
	value := strings.TrimSpace(string(model))
	if value == "To Be Filled By O.E.M." || value == "Default string" {
		return ""
	}
	return value
}
