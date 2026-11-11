//go:build !windows && !linux

package externalbrowser

import "os/exec"

// The browser exits by itself when the Node's end of its pipe closes.
func configurePlatform(*exec.Cmd) {}
