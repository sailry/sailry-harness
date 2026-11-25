package process

import (
	"errors"
	"os"
	"path/filepath"
)

// CaptureWorkspaceBoundary protects existing child checkouts without creating them.
func CaptureWorkspaceBoundary(root string) (string, error) {
	anchor, err := os.OpenRoot(root)
	if err != nil {
		return "", err
	}
	defer anchor.Close()
	info, err := anchor.Lstat(".sailry")
	if errors.Is(err, os.ErrNotExist) || err == nil && (!info.IsDir() || info.Mode()&os.ModeSymlink != 0) {
		return "", nil
	}
	if err != nil {
		return "", err
	}
	directory, err := anchor.OpenRoot(".sailry")
	if err != nil {
		return "", err
	}
	defer directory.Close()
	info, err = directory.Lstat("worktrees")
	if errors.Is(err, os.ErrNotExist) || err == nil && (!info.IsDir() || info.Mode()&os.ModeSymlink != 0) {
		return "", nil
	}
	if err != nil {
		return "", err
	}
	return filepath.Join(root, ".sailry", "worktrees"), nil
}
