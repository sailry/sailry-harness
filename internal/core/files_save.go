// Portions adapted from sailry-code 67ae9fa0 (Apache-2.0).

package core

import (
	"context"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"strings"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type originalFile struct {
	revision    string
	identity    fs.FileInfo
	permissions fs.FileMode
}

// FileTarget retains a publication directory through revision-checked writes.
type FileTarget struct {
	files     *Files
	relative  string
	parent    *os.Root
	parentID  fs.FileInfo
	rootID    fs.FileInfo
	original  *originalFile
	limit     uint64
	control   fileControl
	writeHook func(bool)
}

func (files *Files) PrepareFileTarget(ctx context.Context, relative string, expected *string, limit uint64) (*FileTarget, error) {
	target, err := files.fileTarget(ctx, relative, limit)
	if err != nil {
		return nil, err
	}
	parent, name := target.parent, baseFileName(relative)
	if expected != nil {
		file, revision, err := currentFile(parent, name, limit, target.control)
		if err != nil {
			target.Close()
			return nil, err
		}
		defer file.Close()
		if revision != *expected {
			target.Close()
			return nil, fileConflict()
		}
		metadata, err := file.Stat()
		if err != nil {
			target.Close()
			return nil, fileIOFault(err)
		}
		if fileReadonly(metadata) {
			target.Close()
			return nil, fileFault(protocol.ErrorCodePermissionDenied, "file is read-only")
		}
		target.original = &originalFile{revision: revision, identity: metadata, permissions: metadata.Mode()}
	} else if _, err := parent.Lstat(name); !errors.Is(err, fs.ErrNotExist) {
		target.Close()
		if err != nil {
			return nil, fileIOFault(err)
		}
		return nil, fileConflict()
	}
	return target, nil
}

func (files *Files) fileTarget(ctx context.Context, relative string, limit uint64) (*FileTarget, error) {
	parent, _, err := files.parent(relative, true)
	if err != nil {
		return nil, err
	}
	parentID, err := parent.Stat(".")
	if err != nil {
		parent.Close()
		return nil, fileIOFault(err)
	}
	rootID, err := files.root.Stat(".")
	if err != nil {
		parent.Close()
		return nil, fileIOFault(err)
	}
	target := &FileTarget{files: files, relative: relative, parent: parent, parentID: parentID, rootID: rootID, limit: limit, control: fileControl{context: ctx}}
	return target, nil
}

func (target *FileTarget) Close() error { return target.parent.Close() }

func (files *Files) Save(ctx context.Context, relative, text string, expected *string) (protocol.FileWritten, error) {
	if len(text) > protocol.MAX_FILE_BYTES || strings.IndexByte(text, 0) >= 0 || expected != nil && !validFileRevision(*expected) {
		return protocol.FileWritten{}, fileFault(protocol.ErrorCodeInvalidRequest, "invalid file content or revision")
	}
	if expected == nil {
		if err := files.createParents(relative); err != nil {
			return protocol.FileWritten{}, err
		}
	}
	target, err := files.PrepareFileTarget(ctx, relative, expected, protocol.MAX_EDIT_BYTES)
	if err != nil {
		return protocol.FileWritten{}, err
	}
	return target.Write(strings.NewReader(text), uint64(len(text)), contentRevision([]byte(text)))
}

func (files *Files) RestoreText(ctx context.Context, relative, text, expected string) (protocol.FileWritten, error) {
	if len(text) > protocol.MAX_EDIT_BYTES || strings.IndexByte(text, 0) >= 0 || !validFileRevision(expected) {
		return protocol.FileWritten{}, fileFault(protocol.ErrorCodeInvalidRequest, "invalid checkpoint content or revision")
	}
	target, err := files.PrepareFileTarget(ctx, relative, &expected, protocol.MAX_EDIT_BYTES)
	if err != nil {
		return protocol.FileWritten{}, err
	}
	return target.Write(strings.NewReader(text), uint64(len(text)), contentRevision([]byte(text)))
}

func (target *FileTarget) Write(source io.Reader, size uint64, revision string) (protocol.FileWritten, error) {
	defer target.Close()
	name := baseFileName(target.relative)
	var published fs.FileInfo
	if target.original != nil {
		temporary := ".sailry-save-" + uuid.NewString()
		file, err := target.parent.OpenFile(temporary, os.O_RDWR|os.O_CREATE|os.O_EXCL, 0o600)
		if err != nil {
			return protocol.FileWritten{}, fileIOFault(err)
		}
		defer file.Close()
		temporaryID, err := file.Stat()
		if err != nil {
			return protocol.FileWritten{}, fileIOFault(err)
		}
		defer func() {
			current, err := target.parent.Lstat(temporary)
			if err == nil && os.SameFile(current, temporaryID) {
				target.parent.Remove(temporary)
			}
		}()
		if err := file.Chmod(target.original.permissions); err != nil {
			return protocol.FileWritten{}, fileIOFault(err)
		}
		if err := copyFileContent(source, file, size, revision, target.control); err != nil {
			return protocol.FileWritten{}, err
		}
		if err := file.Sync(); err != nil {
			return protocol.FileWritten{}, fileIOFault(err)
		}
		published, err = file.Stat()
		if err != nil {
			return protocol.FileWritten{}, fileIOFault(err)
		}
		if target.writeHook != nil {
			target.writeHook(false)
		}
		if err := target.validateParent(); err != nil {
			return protocol.FileWritten{}, err
		}
		current, currentRevision, err := currentFile(target.parent, name, target.limit, target.control)
		if err != nil {
			return protocol.FileWritten{}, err
		}
		metadata, err := current.Stat()
		current.Close()
		if err != nil {
			return protocol.FileWritten{}, fileIOFault(err)
		}
		if currentRevision != target.original.revision || !os.SameFile(metadata, target.original.identity) || metadata.Mode() != target.original.permissions {
			return protocol.FileWritten{}, fileConflict()
		}
		if err := file.Close(); err != nil {
			return protocol.FileWritten{}, fileIOFault(err)
		}
		if err := target.parent.Rename(temporary, name); err != nil {
			return protocol.FileWritten{}, fileIOFault(err)
		}
	} else {
		if target.writeHook != nil {
			target.writeHook(false)
		}
		if err := target.validateParent(); err != nil {
			return protocol.FileWritten{}, err
		}
		file, err := target.parent.OpenFile(name, exclusiveFileFlags(), 0o666)
		if err != nil {
			if errors.Is(err, fs.ErrExist) {
				return protocol.FileWritten{}, fileConflict()
			}
			return protocol.FileWritten{}, fileIOFault(err)
		}
		defer file.Close()
		published, err = file.Stat()
		if err != nil {
			return protocol.FileWritten{}, fileUnknown()
		}
		if err := copyFileContent(source, file, size, revision, target.control); err != nil {
			return protocol.FileWritten{}, fileUnknown()
		}
		if err := file.Sync(); err != nil {
			return protocol.FileWritten{}, fileUnknown()
		}
	}
	if target.writeHook != nil {
		target.writeHook(true)
	}
	if err := syncFileDirectory(target.parent); err != nil {
		return protocol.FileWritten{}, fileUnknown()
	}
	if err := target.validateParent(); err != nil {
		return protocol.FileWritten{}, fileUnknown()
	}
	file, observed, err := currentFile(target.parent, name, max(target.limit, size), target.control)
	if err != nil {
		return protocol.FileWritten{}, fileUnknown()
	}
	defer file.Close()
	current, err := file.Stat()
	if err != nil || !os.SameFile(published, current) || observed != revision {
		return protocol.FileWritten{}, fileUnknown()
	}
	return protocol.FileWritten{Path: target.relative, Size: size, Revision: revision}, nil
}

func (target *FileTarget) validateParent() error {
	root, err := acquireFileRoot(target.files.path)
	if err != nil {
		return err
	}
	defer root.Close()
	current := Files{root: root, access: target.files.access}
	rootID, err := current.root.Stat(".")
	if err != nil {
		return fileIOFault(err)
	}
	parent, _, err := current.parent(target.relative, true)
	if err != nil {
		return err
	}
	defer parent.Close()
	parentID, err := parent.Stat(".")
	if err != nil {
		return fileIOFault(err)
	}
	if !os.SameFile(rootID, target.rootID) || !os.SameFile(parentID, target.parentID) {
		return fileConflict()
	}
	return nil
}

func currentFile(parent *os.Root, name string, limit uint64, control fileControl) (*os.File, string, error) {
	metadata, err := parent.Lstat(name)
	if err != nil {
		return nil, "", fileIOFault(err)
	}
	if !metadata.Mode().IsRegular() {
		return nil, "", fileConflict()
	}
	file, err := openRegularFile(parent, name)
	if err != nil {
		return nil, "", err
	}
	revision, err := fileContentRevision(file, limit, control)
	if err != nil {
		file.Close()
		return nil, "", err
	}
	return file, revision, nil
}

func fileContentRevision(file *os.File, limit uint64, control fileControl) (string, error) {
	before, err := file.Stat()
	if err != nil {
		return "", fileIOFault(err)
	}
	if uint64(before.Size()) > limit {
		return "", fileConflict()
	}
	hash := blake3.New(32, nil)
	var total int64
	var buffer [protocol.FILE_TRANSFER_CHUNK_BYTES]byte
	for {
		if err := control.check(); err != nil {
			return "", err
		}
		count, err := file.Read(buffer[:])
		total += int64(count)
		if total > before.Size() {
			return "", fileConflict()
		}
		hash.Write(buffer[:count])
		if err != nil {
			if errors.Is(err, io.EOF) {
				break
			}
			return "", fileIOFault(err)
		}
	}
	after, err := file.Stat()
	if err != nil {
		return "", fileIOFault(err)
	}
	if !fileMetadataUnchanged(before, after, total) {
		return "", fileConflict()
	}
	return fmt.Sprintf("%x", hash.Sum(nil)), nil
}

func copyFileContent(source io.Reader, target io.Writer, size uint64, revision string, control fileControl) error {
	hash := blake3.New(32, nil)
	var total uint64
	var buffer [protocol.FILE_TRANSFER_CHUNK_BYTES]byte
	for {
		if err := control.check(); err != nil {
			return err
		}
		count, err := source.Read(buffer[:])
		if uint64(count) > size-total {
			return fileConflict()
		}
		total += uint64(count)
		if count != 0 {
			written, writeErr := target.Write(buffer[:count])
			if writeErr != nil {
				return fileIOFault(writeErr)
			}
			if written != count {
				return fileIOFault(io.ErrShortWrite)
			}
			hash.Write(buffer[:count])
		}
		if err != nil {
			if errors.Is(err, io.EOF) {
				break
			}
			return fileIOFault(err)
		}
	}
	if total != size || fmt.Sprintf("%x", hash.Sum(nil)) != revision {
		return fileConflict()
	}
	return nil
}

func contentRevision(data []byte) string { return fmt.Sprintf("%x", blake3.Sum256(data)) }

func validFileRevision(value string) bool {
	decoded, err := hex.DecodeString(value)
	return err == nil && len(decoded) == 32
}

func baseFileName(relative string) string {
	return relative[strings.LastIndexByte(relative, '/')+1:]
}
