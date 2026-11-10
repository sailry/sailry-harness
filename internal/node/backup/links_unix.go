//go:build unix

package backup

import (
	"io/fs"
	"syscall"
)

func regularDatabase(info fs.FileInfo) error {
	if !info.Mode().IsRegular() {
		return invalid("backup database must be a regular file")
	}
	if status, ok := info.Sys().(*syscall.Stat_t); ok && status.Nlink != 1 {
		return invalid("backup database must not be hard-linked")
	}
	return nil
}
