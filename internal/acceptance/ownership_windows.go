package acceptance

import (
	"errors"
	"os"

	"golang.org/x/sys/windows"
)

func lockAcceptanceFiles(path string) (*os.File, error) {
	if info, err := os.Lstat(path); err == nil && !info.Mode().IsRegular() {
		return nil, errors.New("acceptance lock must be a regular file")
	} else if err != nil && !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	file, err := os.OpenFile(path, os.O_RDWR|os.O_CREATE, 0o600)
	if err != nil {
		return nil, err
	}
	if err := windows.LockFileEx(windows.Handle(file.Fd()), windows.LOCKFILE_EXCLUSIVE_LOCK, 0, 1, 0, new(windows.Overlapped)); err != nil {
		_ = file.Close()
		return nil, err
	}
	return file, nil
}
