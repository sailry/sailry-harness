package core

import (
	"context"
	"errors"
	"io/fs"
	"os"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// MoveEntryTo prefers the confined native rename so same-filesystem moves keep
// identity, timestamps and links; cross-filesystem moves use copy-before-Trash.
func (files *Files) MoveEntryTo(ctx context.Context, target *Files, from, to string) error {
	source, name, err := files.parent(from, true)
	if err != nil {
		return err
	}
	defer source.Close()
	destination, renamed, err := target.parent(to, true)
	if err != nil {
		return err
	}
	defer destination.Close()
	before, err := source.Lstat(name)
	if err != nil {
		return fileIOFault(err)
	}
	sourceParent, err := source.Stat(".")
	if err != nil {
		return fileIOFault(err)
	}
	destinationParent, err := destination.Stat(".")
	if err != nil {
		return fileIOFault(err)
	}
	if name == renamed && os.SameFile(sourceParent, destinationParent) {
		return nil
	}
	if _, err = destination.Lstat(renamed); !errors.Is(err, fs.ErrNotExist) {
		if err == nil {
			return fileFault(protocol.ErrorCodeConflict, "an entry already exists at this path")
		}
		return fileIOFault(err)
	}
	if err = ctx.Err(); err != nil {
		return err
	}
	if err = files.validateEntryParent(from, sourceParent); err != nil {
		return err
	}
	if err = target.validateEntryParent(to, destinationParent); err != nil {
		return err
	}
	err = renameFileBetween(source, name, destination, renamed)
	if err != nil {
		if fileCrossDevice(err) {
			return files.CopyEntryTo(ctx, target, from, to, true)
		}
		return fileIOFault(err)
	}
	after, err := destination.Lstat(renamed)
	if err != nil || !os.SameFile(before, after) {
		return copyUnknown()
	}
	if err = files.validateEntryParent(from, sourceParent); err != nil {
		return copyUnknown()
	}
	if err = target.validateEntryParent(to, destinationParent); err != nil {
		return copyUnknown()
	}
	return nil
}
