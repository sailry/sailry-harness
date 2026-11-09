//go:build unix

package core

import (
	"errors"
	"os"
	"path/filepath"
	"syscall"

	"golang.org/x/sys/unix"
)

func PrivateDirectory(path string) error {
	if err := os.MkdirAll(path, 0o700); err != nil {
		return err
	}
	info, err := os.Lstat(path)
	if err != nil {
		return err
	}
	if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return invalidProfile(path, "data directory must not be a symbolic link")
	}
	if info.Sys().(*syscall.Stat_t).Uid != uint32(os.Geteuid()) {
		return invalidProfile(path, "data directory must belong to the current user")
	}
	if info.Mode().Perm()&0o077 != 0 {
		return invalidProfile(path, "data directory permissions must exclude group and other users")
	}
	return nil
}

func openProfileLock(path string) (*os.File, error) {
	lock, err := os.OpenFile(filepath.Join(path, "node.lock"), os.O_RDWR|os.O_CREATE|unix.O_NOFOLLOW, 0o600)
	if err != nil {
		return nil, err
	}
	if err := unix.Flock(int(lock.Fd()), unix.LOCK_EX|unix.LOCK_NB); err != nil {
		_ = lock.Close()
		if errors.Is(err, unix.EWOULDBLOCK) || errors.Is(err, unix.EAGAIN) {
			return nil, &ProfileInUseError{Path: path}
		}
		return nil, err
	}
	return lock, nil
}

func unlockProfile(lock *os.File) error {
	return unix.Flock(int(lock.Fd()), unix.LOCK_UN)
}
