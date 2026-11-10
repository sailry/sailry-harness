package live

import (
	"context"
	"io"
	"os"
	"path/filepath"

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
	file, err := os.Open(value.path)
	if err != nil {
		return nil, "", pluginNativeFault(protocol.ErrorCodeUnavailable, "credential file could not be opened")
	}
	defer file.Close()
	content, err := io.ReadAll(io.LimitReader(file, 65537))
	if err != nil {
		clear(content)
		return nil, "", pluginNativeFault(protocol.ErrorCodeUnavailable, "credential file could not be read")
	}
	if err := ctx.Err(); err != nil {
		clear(content)
		return nil, "", err
	}
	return content, filepath.Base(value.path), nil
}
