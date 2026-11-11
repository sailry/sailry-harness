//go:build unix

package files

import "golang.org/x/sys/unix"

// readable reports whether the Node's account may list directory. It asks
// for the permission without opening the directory, so macOS does not prompt
// for the Desktop, Documents and Downloads folders it guards.
func readable(directory string) bool { return unix.Access(directory, unix.R_OK|unix.X_OK) == nil }
