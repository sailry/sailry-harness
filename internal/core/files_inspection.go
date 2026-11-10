package core

import (
	"bytes"
	"context"
	"github.com/sailry/sailry-harness/internal/protocol"
	"unicode/utf8"
)

func (files *Files) ReadContent(ctx context.Context, relative string) (protocol.FileContent, error) {
	out := protocol.DefaultFileContent()
	out.Path = relative
	f, err := files.Open(relative)
	if err != nil {
		return out, err
	}
	defer f.Close()
	before, err := f.Stat()
	if err != nil {
		return out, fileIOFault(err)
	}
	data, err := readFileBytes(f, protocol.MAX_FILE_BYTES+1, newFileControl(ctx))
	if err != nil {
		return out, err
	}
	out.Size = uint64(before.Size())
	out.Truncated = len(data) > protocol.MAX_FILE_BYTES
	if out.Truncated {
		data = data[:protocol.MAX_FILE_BYTES]
		if !utf8.Valid(data) {
			for trim := 1; trim <= 3 && trim <= len(data); trim++ {
				if utf8.Valid(data[:len(data)-trim]) && !utf8.FullRune(data[len(data)-trim:]) {
					data = data[:len(data)-trim]
					break
				}
			}
		}
	}
	if !utf8.Valid(data) {
		return out, fileFault(protocol.ErrorCodeInvalidRequest, "file is not UTF-8 text")
	}
	after, err := f.Stat()
	if err != nil {
		return out, fileIOFault(err)
	}
	if !fileMetadataUnchanged(before, after, before.Size()) {
		return out, fileConflict()
	}
	if bytes.IndexByte(data, 0) >= 0 {
		return out, fileFault(protocol.ErrorCodeInvalidRequest, "binary file cannot be displayed as text")
	}
	out.Text = string(data)
	if !out.Truncated {
		revision := contentRevision(data)
		out.Revision = &revision
	}
	return out, nil
}

func (files *Files) DeleteFile(ctx context.Context, relative, revision string) error {
	stamp, err := files.SourceRevision(relative)
	if err != nil {
		return err
	}
	return files.RemoveVerified(ctx, relative, revision, stamp)
}
func (files *Files) MoveFile(ctx context.Context, from, to, revision string) error {
	stamp, err := files.SourceRevision(from)
	if err != nil {
		return err
	}
	if err = files.VerifySource(ctx, from, revision, stamp); err != nil {
		return err
	}
	return files.MoveEntryTo(ctx, files, from, to)
}
func (files *Files) ValidatePath(relative string, mutation bool) error {
	_, err := fileComponents(relative, false, files.access, mutation)
	return err
}
