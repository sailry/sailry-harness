package live

import (
	"context"
	"io"
	"path/filepath"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (state *pluginNativeState) chooseSecretFile(ctx context.Context, _ string) ([]byte, string, error) {
	if state.runtime.attachments == nil || state.runtime.attachments.native.PickFiles == nil {
		return nil, "", pluginNativeFault(protocol.ErrorCodeUnavailable, "credential file selection is unavailable")
	}
	type selection struct {
		path string
		err  error
	}
	selected := make(chan selection, 1)
	if !state.post(func(*ui.Context) {
		state.runtime.attachments.native.PickFiles(func(paths []string) {
			value := selection{}
			switch len(paths) {
			case 0:
			case 1:
				value.path = paths[0]
			default:
				value.err = pluginNativeFault(protocol.ErrorCodeInvalidRequest, "select one credential file")
			}
			select {
			case selected <- value:
			default:
			}
		})
	}) {
		return nil, "", pluginNativeFault(protocol.ErrorCodeUnavailable, "credential file selection is unavailable")
	}
	var value selection
	select {
	case <-ctx.Done():
		return nil, "", ctx.Err()
	case <-state.native.Context.Done():
		return nil, "", state.native.Context.Err()
	case value = <-selected:
	}
	if value.err != nil || value.path == "" {
		return nil, "", value.err
	}
	readCtx, cancel := context.WithCancel(ctx)
	defer cancel()
	stop := context.AfterFunc(state.native.Context, cancel)
	defer stop()
	if state.native.Context.Err() != nil {
		return nil, "", state.native.Context.Err()
	}
	return readPickedSecretFile(readCtx, value.path)
}

func readPickedSecretFile(ctx context.Context, path string) ([]byte, string, error) {
	const limit = 64 << 10
	if err := ctx.Err(); err != nil {
		return nil, "", err
	}
	if !filepath.IsAbs(path) {
		return nil, "", pluginNativeFault(protocol.ErrorCodeInvalidRequest, "an absolute credential file is required")
	}
	file, err := openLocalAttachment(path)
	if err != nil {
		return nil, "", pluginNativeFault(protocol.ErrorCodeUnavailable, "credential file could not be opened")
	}
	defer file.Close()
	if info, err := file.Stat(); err != nil || !info.Mode().IsRegular() {
		return nil, "", pluginNativeFault(protocol.ErrorCodeInvalidRequest, "credential source is not an ordinary file")
	}
	content, err := io.ReadAll(io.LimitReader(file, limit+1))
	if err != nil {
		clear(content)
		return nil, "", pluginNativeFault(protocol.ErrorCodeUnavailable, "credential file could not be read")
	}
	if err := ctx.Err(); err != nil {
		clear(content)
		return nil, "", err
	}
	if len(content) == 0 || len(content) > limit || !utf8.Valid(content) {
		clear(content)
		return nil, "", pluginNativeFault(protocol.ErrorCodeInvalidRequest, "credential file must contain between 1 and 65536 UTF-8 bytes")
	}
	return content, filepath.Base(path), nil
}
