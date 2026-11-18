package core

import (
	"context"
	"os"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// FileStamp identifies a particular regular file and version of its data.
// A zero stamp is never evidence that two reads have the same contents.
type FileStamp struct {
	device, inode           uint64
	created, changed, mtime int64
	size                    int64
	generation              uint32
}

// InspectText skips inspect only if previous proves the contents unchanged.
// Otherwise inspect receives the bounded text (nil for an oversized file).
// Its result is usable only after the named file has been checked again.
func (files *Files) InspectText(ctx context.Context, relative string, limit int, previous FileStamp, inspect func(*string) error) (FileStamp, error) {
	if limit <= 0 || limit > protocol.MAX_EDIT_BYTES {
		return FileStamp{}, fileFault(protocol.ErrorCodeInvalidRequest, "text inspection limit is invalid")
	}
	control := newFileControl(ctx)
	if err := control.check(); err != nil {
		return FileStamp{}, err
	}
	file, err := files.Open(relative)
	if err != nil {
		return FileStamp{}, err
	}
	defer file.Close()
	before, err := file.Stat()
	if err != nil {
		return FileStamp{}, fileIOFault(err)
	}
	stamp := fileStamp(file, before)
	if stamp != (FileStamp{}) && stamp == previous {
		return stamp, control.check()
	}
	source, inspectErr := boundedFileText(file, limit, control)
	if inspectErr == nil {
		inspectErr = inspect(source)
	}
	if ctx.Err() != nil {
		return FileStamp{}, ctx.Err()
	}
	// Re-resolve the path below the retained root: fstat on the first handle
	// alone cannot see a rename over the name while the consumer was parsing.
	current, err := files.Open(relative)
	if err != nil {
		return FileStamp{}, fileConflict()
	}
	defer current.Close()
	after, err := current.Stat()
	if err != nil {
		return FileStamp{}, fileIOFault(err)
	}
	if !os.SameFile(before, after) || !fileMetadataUnchanged(before, after, before.Size()) || stamp != fileStamp(current, after) {
		return FileStamp{}, fileConflict()
	}
	if inspectErr != nil {
		return FileStamp{}, inspectErr
	}
	return stamp, nil
}
