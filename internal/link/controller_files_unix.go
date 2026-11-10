//go:build unix

package link

import (
	"errors"
	"os"
	"path/filepath"
	"syscall"

	"golang.org/x/sys/unix"
)

func prepareControllerFiles(path string) (string, *os.File, error) {
	if err := os.MkdirAll(path, 0o700); err != nil {
		return "", nil, err
	}
	metadata, err := os.Lstat(path)
	if err != nil {
		return "", nil, err
	}
	if !metadata.IsDir() || metadata.Mode()&os.ModeSymlink != 0 || metadata.Mode().Perm()&0o077 != 0 || metadata.Sys().(*syscall.Stat_t).Uid != uint32(os.Geteuid()) {
		return "", nil, errors.New("controller directory must be private and owned by the current user")
	}
	directory, err := filepath.EvalSymlinks(path)
	if err != nil {
		return "", nil, err
	}
	lock, err := os.OpenFile(filepath.Join(directory, "node.lock"), os.O_RDWR|os.O_CREATE|unix.O_NOFOLLOW, 0o600)
	if err != nil {
		return "", nil, err
	}
	information, err := lock.Stat()
	if err != nil || !information.Mode().IsRegular() {
		_ = lock.Close()
		return "", nil, errors.New("controller lock must be a regular file")
	}
	if err := unix.Flock(int(lock.Fd()), unix.LOCK_EX|unix.LOCK_NB); err != nil {
		_ = lock.Close()
		return "", nil, errors.New("controller profile is already in use")
	}
	return directory, lock, nil
}

func readControllerFile(path string) (*os.File, error) {
	file, err := os.OpenFile(path, os.O_RDONLY|unix.O_NOFOLLOW, 0)
	if err != nil {
		return nil, err
	}
	metadata, err := file.Stat()
	if err == nil && (!metadata.Mode().IsRegular() || metadata.Mode().Perm()&0o077 != 0) {
		err = errors.New("invalid private peer file")
	}
	if err != nil {
		_ = file.Close()
		return nil, err
	}
	return file, nil
}

func validateControllerIdentity(string) error { return nil }

func replaceControllerFile(source, target string) error { return os.Rename(source, target) }

func syncControllerDirectory(path string) error {
	directory, err := os.Open(path)
	if err != nil {
		return err
	}
	return errors.Join(directory.Sync(), directory.Close())
}
