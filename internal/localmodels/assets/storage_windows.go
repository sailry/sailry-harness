package assets

import (
	"os"
	"path/filepath"
	"syscall"
)

func directoryPath(path string) (string, error) {
	// EvalSymlinks also expands 8.3 names and restores on-disk casing. Check
	// the original components for reparse points before normalizing those aliases.
	for current := path; ; {
		info, err := os.Lstat(current)
		if err != nil {
			return "", storage(err)
		}
		if info.Sys().(*syscall.Win32FileAttributeData).FileAttributes&syscall.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
			return "", storageMessage("model asset directory path is invalid")
		}
		parent := filepath.Dir(current)
		if parent == current {
			break
		}
		current = parent
	}
	resolved, err := filepath.EvalSymlinks(path)
	if err != nil {
		return "", storage(err)
	}
	return resolved, nil
}
