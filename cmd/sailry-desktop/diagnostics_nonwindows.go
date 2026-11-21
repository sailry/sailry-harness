//go:build !windows

package main

func nativeWindowProbe() func() (bool, bool) { return nil }
