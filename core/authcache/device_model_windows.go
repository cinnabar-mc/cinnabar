package authcache

import "golang.org/x/sys/windows/registry"

// nativeDeviceModel reads the firmware product name without collecting identifying registry values.
func nativeDeviceModel() string {
	key, err := registry.OpenKey(registry.LOCAL_MACHINE, `HARDWARE\DESCRIPTION\System\BIOS`, registry.QUERY_VALUE)
	if err != nil {
		return ""
	}
	defer key.Close()
	model, _, _ := key.GetStringValue("SystemProductName")
	return model
}
