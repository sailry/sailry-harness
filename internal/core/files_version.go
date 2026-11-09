package core

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func (files *Files) SourceRevision(relative string) (string, error) {
	parent, name, err := files.parent(relative, false)
	if err != nil {
		return "", err
	}
	defer parent.Close()
	file, err := openRegularFile(parent, name)
	if err != nil {
		return "", err
	}
	defer file.Close()
	return retainedFileRevision(files.root, parent, file, relative)
}

func retainedFileRevision(root, parent *os.Root, file *os.File, relative string) (string, error) {
	metadata, err := file.Stat()
	if err != nil {
		return "", fileIOFault(err)
	}
	rootFile, err := root.Open(".")
	if err != nil {
		return "", fileIOFault(err)
	}
	defer rootFile.Close()
	parentFile, err := parent.Open(".")
	if err != nil {
		return "", fileIOFault(err)
	}
	defer parentFile.Close()
	rootID, err := fileNativeIdentity(rootFile)
	if err != nil {
		return "", err
	}
	parentID, err := fileNativeIdentity(parentFile)
	if err != nil {
		return "", err
	}
	fileID, err := fileNativeIdentity(file)
	if err != nil {
		return "", err
	}
	created, platform, err := fileNativeVersion(file, metadata)
	if err != nil {
		return "", err
	}
	data, err := json.Marshal([]any{"file-source-v1", relative, rootID, parentID, fileID, uint64(metadata.Size()), fileVersionTime(metadata.ModTime()), created, platform})
	if err != nil {
		return "", fileFault(protocol.ErrorCodeInternal, "file source revision could not be encoded")
	}
	return fmt.Sprintf("%x", blake3.Sum256(data)), nil
}

func (files *Files) VerifySource(ctx context.Context, relative, revision, stamp string) error {
	parent, name, err := files.parent(relative, true)
	if err != nil {
		return err
	}
	defer parent.Close()
	file, err := openRegularFile(parent, name)
	if err != nil {
		return err
	}
	defer file.Close()
	retained, err := retainedFileRevision(files.root, parent, file, relative)
	if err != nil {
		return err
	}
	if retained != stamp {
		return fileSourceChanged()
	}
	metadata, err := file.Stat()
	if err != nil {
		return fileIOFault(err)
	}
	observed, err := fileContentRevision(file, uint64(metadata.Size()), fileControl{context: ctx})
	if err != nil {
		return err
	}
	retained, err = retainedFileRevision(files.root, parent, file, relative)
	if err != nil {
		return err
	}
	current, err := files.SourceRevision(relative)
	if err != nil {
		return err
	}
	if observed != revision || retained != stamp || current != stamp {
		return fileSourceChanged()
	}
	return nil
}

func fileVersionTime(value time.Time) any {
	if value.IsZero() {
		return nil
	}
	seconds, nanos := value.Unix(), value.Nanosecond()
	if seconds < 0 {
		if nanos == 0 {
			return []any{true, uint64(-(seconds + 1)) + 1, uint32(0)}
		}
		return []any{true, uint64(-(seconds + 1)), uint32(1_000_000_000 - nanos)}
	}
	return []any{false, uint64(seconds), uint32(nanos)}
}

func fileSourceChanged() error {
	return fileFault(protocol.ErrorCodeRevisionConflict, "file source changed since download")
}
