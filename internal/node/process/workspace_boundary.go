package process

import (
	"errors"
	"os"
	"path/filepath"
)

// CaptureWorkspaceBoundary protects existing child checkouts without preparing
// Git metadata or creating managed directories when no Git service is assembled.
func CaptureWorkspaceBoundary(root string) (string, error) {
	anchor, err := os.OpenRoot(root)
	if err != nil {
		return "", err
	}
	defer anchor.Close()
	for _, name := range []string{".sailry", filepath.Join(".sailry", "worktrees")} {
		info, err := anchor.Lstat(name)
		if errors.Is(err, os.ErrNotExist) {
			return "", nil
		}
		if err != nil {
			return "", err
		}
		if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
			return "", nil
		}
	}
	return filepath.Join(root, ".sailry", "worktrees"), nil
}
