//go:build !windows

package store

import (
	"os"
	"syscall"

	"github.com/sailry/sailry-harness/internal/core"
	"golang.org/x/sys/unix"
)

func checkDatabaseLinks(path string, info os.FileInfo) error {
	if info.Sys().(*syscall.Stat_t).Nlink != 1 {
		return &core.InvalidProfileError{Path: path, Reason: "database must not be hard-linked"}
	}
	return nil
}

func validateDatabasePrivacy(string) error { return nil }

func createDatabaseFile(path string) error {
	descriptor, err := unix.Open(path, unix.O_RDWR|unix.O_CREAT|unix.O_CLOEXEC|unix.O_NOFOLLOW, 0o600)
	if err != nil {
		return err
	}
	file := os.NewFile(uintptr(descriptor), path)
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return err
	}
	if !info.Mode().IsRegular() {
		return &core.InvalidProfileError{Path: path, Reason: "database must be a regular file"}
	}
	return checkDatabaseLinks(path, info)
}
