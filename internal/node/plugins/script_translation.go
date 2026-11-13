package plugins

import (
	"context"
	"encoding/json"
	"sync"
	"time"

	"github.com/MoeForge/moejs"
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
	session := &Translation{runtime: runtime, timeout: budget}
	if entry := runtime.memory; entry != nil {
		entry.tracker.mu.Lock()
		entry.idle = session.sampleMemory
		entry.tracker.mu.Unlock()
	}
	return session, nil
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
	session.runtime.memory.executing(true)
	defer session.runtime.memoryIdle()
	session.runtime.deadline = scriptTranslationDeadline(session.runtime.ctx, session.timeout)
	stop, err := session.runtime.begin()
	if err != nil {
		return false, err
	}
	defer stop()
	defer session.runtime.vm.ReleaseCallData()
	value, err := session.runtime.vm.CallFunction(session.runtime.has, moejs.Undefined(), session.runtime.exports, moejs.String(handler))
	if err != nil {
		return false, session.runtime.engine(err)
	}
	if err := session.runtime.arm(); err != nil {
		return false, err
	}
	encoded, err := session.runtime.vm.AppendJSON(nil, value)
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

// sampleMemory takes over an idle runtime only while its call mutex is free.
// Diagnostics never wait for a model stream or run JavaScript themselves.
func (session *Translation) sampleMemory() {
	if !session.mu.TryLock() {
		return
	}
	defer session.mu.Unlock()
	if session.runtime != nil {
		session.runtime.measureMemory()
	}
}
