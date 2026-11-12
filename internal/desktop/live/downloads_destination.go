package live

import (
	"context"
	"errors"
	"io/fs"
	"os"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type downloadDestination struct {
	path, parentPath, originalParent, name, temporary string
	root                                              *os.Root
	file                                              *os.File
	expected, identity, parent                        fs.FileInfo
	published                                         bool
}

func prepareDownloadDestination(path string) (*downloadDestination, error) {
	if !filepath.IsAbs(path) || filepath.Base(path) == string(filepath.Separator) {
		return nil, fileActionFault(protocol.ErrorCodeInvalidRequest, "an absolute download destination is required")
	}
	parent, err := filepath.EvalSymlinks(filepath.Dir(path))
	if err != nil {
		return nil, err
	}
	root, err := os.OpenRoot(parent)
	if err != nil {
		return nil, err
	}
	target := &downloadDestination{path: path, parentPath: parent, originalParent: filepath.Dir(path), root: root, name: filepath.Base(path), temporary: ".sailry-download-" + protocol.NewStreamID().String()}
	success := false
	defer func() {
		if !success {
			target.close()
		}
	}()
	target.parent, err = root.Stat(".")
	if err != nil {
		return nil, err
	}
	target.expected, err = root.Lstat(target.name)
	if errors.Is(err, fs.ErrNotExist) {
		target.expected = nil
	} else if err != nil {
		return nil, err
	} else if !target.expected.Mode().IsRegular() {
		return nil, fileActionFault(protocol.ErrorCodeInvalidRequest, "download destination is not an ordinary file")
	}
	target.file, err = root.OpenFile(target.temporary, os.O_RDWR|os.O_CREATE|os.O_EXCL, 0o600)
	if err != nil {
		return nil, err
	}
	target.identity, err = target.file.Stat()
	if err != nil {
		return nil, err
	}
	success = true
	return target, nil
}
func (target *downloadDestination) publish(ctx context.Context) error {
	if err := target.file.Sync(); err != nil {
		return err
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	currentParent, err := filepath.EvalSymlinks(target.originalParent)
	if err != nil || currentParent != target.parentPath {
		return fileActionFault(protocol.ErrorCodeRevisionConflict, "download destination changed")
	}
	parent, err := os.Stat(target.parentPath)
	if err != nil || !os.SameFile(parent, target.parent) {
		return fileActionFault(protocol.ErrorCodeRevisionConflict, "download destination parent changed")
	}
	current, err := target.root.Lstat(target.name)
	if target.expected == nil {
		if !errors.Is(err, fs.ErrNotExist) {
			return fileActionFault(protocol.ErrorCodeRevisionConflict, "download destination changed")
		}
	} else {
		if err != nil || !sameDownloadStamp(target.expected, current) {
			return fileActionFault(protocol.ErrorCodeRevisionConflict, "download destination changed")
		}
	}
	if err = target.file.Close(); err != nil {
		return err
	}
	if target.expected == nil {
		err = target.root.Link(target.temporary, target.name)
		if err == nil {
			_ = target.root.Remove(target.temporary)
		} else if errors.Is(err, errors.ErrUnsupported) || errors.Is(err, fs.ErrPermission) {
			// A filesystem without hard links (FAT, exFAT, some shares)
			// refuses Link. The name was absent a moment ago, so the
			// temporary file is renamed onto it.
			err = target.root.Rename(target.temporary, target.name)
		}
	} else {
		err = target.root.Rename(target.temporary, target.name)
	}
	if err != nil {
		if errors.Is(err, fs.ErrExist) {
			return fileActionFault(protocol.ErrorCodeRevisionConflict, "download destination changed")
		}
		return err
	}
	target.published = true
	published, err := target.root.Lstat(target.name)
	if err != nil || !os.SameFile(published, target.identity) {
		return fileActionFault(protocol.ErrorCodeOutcomeUnknown, "download publication could not be confirmed")
	}
	return nil
}
func sameDownloadStamp(before, after fs.FileInfo) bool {
	return after != nil && after.Mode().IsRegular() && os.SameFile(before, after) && before.Size() == after.Size() && before.ModTime().Equal(after.ModTime())
}
func (target *downloadDestination) close() {
	if target == nil {
		return
	}
	if target.file != nil {
		_ = target.file.Close()
	}
	if target.root != nil {
		if target.identity != nil {
			if current, err := target.root.Lstat(target.temporary); err == nil && os.SameFile(current, target.identity) {
				_ = target.root.Remove(target.temporary)
			}
		}
		_ = target.root.Close()
	}
}
