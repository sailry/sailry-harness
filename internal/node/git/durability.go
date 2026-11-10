package git

import (
	"errors"
	"os"
	"path/filepath"
	"strings"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
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
func syncBlob(r *gogit.Repository, hash plumbing.Hash) error {
	dir, err := commonDir(r)
	if err != nil {
		return err
	}
	hex := hash.String()
	directory := filepath.Join(dir, "objects", hex[:2])
	file, err := os.Open(filepath.Join(directory, hex[2:]))
	if err != nil {
		return mapError(err)
	}
	err = file.Sync()
	file.Close()
	if err != nil {
		return mapError(err)
	}
	if err := syncDirectory(directory); err != nil {
		return err
	}
	return syncDirectory(filepath.Join(dir, "objects"))
}
