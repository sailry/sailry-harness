package mobile

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type digest struct {
	mu     sync.Mutex
	hasher *blake3.Hasher
}

func newDigest() *digest {
	return &digest{hasher: blake3.New(32, nil)}
}

func (owner *digest) invoke(ctx context.Context, _ *Runtime, _ uint64, method string, data json.RawMessage) (any, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	owner.mu.Lock()
	defer owner.mu.Unlock()
	switch method {
	case "update":
		var values struct {
			Bytes []byte `json:"bytes"`
		}
		if err := arguments(data, &values); err != nil {
			return nil, err
		}
		if len(values.Bytes) > protocol.FILE_TRANSFER_CHUNK_BYTES {
			return nil, invalid("transfer chunk exceeds 64 KiB")
		}
		_, err := owner.hasher.Write(values.Bytes)
		return nil, err
	case "revision":
		return hex.EncodeToString(owner.hasher.Sum(nil)), nil
	}
	return nil, invalid("unknown digest operation")
}

func (owner *digest) close() error { return nil }

type transfer struct {
	ctx    context.Context
	cancel context.CancelFunc
	done   chan struct{}
	err    error
	reader *io.PipeReader
	writer *io.PipeWriter
	mu     sync.Mutex
}

func (owner *connection) transfer(ctx context.Context, runtime *Runtime, handle uint64, method string, values input) (any, error) {
	var stream protocol.StreamID
	var size uint64
	var revision string
	switch method {
	case "upload_attachment":
		var descriptor protocol.AttachmentUpload
		if err := json.Unmarshal([]byte(values.Upload), &descriptor); err != nil {
			return nil, invalid("invalid upload descriptor")
		}
		stream, size, revision = descriptor.Stream, descriptor.Spec.Size, descriptor.Spec.Revision
	case "download_attachment":
		var descriptor protocol.AttachmentDownload
		if err := json.Unmarshal([]byte(values.Download), &descriptor); err != nil {
			return nil, invalid("invalid download descriptor")
		}
		stream, size, revision = descriptor.Stream, descriptor.Attachment.Spec.Size, descriptor.Attachment.Spec.Revision
	case "download_file":
		var descriptor protocol.FileDownload
		if err := json.Unmarshal([]byte(values.Download), &descriptor); err != nil {
			return nil, invalid("invalid download descriptor")
		}
		stream, size, revision = descriptor.Stream, descriptor.Size, descriptor.Revision
	}
	lifetime, cancel, detach := runtime.lifetime(ctx, handle)
	detach()
	reader, writer := io.Pipe()
	shared := &transfer{ctx: lifetime, cancel: cancel, done: make(chan struct{}), reader: reader, writer: writer}
	if method == "upload_attachment" {
		go func() {
			shared.err = owner.client.Upload(lifetime, stream, size, revision, reader, nil)
			_ = reader.CloseWithError(shared.err)
			close(shared.done)
		}()
		return runtime.add(handle, "upload", &upload{transfer: shared}, lifetime, cancel)
	}
	go func() {
		shared.err = owner.client.Download(lifetime, stream, size, revision, writer, nil)
		_ = writer.CloseWithError(shared.err)
		close(shared.done)
	}()
	return runtime.add(handle, "download", &download{transfer: shared}, lifetime, cancel)
}

func (owner *transfer) close() error {
	owner.cancel()
	_ = owner.reader.CloseWithError(context.Canceled)
	_ = owner.writer.CloseWithError(context.Canceled)
	<-owner.done
	return nil
}

func (owner *transfer) wait(ctx context.Context) error {
	select {
	case <-owner.done:
		return owner.err
	case <-ctx.Done():
		return ctx.Err()
	}
}

type upload struct{ *transfer }

func (owner *upload) invoke(ctx context.Context, _ *Runtime, _ uint64, method string, data json.RawMessage) (any, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	switch method {
	case "write":
		var values struct {
			Bytes []byte `json:"bytes"`
		}
		if err := arguments(data, &values); err != nil {
			return nil, err
		}
		if len(values.Bytes) > protocol.FILE_TRANSFER_CHUNK_BYTES {
			return nil, invalid("transfer chunk exceeds 64 KiB")
		}
		stop := context.AfterFunc(ctx, func() { _ = owner.writer.CloseWithError(ctx.Err()) })
		defer stop()
		owner.mu.Lock()
		_, err := owner.writer.Write(values.Bytes)
		owner.mu.Unlock()
		return nil, err
	case "finish":
		owner.mu.Lock()
		err := owner.writer.Close()
		owner.mu.Unlock()
		if err != nil {
			return nil, err
		}
		return nil, owner.wait(ctx)
	}
	return nil, invalid("unknown upload operation")
}

type download struct{ *transfer }

func (owner *download) invoke(ctx context.Context, _ *Runtime, _ uint64, method string, _ json.RawMessage) (any, error) {
	if method != "next" {
		return nil, invalid("unknown download operation")
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	stop := context.AfterFunc(ctx, func() { _ = owner.reader.CloseWithError(ctx.Err()) })
	defer stop()
	owner.mu.Lock()
	defer owner.mu.Unlock()
	data := make([]byte, protocol.FILE_TRANSFER_CHUNK_BYTES)
	count, err := owner.reader.Read(data)
	if err != nil && !errors.Is(err, io.EOF) {
		return nil, err
	}
	if count == 0 {
		return nil, owner.wait(ctx)
	}
	// A byte slice crosses the boundary as one base64 string in both directions.
	return data[:count], nil
}
