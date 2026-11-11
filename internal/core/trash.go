// Trash relocation behavior is adapted from harbor-file-service 727ce0a
// (Apache-2.0).
package core

import (
	"context"
	"errors"
	"io/fs"
	"os"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Trash rechecks the retained scope around a native move to the recovery bin.
// Native APIs accept paths, so concurrent external writers are not isolated.
func (files *Files) Trash(relative string) error {
	return files.relocateTrash(relative, moveToTrash)
}

func (files *Files) RemoveVerified(ctx context.Context, relative, revision, stamp string) error {
	return files.checkedTrash(ctx, relative, revision, stamp, moveToTrash)
}

func (files *Files) checkedTrash(ctx context.Context, relative, revision, stamp string, operation func(string) error) error {
	if !validFileRevision(revision) || !validFileRevision(stamp) {
		return fileFault(protocol.ErrorCodeInvalidRequest, "invalid file source revision")
	}
	return files.relocateTrash(relative, func(target string) error {
		if err := files.VerifySource(ctx, relative, revision, stamp); err != nil {
			return err
		}
		return operation(target)
	})
}

func (files *Files) relocateTrash(relative string, operation func(string) error) error {
	parent, name, err := files.parent(relative, true)
	if err != nil {
		return err
	}
	defer parent.Close()
	if _, err := parent.Lstat(name); err != nil {
		return fileIOFault(err)
	}
	rootID, err := trashDirectoryIdentity(files.root)
	if err != nil {
		return err
	}
	parentID, err := trashDirectoryIdentity(parent)
	if err != nil {
		return err
	}
	validate := func() error {
		root, err := acquireFileRoot(files.path)
		if err != nil {
			return err
		}
		defer root.Close()
		current := Files{root: root, access: files.access}
		currentRoot, err := trashDirectoryIdentity(current.root)
		if err != nil {
			return err
		}
		currentParent, _, err := current.parent(relative, true)
		if err != nil {
			return err
		}
		defer currentParent.Close()
		currentID, err := trashDirectoryIdentity(currentParent)
		if err != nil {
			return err
		}
		if rootID != currentRoot || parentID != currentID {
			return fileFault(protocol.ErrorCodeRevisionConflict, "entry parent changed")
		}
		return nil
	}
	if err := validate(); err != nil {
		return err
	}
	if err := operation(filepath.Join(files.path, filepath.FromSlash(relative))); err != nil {
		return err
	}
	if err := validate(); err != nil {
		return trashUnknown()
	}
	if _, err := parent.Lstat(name); errors.Is(err, fs.ErrNotExist) {
		return nil
	}
	return trashUnknown()
}

func trashDirectoryIdentity(root *os.Root) (nativeFileIdentity, error) {
	directory, err := root.Open(".")
	if err != nil {
		return nativeFileIdentity{}, fileIOFault(err)
	}
	defer directory.Close()
	return fileNativeIdentity(directory)
}

func moveToTrash(target string) error {
	if _, err := trashNative(target); err != nil {
		return trashUnknown()
	}
	return nil
}

func trashUnknown() error {
	return fileFault(protocol.ErrorCodeOutcomeUnknown, "move to Trash could not be confirmed; inspect the entry and Trash before retrying")
}
