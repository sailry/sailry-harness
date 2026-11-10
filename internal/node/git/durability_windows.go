//go:build windows

package git

// Windows file Sync flushes the filesystem-managed object before index rename.
// Directory handles do not support os.File.Sync on this platform.
func syncDirectory(string) error { return nil }
