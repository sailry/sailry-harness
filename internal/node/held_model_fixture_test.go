package node

import (
	"context"
	"sync"

	"charm.land/fantasy"
)

type connectionHeldModel struct {
	fantasy.LanguageModel
	once    sync.Once
	started chan struct{}
	release chan struct{}
}

func (held *connectionHeldModel) Stream(ctx context.Context, _ fantasy.Call) (fantasy.StreamResponse, error) {
	held.once.Do(func() { close(held.started) })
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-held.release:
	}
	return func(yield func(fantasy.StreamPart) bool) {
		if yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Done"}) {
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
		}
	}, nil
}
