package git

import (
	"errors"
	"os"
	"path/filepath"
	"strings"

	gogit "github.com/go-git/go-git/v5"
)

func commonDir(r *gogit.Repository) (string, error) {
	dir, err := gitDir("", r)
	if err != nil {
		return "", err
	}
	data, err := os.ReadFile(filepath.Join(dir, "commondir"))
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return dir, nil
		}
		return "", mapError(err)
	}
	common := strings.TrimSpace(string(data))
	if !filepath.IsAbs(common) {
		common = filepath.Join(dir, common)
	}
	return filepath.EvalSymlinks(common)
}
