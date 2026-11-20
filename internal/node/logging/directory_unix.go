//go:build unix

package logging

import (
	"os"
	"syscall"
)

// tightenDirectory removes group and other access from an existing log
// directory the current user owns. Earlier releases created logs directories
// with 0755; refusing them would silently stop all logging. The profile root
// keeps the strict check because it holds credentials.
func tightenDirectory(path string) error {
	info, err := os.Lstat(path)
	if err != nil || !info.IsDir() || info.Mode().Perm()&0o077 == 0 {
		return nil
	}
	if info.Sys().(*syscall.Stat_t).Uid != uint32(os.Geteuid()) {
		return nil
	}
	return os.Chmod(path, info.Mode().Perm()&^0o077)
}
