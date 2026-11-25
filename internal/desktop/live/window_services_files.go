package live

import (
	"context"
	"errors"
	"io"
	"path/filepath"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/fileview"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/sdk/desktop/view"
)

func windowPick[Value any](h *windowViewHost, ctx context.Context, resolve func() func(func(Value))) (Value, error) {
	var zero Value
	if err := ctx.Err(); err != nil {
		return zero, err
	}
	type result struct {
		value Value
		err   error
	}
	reply := make(chan result, 1)
	if !h.post(func(*ui.Context) {
		if ctx.Err() != nil {
			return
		}
		open := resolve()
		if open == nil || h.runtime.App.DemoOn() {
			reply <- result{err: fileActionFault(protocol.ErrorCodeUnavailable, "native file selection is unavailable")}
			return
		}
		// Do not run the OS dialog's nested event loop inside the frame or
		// make window close wait for the user to dismiss the dialog. The
		// platform callback is synchronous and may skip a dismissed choice.
		go func() {
			if ctx.Err() != nil || !h.open() {
				return
			}
			selected := func(value Value) {
				select {
				case reply <- result{value: value}:
				default:
				}
			}
			open(selected)
			selected(zero)
		}()
	}) {
		return zero, context.Canceled
	}
	select {
	case <-ctx.Done():
		return zero, ctx.Err()
	case <-h.runtime.ctx.Done():
		return zero, h.runtime.ctx.Err()
	case chosen := <-reply:
		if err := ctx.Err(); err != nil {
			return zero, err
		}
		if !h.open() {
			return zero, context.Canceled
		}
		return chosen.value, chosen.err
	}
}

func (h *windowViewHost) pickFiles(ctx context.Context) ([]string, error) {
	paths, err := windowPick(h, ctx, func() func(func([]string)) {
		if state := h.runtime.attachments; state != nil {
			return state.native.PickFiles
		}
		return nil
	})
	for _, path := range paths {
		if !filepath.IsAbs(path) {
			return nil, fileActionFault(protocol.ErrorCodeInvalidRequest, "native file selection is not absolute")
		}
	}
	return paths, err
}

func (h *windowViewHost) pickFolder(ctx context.Context) (string, error) {
	path, err := windowPick(h, ctx, func() func(func(string)) { return h.runtime.pickDirectory })
	return absoluteWindowSelection(path, err)
}

func (h *windowViewHost) pickDestination(ctx context.Context, name string) (string, error) {
	if name == "." || !filepath.IsLocal(name) || strings.ContainsAny(name, "/\\\x00") {
		return "", fileActionFault(protocol.ErrorCodeInvalidRequest, "invalid download file name")
	}
	path, err := windowPick(h, ctx, func() func(func(string)) {
		if state := h.runtime.attachments; state != nil && state.native.saveFilePicker != nil {
			save := state.native.saveFilePicker
			return func(reply func(string)) { save(name, reply) }
		}
		return nil
	})
	return absoluteWindowSelection(path, err)
}

func absoluteWindowSelection(path string, err error) (string, error) {
	if err == nil && path != "" && !filepath.IsAbs(path) {
		return "", fileActionFault(protocol.ErrorCodeInvalidRequest, "native file selection is not absolute")
	}
	return path, err
}

func (h *windowViewHost) pickSecret(ctx context.Context, _ string) ([]byte, string, error) {
	paths, err := h.pickFiles(ctx)
	if err != nil || len(paths) == 0 {
		return nil, "", err
	}
	if len(paths) != 1 {
		return nil, "", fileActionFault(protocol.ErrorCodeInvalidRequest, "select one credential file")
	}
	ctx, done := h.operationContext(ctx)
	defer done()
	return readPickedSecretFile(ctx, paths[0])
}

func (h *windowViewHost) writeClipboard(text string) error {
	if !h.post(func(c *ui.Context) {
		if !h.runtime.App.DemoOn() {
			c.WriteClipboard(text)
		}
	}) {
		return context.Canceled
	}
	return nil
}

func (h *windowViewHost) preview(path string, read func(context.Context, uint64, func(uint64, uint64)) ([]byte, error)) error {
	if read == nil {
		return fileActionFault(protocol.ErrorCodeInvalidRequest, "preview has no source")
	}
	if !h.post(func(*ui.Context) {
		if h.runtime.App.DemoOn() {
			return
		}
		h.runtime.openFilePreview(path, func(ctx context.Context, progress func(uint64, uint64)) ([]byte, error) {
			data, err := read(ctx, fileview.Limit, progress)
			if err == nil && len(data) > fileview.Limit {
				return nil, errPreviewLarge
			}
			return data, err
		})
	}) {
		return context.Canceled
	}
	return nil
}

func (h *windowViewHost) openSource(ctx context.Context, path string, maximum uint64) (view.SourceFile, error) {
	if !filepath.IsAbs(path) {
		return view.SourceFile{}, fileActionFault(protocol.ErrorCodeInvalidRequest, "an absolute upload source is required")
	}
	ctx, done := h.operationContext(ctx)
	defer done()
	source, err := localSource(ctx, path, nil, "", min(maximum, sshUploadMaximum))
	if err != nil {
		if ctx.Err() != nil {
			return view.SourceFile{}, ctx.Err()
		}
		var refused *attachmentSourceError
		if errors.As(err, &refused) {
			code := faultCode(err)
			if code == "" {
				code = protocol.ErrorCodeUnavailable
			}
			return view.SourceFile{}, &protocol.Fault{Code: code, Message: refused.key}
		}
		return view.SourceFile{}, err
	}
	if err := ctx.Err(); err != nil {
		_ = source.reader.Close()
		return view.SourceFile{}, err
	}
	return view.SourceFile{Reader: source.reader, Size: source.size, Revision: source.revision}, nil
}

func (h *windowViewHost) openDestination(path string) (view.Destination, error) {
	if !h.open() {
		return view.Destination{}, context.Canceled
	}
	target, err := prepareDownloadDestination(path)
	if err != nil {
		return view.Destination{}, err
	}
	return view.Destination{Writer: target.file, Close: target.close, Publish: func(ctx context.Context) error {
		ctx, done := h.operationContext(ctx)
		defer done()
		if err := ctx.Err(); err != nil {
			return err
		}
		return target.publish(ctx)
	}}, nil
}

func (h *windowViewHost) upload(ctx context.Context, stream protocol.StreamID, size uint64, revision string, source io.ReadCloser, progress func(uint64)) error {
	ctx, done := h.operationContext(ctx)
	defer done()
	return h.runtime.controller.Upload(ctx, stream, size, revision, source, progress)
}

func (h *windowViewHost) download(ctx context.Context, stream protocol.StreamID, size uint64, revision string, destination io.WriteCloser, progress func(uint64)) error {
	ctx, done := h.operationContext(ctx)
	defer done()
	return h.runtime.controller.Download(ctx, stream, size, revision, destination, progress)
}

func (h *windowViewHost) releaseStream(stream protocol.StreamID) {
	if stream != (protocol.StreamID{}) {
		h.runtime.cancelAttachmentStream(stream)
	}
}
