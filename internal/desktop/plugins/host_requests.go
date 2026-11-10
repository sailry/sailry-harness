package plugins

import (
	"context"
	"encoding/json"
	"errors"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func (owner *Host) prepare(ctx context.Context, command protocol.Command) (json.RawMessage, error) {
	if err := owner.check(ctx); err != nil {
		return nil, err
	}
	body, err := json.Marshal(command)
	if err != nil {
		return nil, err
	}
	if len(body) > 2*1024*1024 {
		return nil, hostInvalid("plugin command exceeds the size limit")
	}
	if owner.binding.Client == nil {
		return nil, hostUnavailable("plugin Client is unavailable")
	}
	request := owner.binding.Client.Prepare(command)
	scope := owner.scope()
	request.Plugin = &scope
	owner.mu.Lock()
	defer owner.mu.Unlock()
	if err := owner.settings.prepare(request, owner.binding.Client.Connected()); err != nil {
		return nil, err
	}
	owner.requests[request.ID.String()] = request
	return hostJSON(request.ID.String())
}

func (owner *Host) prepareData(ctx context.Context, kind string, data any) (json.RawMessage, error) {
	command, err := commandFrom(kind, data)
	if err != nil {
		return nil, err
	}
	return owner.prepare(ctx, command)
}

func (owner *Host) read(ctx context.Context, command protocol.Command) (json.RawMessage, error) {
	if err := owner.check(ctx); err != nil {
		return nil, err
	}
	if owner.binding.Client == nil {
		return nil, hostUnavailable("plugin Client is unavailable")
	}
	owner.mu.Lock()
	pending := owner.settings.pending != nil
	owner.mu.Unlock()
	if pending {
		return nil, hostInvalid("settings save is pending")
	}
	request := owner.binding.Client.Prepare(command)
	scope := owner.scope()
	request.Plugin = &scope
	output, err := owner.binding.Client.Execute(ctx, request)
	if err != nil {
		failure := hostFault(err)
		encoded, _ := json.Marshal(failure)
		return nil, errors.New(string(encoded))
	}
	value, err := publicHostOutput(output)
	if err != nil {
		return nil, err
	}
	return hostJSON(hostAt(value, "data"))
}

func (owner *Host) readData(ctx context.Context, kind string, data any) (json.RawMessage, error) {
	command, err := commandFrom(kind, data)
	if err != nil {
		return nil, err
	}
	return owner.read(ctx, command)
}

func (owner *Host) prepared(ctx context.Context, identity string) (protocol.Request, error) {
	if err := owner.check(ctx); err != nil {
		return protocol.Request{}, err
	}
	owner.mu.Lock()
	defer owner.mu.Unlock()
	request, exists := owner.requests[identity]
	if !exists {
		return protocol.Request{}, hostInvalid("plugin request draft is unavailable")
	}
	return request, nil
}

func (owner *Host) complete(ctx context.Context, identity string, confirm bool) (json.RawMessage, error) {
	request, err := owner.prepared(ctx, identity)
	if err != nil {
		return nil, err
	}
	output, failure := owner.binding.Client.Execute(ctx, request)
	if failure != nil && confirm && request.Command.Durable() && hostRequestUncertain(failure) {
		for {
			outcome, err := owner.binding.Client.Outcome(ctx, request)
			if err != nil {
				return nil, errors.New("unconfirmed")
			}
			switch outcome.Kind {
			case "completed":
				var result protocol.Result[protocol.Output, protocol.Fault]
				body, err := json.Marshal(outcome.Data)
				if err != nil || json.Unmarshal(body, &result) != nil {
					return nil, errors.New("unconfirmed")
				}
				if result.Ok != nil {
					output, failure = *result.Ok, nil
				} else {
					failure = result.Err
				}
			case "admitted":
				timer := owner.binding.Clock.NewTimer(250 * time.Millisecond)
				select {
				case <-ctx.Done():
					timer.Stop()
					return nil, ctx.Err()
				case <-owner.ctx.Done():
					timer.Stop()
					return nil, owner.ctx.Err()
				case <-timer.Chan():
					continue
				}
			default:
				return nil, errors.New("unconfirmed")
			}
			break
		}
	}
	owner.mu.Lock()
	settingsErr := owner.settings.finish(request.ID, output, failure)
	owner.mu.Unlock()
	if settingsErr != nil {
		owner.cancel()
		return nil, settingsErr
	}
	if failure != nil {
		return hostJSON(map[string]any{"Err": hostFault(failure)})
	}
	value, err := publicHostOutput(output)
	if err != nil {
		return nil, err
	}
	return hostJSON(map[string]any{"Ok": value})
}

func hostRequestUncertain(err error) bool {
	switch hostFault(err).Code {
	case protocol.ErrorCodeOutcomeUnknown, protocol.ErrorCodeUnavailable, protocol.ErrorCodeBusy, protocol.ErrorCodeInternal, protocol.ErrorCodeCancelled:
		return true
	default:
		return false
	}
}

func (owner *Host) outcome(ctx context.Context, identity string) (json.RawMessage, error) {
	request, err := owner.prepared(ctx, identity)
	if err != nil {
		return nil, err
	}
	outcome, err := owner.binding.Client.Outcome(ctx, request)
	if err != nil {
		return nil, err
	}
	if outcome.Kind == "completed" {
		var result protocol.Result[protocol.Output, protocol.Fault]
		body, err := json.Marshal(outcome.Data)
		if err != nil || json.Unmarshal(body, &result) != nil {
			return nil, hostInvalid("invalid plugin request outcome")
		}
		owner.mu.Lock()
		var output protocol.Output
		var failure error
		if result.Ok != nil {
			output = *result.Ok
		} else if result.Err != nil {
			failure = result.Err
		}
		settingsErr := owner.settings.finish(request.ID, output, failure)
		owner.mu.Unlock()
		if settingsErr != nil {
			owner.cancel()
			return nil, settingsErr
		}
		if result.Err != nil {
			return hostJSON(map[string]any{"kind": "completed", "data": map[string]any{"Err": result.Err}})
		}
		value, err := publicHostOutput(*result.Ok)
		if err != nil {
			return nil, err
		}
		return hostJSON(map[string]any{"kind": "completed", "data": map[string]any{"Ok": value}})
	}
	return hostJSON(outcome)
}

func (owner *Host) forget(ctx context.Context, identity string) (json.RawMessage, error) {
	if err := owner.check(ctx); err != nil {
		return nil, err
	}
	owner.mu.Lock()
	defer owner.mu.Unlock()
	if owner.settings.pending != nil && owner.settings.pending.identity.String() == identity {
		return nil, hostInvalid("unconfirmed settings save cannot be forgotten")
	}
	delete(owner.requests, identity)
	return json.RawMessage("null"), nil
}
