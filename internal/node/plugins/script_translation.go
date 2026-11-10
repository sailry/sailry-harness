package plugins

import (
	"context"
	"encoding/json"
	"sync"
	"time"

	"modernc.org/quickjs"
)

type Translation struct {
	mu      sync.Mutex
	runtime *scriptVM
	timeout time.Duration
	failed  bool
}

func NewTranslation(ctx context.Context, bundle ScriptBundle) (*Translation, error) {
	return NewTranslationBounded(ctx, bundle, ScriptCallTimeout)
}

func NewTranslationBounded(ctx context.Context, bundle ScriptBundle, budget time.Duration) (*Translation, error) {
	if budget <= 0 || budget > ScriptCallTimeout {
		budget = ScriptCallTimeout
	}
	deadline := scriptTranslationDeadline(ctx, budget)
	runtime, err := newScriptVM(ctx, bundle, deadline, nil, true)
	if err != nil {
		return nil, err
	}
	return &Translation{runtime: runtime, timeout: budget}, nil
}

func Translate(ctx context.Context, bundle ScriptBundle, handler string, input any, maxBytes int, budget time.Duration) (json.RawMessage, error) {
	session, err := NewTranslationBounded(ctx, bundle, budget)
	if err != nil {
		return nil, err
	}
	defer session.Close()
	return session.Call(handler, input, maxBytes)
}

func (session *Translation) HasExport(handler string) (bool, error) {
	session.mu.Lock()
	defer session.mu.Unlock()
	if session.runtime == nil || session.failed {
		return false, scriptInvalid("plugin translation failed")
	}
	session.runtime.deadline = scriptTranslationDeadline(session.runtime.ctx, session.timeout)
	if err := session.runtime.arm(); err != nil {
		return false, err
	}
	stop := context.AfterFunc(session.runtime.ctx, session.runtime.vm.Interrupt)
	defer stop()
	value, err := session.runtime.has.CallValue(quickjs.UndefinedValue, session.runtime.exports, handler)
	if err != nil {
		return false, session.runtime.engine(err)
	}
	defer value.Free()
	if err := session.runtime.arm(); err != nil {
		return false, err
	}
	encoded, err := value.MarshalJSON()
	if err != nil {
		return false, session.runtime.engine(err)
	}
	var present bool
	if string(encoded) == "null" || json.Unmarshal(encoded, &present) != nil {
		return false, scriptInvalid("plugin translation failed")
	}
	return present, nil
}

func (session *Translation) Call(handler string, input any, maxBytes int) (json.RawMessage, error) {
	session.mu.Lock()
	defer session.mu.Unlock()
	if session.runtime == nil || session.failed {
		return nil, scriptInvalid("plugin translation failed")
	}
	if err := scriptCheck(session.runtime.ctx, scriptTranslationDeadline(session.runtime.ctx, session.timeout), true); err != nil {
		return nil, err
	}
	encoded, err := json.Marshal(input)
	if err != nil {
		return nil, scriptInvalid("plugin translation failed")
	}
	if len(encoded) > maxBytes {
		return nil, scriptInvalid("plugin translation input exceeds its byte budget")
	}
	session.runtime.deadline = scriptTranslationDeadline(session.runtime.ctx, session.timeout)
	value, err := session.runtime.call(handler, json.RawMessage(encoded), maxBytes)
	session.failed = err != nil
	return value, err
}

func (session *Translation) Close() {
	session.mu.Lock()
	defer session.mu.Unlock()
	if session.runtime != nil {
		session.runtime.close()
		session.runtime = nil
	}
}

func scriptTranslationDeadline(ctx context.Context, budget time.Duration) time.Time {
	deadline := time.Now().Add(budget)
	if contextual, ok := ctx.Deadline(); ok && contextual.Before(deadline) {
		return contextual
	}
	return deadline
}
