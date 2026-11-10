package mobile

import (
	"context"
	"encoding/json"
)

type speech struct{}

func (owner *speech) invoke(ctx context.Context, _ *Runtime, _ uint64, method string, _ json.RawMessage) (any, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	switch method {
	case "ready":
		return false, nil
	case "download_progress":
		return 0, nil
	case "cancel":
		return nil, nil
	case "download", "transcribe":
		return nil, unavailable("speech input is unavailable until the speech runtime is installed")
	}
	return nil, invalid("unknown speech input operation")
}

func (owner *speech) close() error { return nil }
