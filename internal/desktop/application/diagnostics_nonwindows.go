//go:build !windows

package application

func nativeWindowProbe() func() (bool, bool) { return nil }
