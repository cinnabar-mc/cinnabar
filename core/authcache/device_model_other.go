//go:build !darwin && !ios && !windows && !linux && !android

package authcache

// nativeDeviceModel leaves unsupported hardware unreported instead of inventing a model.
func nativeDeviceModel() string { return "" }
