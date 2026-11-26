package core

import (
	"context"
	"errors"
	"io/fs"
	"os"
	"strings"

	"golang.org/x/text/unicode/norm"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// SameFileEntry distinguishes alternate spellings of a directory entry from
// distinct hard links. A move may overwrite a hard link, but must never unlink
// its destination when the two spellings resolve to the same entry.
func (files *Files) SameFileEntry(from, to string) (bool, error) {
	source, name, err := files.parent(from, true)
	if isFileFault(err, protocol.ErrorCodeNotFound) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	defer source.Close()
	destination, renamed, err := files.parent(to, true)
	if isFileFault(err, protocol.ErrorCodeNotFound) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	defer destination.Close()
	before, err := source.Lstat(name)
	if errors.Is(err, fs.ErrNotExist) {
		return false, nil
	}
	if err != nil {
		return false, fileIOFault(err)
	}
	after, err := destination.Lstat(renamed)
	if errors.Is(err, fs.ErrNotExist) {
		return false, nil
	}
	if err != nil {
		return false, fileIOFault(err)
	}
	return sameMoveEntry(source, name, before, destination, renamed, after)
}

func sameMoveEntry(source *os.Root, name string, before fs.FileInfo, destination *os.Root, renamed string, after fs.FileInfo) (bool, error) {
	if !os.SameFile(before, after) {
		return false, nil
	}
	parent, err := source.Stat(".")
	if err != nil {
		return false, fileIOFault(err)
	}
	other, err := destination.Stat(".")
	if err != nil {
		return false, fileIOFault(err)
	}
	if !os.SameFile(parent, other) {
		return false, nil
	}
	if name == renamed {
		return true, nil
	}
	dir, err := source.Open(".")
	if err != nil {
		return false, fileIOFault(err)
	}
	defer dir.Close()
	names, err := dir.Readdirnames(-1)
	if err != nil {
		return false, fileIOFault(err)
	}
	onDisk := func(spelling string) string {
		folded := norm.NFC.String(spelling)
		found := ""
		for _, actual := range names {
			if spelling == actual {
				return actual
			}
			if strings.EqualFold(folded, norm.NFC.String(actual)) {
				found = actual
			}
		}
		return found
	}
	a, b := onDisk(name), onDisk(renamed)
	if a == "" || b == "" {
		return false, fileFault(protocol.ErrorCodeConflict, "could not distinguish the source and destination directory entries")
	}
	return a == b, nil
}

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
	if existing, err := destination.Lstat(renamed); err == nil {
		same, err := sameMoveEntry(source, name, before, destination, renamed, existing)
		if err != nil {
			return err
		}
		if !same {
			return fileFault(protocol.ErrorCodeConflict, "an entry already exists at this path")
		}
	} else if !errors.Is(err, fs.ErrNotExist) {
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
