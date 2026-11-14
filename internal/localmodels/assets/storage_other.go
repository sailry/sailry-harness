//go:build !windows

package assets

import "path/filepath"

func directoryPath(path string) (string, error) {
	resolved, err := filepath.EvalSymlinks(path)
	if err != nil {
		return "", storage(err)
	}
	if resolved != path {
		return "", storageMessage("model asset directory path is invalid")
	}
	return resolved, nil
}
