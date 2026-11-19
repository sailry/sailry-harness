package client

import (
	"context"
	"encoding/json"
	"strings"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type commandOutputKey struct {
	session protocol.SessionID
	command protocol.RequestID
}
type CommandOutputState struct {
	Snapshot protocol.ProcessSnapshot
	Known    bool
	Err      error
}

type HistoricalCommandReference struct {
	Detail  protocol.ToolDetailReference `json:"detail"`
	Command protocol.RequestID           `json:"command"`
	Session protocol.SessionID           `json:"session"`
}

type HistoricalCommandOutput struct {
	Running  bool                                   `json:"running"`
	Outcome  protocol.ConversationBackgroundOutcome `json:"outcome,omitempty"`
	ExitCode *int32                                 `json:"exit_code,omitempty"`
	Stdout   protocol.ProcessCapture                `json:"stdout"`
	Stderr   protocol.ProcessCapture                `json:"stderr"`
	Failure  string                                 `json:"failure,omitempty"`
}

func (c *Client) ReadHistoricalCommand(ctx context.Context, ref HistoricalCommandReference) (HistoricalCommandOutput, error) {
	if err := ctx.Err(); err != nil {
		return HistoricalCommandOutput{}, err
	}
	detail, ok := c.ConversationDetail(ref.Detail)
	if !ok {
		var err error
		detail, err = c.ReadToolDetail(ctx, ref.Detail)
		if err != nil {
			return HistoricalCommandOutput{}, err
		}
	}
	var started struct {
		Kind string `json:"kind"`
		Data struct {
			ID      protocol.RequestID `json:"id"`
			Session protocol.SessionID `json:"session"`
			Turn    *protocol.TurnID   `json:"turn"`
		} `json:"data"`
	}
	// The activity ID and visible conversation are not a process binding. An
	// inherited or legacy row must still carry the original pair in its result.
	if detail.Result == nil || json.Unmarshal(*detail.Result, &started) != nil || started.Kind != "command_started" ||
		started.Data.ID == (protocol.RequestID{}) || started.Data.ID != ref.Command ||
		started.Data.Session == (protocol.SessionID{}) || started.Data.Session != ref.Session ||
		started.Data.Turn != nil && *started.Data.Turn != ref.Detail.Turn {
		return HistoricalCommandOutput{}, fault(protocol.ErrorCodeOutcomeUnknown, "the tool result has no matching command binding")
	}
	if err := ctx.Err(); err != nil {
		return HistoricalCommandOutput{}, err
	}
	snapshot, err := c.readCommandOutput(ctx, started.Data.Session, started.Data.ID, 0)
	if err != nil {
		return HistoricalCommandOutput{}, err
	}
	if snapshot.Info.Turn != (protocol.TurnID{}) && snapshot.Info.Turn != ref.Detail.Turn {
		return HistoricalCommandOutput{}, fault(protocol.ErrorCodeInvalidRequest, "command output belongs to another turn")
	}
	return historicalCommandOutcome(snapshot), nil
}

func historicalCommandOutcome(snapshot protocol.ProcessSnapshot) HistoricalCommandOutput {
	result := HistoricalCommandOutput{Stdout: snapshot.Stdout, Stderr: snapshot.Stderr}
	switch status := snapshot.Info.Status; status.Kind {
	case "running", "stopping":
		result.Running = true
		return result
	case "failed":
		result.Outcome = protocol.ConversationBackgroundOutcomeFailed
		result.Failure, _ = payload[string](status.Data)
		return result
	case "finished":
		if outcome, err := payload[protocol.ProcessOutcome](status.Data); err == nil {
			switch outcome.Kind {
			case "exited":
				if code, err := payload[int32](outcome.Data); err == nil {
					result.ExitCode = &code
					result.Outcome = protocol.ConversationBackgroundOutcomeCompleted
					if code != 0 {
						result.Outcome = protocol.ConversationBackgroundOutcomeFailed
					}
					return result
				}
			case "signal":
				result.Outcome = protocol.ConversationBackgroundOutcomeFailed
				return result
			case "timed_out":
				result.Outcome = protocol.ConversationBackgroundOutcomeTimedOut
				return result
			case "cancelled":
				result.Outcome = protocol.ConversationBackgroundOutcomeCancelled
				return result
			}
		}
	}
	result.Outcome = protocol.ConversationBackgroundOutcomeUnknown
	return result
}

func (c *Client) readCommandOutput(ctx context.Context, session protocol.SessionID, id protocol.RequestID, wait uint64) (protocol.ProcessSnapshot, error) {
	output, err := c.Do(ctx, protocol.Command{Kind: "read_command", Data: protocol.CommandReadCommandPayload{Session: session, ID: id, WaitMs: wait}})
	if err != nil {
		return protocol.ProcessSnapshot{}, err
	}
	snapshot, err := payload[protocol.ProcessSnapshot](output.Data)
	if err != nil || output.Kind != "command_output" || snapshot.Info.ID != id || snapshot.Info.Session != session {
		return protocol.ProcessSnapshot{}, fault(protocol.ErrorCodeInvalidRequest, "unexpected command output")
	}
	return snapshot, nil
}

// ReadCommandTranscript reads retained stdout and stderr for an identified
// command. A positive line count keeps only the end of each stream while paging.
func (c *Client) ReadCommandTranscript(ctx context.Context, session protocol.SessionID, turn protocol.TurnID, id protocol.RequestID, lines int) (string, error) {
	var output strings.Builder
	for _, stream := range []protocol.ProcessOutputOutputStream{protocol.ProcessOutputOutputStreamStdout, protocol.ProcessOutputOutputStreamStderr} {
		var full strings.Builder
		var tail []string
		var newlines int
		var offset, total uint64
		for {
			result, err := c.Do(ctx, protocol.Command{Kind: "read_command_page", Data: protocol.CommandReadCommandPagePayload{
				Session: session, ID: id, Stream: stream, Offset: offset, Limit: 32 * 1024,
			}})
			if err != nil {
				return "", err
			}
			page, err := payload[protocol.ProcessOutputOutputPage](result.Data)
			if err != nil || result.Kind != "command_output_page" || page.Info.Session != session || page.Info.Turn != turn || page.Info.ID != id ||
				page.Stream != stream || page.Offset < offset || page.Offset > page.TotalBytes ||
				page.NextOffset != nil && (*page.NextOffset <= page.Offset || *page.NextOffset > page.TotalBytes) {
				return "", fault(protocol.ErrorCodeInvalidRequest, "unexpected command output page")
			}
			if offset == 0 {
				total = page.TotalBytes
			}
			if lines > 0 {
				tail = append(tail, page.Text)
				newlines += strings.Count(page.Text, "\n")
				// Keep the extra separator needed to distinguish an ending
				// newline from the first of the last complete lines.
				for len(tail) > 1 {
					remaining := newlines - strings.Count(tail[0], "\n")
					if remaining <= lines {
						break
					}
					tail[0], tail, newlines = "", tail[1:], remaining
				}
			} else {
				full.WriteString(page.Text)
			}
			if page.NextOffset == nil || *page.NextOffset >= total {
				break
			}
			offset = *page.NextOffset
		}
		text := full.String()
		if lines > 0 {
			text = retainCommandLines(strings.Join(tail, ""), lines)
		}
		if output.Len() != 0 && text != "" && !strings.HasSuffix(output.String(), "\n") {
			output.WriteByte('\n')
		}
		output.WriteString(text)
	}
	return output.String(), nil
}

func retainCommandLines(text string, lines int) string {
	end := len(strings.TrimSuffix(text, "\n"))
	for range lines {
		end = strings.LastIndexByte(text[:end], '\n')
		if end < 0 {
			return text
		}
	}
	return strings.Clone(text[end+1:])
}

type commandOutputWatch struct {
	ctx       context.Context
	cancel    context.CancelFunc
	key       commandOutputKey
	state     CommandOutputState
	callbacks map[uint64]func()
	next      uint64
}

func (c *Client) CommandOutput(session protocol.SessionID, id protocol.RequestID) (CommandOutputState, bool) {
	c.commandsMu.Lock()
	defer c.commandsMu.Unlock()
	w := c.commandOutputs[commandOutputKey{session, id}]
	if w == nil {
		return CommandOutputState{}, false
	}
	return copyValue(w.state), true
}
func (c *Client) ObserveCommandOutput(ctx context.Context, session protocol.SessionID, id protocol.RequestID, changed func()) (func(), error) {
	c.lifecycle.Lock()
	defer c.lifecycle.Unlock()
	if c.closed {
		return nil, fault(protocol.ErrorCodeUnavailable, "Client is closed")
	}
	c.commandsMu.Lock()
	if c.commandOutputs == nil {
		c.commandOutputs = map[commandOutputKey]*commandOutputWatch{}
	}
	key := commandOutputKey{session, id}
	w := c.commandOutputs[key]
	if w == nil || w.ctx.Err() != nil {
		parent := c.context
		if parent == nil {
			parent = ctx
		}
		owner, cancel := context.WithCancel(parent)
		w = &commandOutputWatch{ctx: owner, cancel: cancel, key: key, callbacks: map[uint64]func(){}}
		c.commandOutputs[key] = w
		c.commandOwners.Add(1)
		go c.observeCommandOutput(w)
	}
	w.next++
	token := w.next
	w.callbacks[token] = changed
	c.commandsMu.Unlock()
	var once sync.Once
	release := func() {
		once.Do(func() {
			c.commandsMu.Lock()
			delete(w.callbacks, token)
			if len(w.callbacks) == 0 {
				w.cancel()
				if c.commandOutputs[key] == w {
					delete(c.commandOutputs, key)
				}
			}
			c.commandsMu.Unlock()
		})
	}
	stop := context.AfterFunc(ctx, release)
	return func() { stop(); release() }, nil
}
func (c *Client) observeCommandOutput(w *commandOutputWatch) {
	defer c.commandOwners.Done()
	defer w.cancel()
	wait := uint64(0)
	for w.ctx.Err() == nil {
		snapshot, err := c.readCommandOutput(w.ctx, w.key.session, w.key.command, wait)
		if w.ctx.Err() != nil {
			return
		}
		ended := err != nil || snapshot.Info.Status.Kind != "running" && snapshot.Info.Status.Kind != "stopping"
		c.commandsMu.Lock()
		w.state.Err = err
		if err == nil {
			w.state.Snapshot, w.state.Known = snapshot, true
		}
		if ended {
			// Callbacks may immediately observe again; they must not join
			// the owner whose final state they are being told about.
			w.cancel()
		}
		callbacks := make([]func(), 0, len(w.callbacks))
		for _, f := range w.callbacks {
			callbacks = append(callbacks, f)
		}
		c.commandsMu.Unlock()
		for _, f := range callbacks {
			if f != nil {
				f()
			}
		}
		c.changed()
		if ended {
			return
		}
		wait = 30000
	}
}
