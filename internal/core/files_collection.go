package core

import (
	"context"
	"os"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// RemoveStoredFile collects an unreferenced ordinary profile file. User entry
// commands continue to use native Trash and revision-bound removal.
func (files *Files) RemoveStoredFile(ctx context.Context, relative string) error {
	parent, name, err := files.parent(relative, true)
	if err != nil {
		return err
	}
	defer parent.Close()
	identity, err := parent.Stat(".")
	if err != nil {
		return fileIOFault(err)
	}
	metadata, err := parent.Lstat(name)
	if err != nil {
		return fileIOFault(err)
	}
	if !metadata.Mode().IsRegular() {
		return fileFault(protocol.ErrorCodeInvalidRequest, "stored file is not an ordinary file")
	}
	file, err := openRegularFile(parent, name)
	if err != nil {
		return err
	}
	defer file.Close()
	retained, err := file.Stat()
	if err != nil {
		return fileIOFault(err)
	}
	if !os.SameFile(metadata, retained) {
		return fileConflict()
	}
	if err = ctx.Err(); err != nil {
		return err
	}
	if err = files.validateEntryParent(relative, identity); err != nil {
		return err
	}
	if err = parent.Remove(name); err != nil {
		return fileIOFault(err)
	}
	return nil
}
