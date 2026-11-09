package authcache

import (
	"context"
	"os/exec"
	"time"
)

// nativeDeviceModel reads Android's public product model, leaving it empty when unavailable.
func nativeDeviceModel() string {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	model, _ := exec.CommandContext(ctx, "/system/bin/getprop", "ro.product.model").Output()
	return string(model)
}
