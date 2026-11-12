package live

import (
	"bytes"
	"context"
	"encoding/hex"
	"errors"
	"io"
	"mime"
	"path/filepath"
	"strings"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

const attachmentMaximum = 64 << 20

type localAttachmentSource struct {
	reader         io.ReadCloser
	size           uint64
	revision, mime string
}
type attachmentSourceError struct {
	key string
	err error
}

func (err *attachmentSourceError) Error() string { return err.err.Error() }
func (err *attachmentSourceError) Unwrap() error { return err.err }

func sourceAttachmentFault(key string, code protocol.ErrorCode, message string) error {
	return &attachmentSourceError{key: key, err: fileActionFault(code, message)}
}

func fileActionFault(code protocol.ErrorCode, message string) error {
	return &link.FaultError{Fault: protocol.Fault{Code: code, Message: message}}
}
func localSource(ctx context.Context, path string, data []byte, media string, maximum ...uint64) (out localAttachmentSource, err error) {
	out.mime = media
	limit := uint64(attachmentMaximum)
	if len(maximum) > 0 {
		limit = maximum[0]
	}
	defer func() {
		var sourceError *attachmentSourceError
		if err != nil && !errors.As(err, &sourceError) {
			err = &attachmentSourceError{key: "files_upload_source", err: err}
		}
	}()
	if err := ctx.Err(); err != nil {
		return out, err
	}
	if path == "" {
		if uint64(len(data)) > limit {
			return out, sourceAttachmentFault("files_upload_too_large", protocol.ErrorCodeInvalidRequest, "attachment exceeds the size limit")
		}
		out.reader = io.NopCloser(bytes.NewReader(data))
		out.size = uint64(len(data))
		sum := blake3.Sum256(data)
		out.revision = hex.EncodeToString(sum[:])
		if out.mime == "" {
			out.mime = "application/octet-stream"
		}
		return out, nil
	}
	if !filepath.IsAbs(path) {
		return out, sourceAttachmentFault("files_upload_source", protocol.ErrorCodeInvalidRequest, "an absolute attachment source is required")
	}
	file, err := openLocalAttachment(path)
	if err != nil {
		return out, err
	}
	success := false
	defer func() {
		if !success {
			file.Close()
		}
	}()
	before, err := file.Stat()
	if err != nil {
		return out, err
	}
	if !before.Mode().IsRegular() {
		return out, sourceAttachmentFault("files_upload_source", protocol.ErrorCodeInvalidRequest, "attachment source is not an ordinary file")
	}
	if uint64(before.Size()) > limit {
		return out, sourceAttachmentFault("files_upload_too_large", protocol.ErrorCodeInvalidRequest, "attachment exceeds the size limit")
	}
	hash := blake3.New(32, nil)
	buffer := make([]byte, protocol.FILE_TRANSFER_CHUNK_BYTES)
	var total int64
	for {
		if err = ctx.Err(); err != nil {
			return out, err
		}
		n, e := file.Read(buffer)
		if n > 0 {
			total += int64(n)
			if total > before.Size() {
				return out, fileActionFault(protocol.ErrorCodeRevisionConflict, "attachment source changed")
			}
			_, _ = hash.Write(buffer[:n])
		}
		if errors.Is(e, io.EOF) {
			break
		}
		if e != nil {
			return out, e
		}
	}
	after, err := file.Stat()
	if err != nil {
		return out, err
	}
	if total != before.Size() || before.Size() != after.Size() || !before.ModTime().Equal(after.ModTime()) {
		return out, fileActionFault(protocol.ErrorCodeRevisionConflict, "attachment source changed")
	}
	if _, err = file.Seek(0, io.SeekStart); err != nil {
		return out, err
	}
	out.reader, out.size, out.revision = file, uint64(total), hex.EncodeToString(hash.Sum(nil))
	if detected := mime.TypeByExtension(strings.ToLower(filepath.Ext(path))); detected != "" {
		if media, _, err := mime.ParseMediaType(detected); err == nil {
			out.mime = media
		}
	}
	if out.mime == "" {
		out.mime = "application/octet-stream"
	}
	success = true
	return out, nil
}

// retryAttachmentRequest asks the Node what became of a kept request and
// never repeats one the Node holds. A request that never arrived is sent with
// its identifier. The error satisfies client.Released when the Node can never
// tell what the request did: its owner drops it and reports that once.
func (r *Runtime) retryAttachmentRequest(ctx context.Context, request protocol.Request) (protocol.Output, error) {
	resolution, err := r.controller.Resolve(ctx, request)
	if err != nil {
		return protocol.Output{}, err
	}
	if resolution.State == client.RequestAbsent {
		return r.controller.Execute(ctx, request)
	}
	return resolution.Result()
}
