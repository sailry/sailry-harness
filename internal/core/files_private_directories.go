package core

import (
	"errors"
	"io/fs"
	"os"
)

// CreatePrivateDirectories prepares Node-owned resource storage. It is not an
// entry action and never runs against a controller-selected destination.
func (files *Files) CreatePrivateDirectories(relative string) error {
	parts, err := fileComponents(relative, false, files.access, true)
	if err != nil {
		return err
	}
	current, err := files.root.OpenRoot(".")
	if err != nil {
		return fileIOFault(err)
	}
	defer func() { current.Close() }()
	for _, name := range parts {
		if err = current.Mkdir(name, 0o700); err != nil && !errors.Is(err, fs.ErrExist) {
			return fileIOFault(err)
		}
		metadata, err := current.Lstat(name)
		if err != nil {
			return fileIOFault(err)
		}
		if !metadata.IsDir() || metadata.Mode()&os.ModeSymlink != 0 {
			return fileConflict()
		}
		next, err := descendFiles(current, []string{name})
		if err != nil {
			return err
		}
		if err = next.Chmod(".", 0o700); err != nil {
			next.Close()
			return fileIOFault(err)
		}
		current.Close()
		current = next
	}
	return nil
}
