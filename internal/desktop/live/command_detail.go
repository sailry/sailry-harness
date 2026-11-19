package live

import (
	"context"
	"encoding/json"
	"errors"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type commandDetailKey struct {
	ref  client.HistoricalCommandReference
	call string
}

type commandDetailRead struct {
	cancel context.CancelFunc
	seen   bool
	value  conversation.BackgroundDetail
}

type retainedCommand struct {
	session protocol.SessionID
	turn    protocol.TurnID
	id      protocol.RequestID
	preview *string
}

type retainedCommandTail struct {
	Text   string `json:"tail"`
	Offset uint64 `json:"tail_offset"`
	Head   uint64 `json:"retained_head_bytes"`
	Start  uint64 `json:"retained_tail_offset"`
	End    uint64 `json:"retained_end"`
	Total  uint64 `json:"total_bytes"`
}

func (tail retainedCommandTail) display(capture protocol.ProcessCapture) (string, bool) {
	if !capture.Truncated {
		return capture.Text, true
	}
	if tail.End != tail.Total || tail.End == 0 || tail.Start > tail.Head && tail.Offset < tail.Start ||
		strings.Count(strings.TrimSuffix(tail.Text, "\n"), "\n") < 100 {
		return "", false
	}
	return tail.Text, true
}

func toolCommand(value protocol.ToolDetail, session protocol.SessionID, turn protocol.TurnID) (retainedCommand, bool) {
	var result struct {
		Kind string `json:"kind"`
		Data struct {
			Info struct {
				ID      protocol.RequestID `json:"id"`
				Session protocol.SessionID `json:"session"`
				Turn    protocol.TurnID    `json:"turn"`
			} `json:"info"`
			Stdout protocol.ProcessCapture `json:"stdout"`
			Stderr protocol.ProcessCapture `json:"stderr"`
		} `json:"data"`
		Retained struct {
			ID     protocol.RequestID  `json:"id"`
			Stdout retainedCommandTail `json:"stdout"`
			Stderr retainedCommandTail `json:"stderr"`
		} `json:"sailry_command"`
	}
	if value.Result == nil || json.Unmarshal(*value.Result, &result) != nil || !result.Data.Stdout.Truncated && !result.Data.Stderr.Truncated {
		return retainedCommand{}, false
	}
	id := result.Retained.ID
	switch result.Kind {
	case "command_result":
	case "command_output":
		// A read or stop result can refer to a command from an earlier turn.
		info := result.Data.Info
		if info.Session != session || info.Turn == (protocol.TurnID{}) || info.ID == (protocol.RequestID{}) || id != (protocol.RequestID{}) && id != info.ID {
			return retainedCommand{}, false
		}
		id, turn = info.ID, info.Turn
	default:
		return retainedCommand{}, false
	}
	ref := retainedCommand{session: session, turn: turn, id: id}
	stdout, completeStdout := result.Retained.Stdout.display(result.Data.Stdout)
	stderr, completeStderr := result.Retained.Stderr.display(result.Data.Stderr)
	if completeStderr && strings.Count(strings.TrimSuffix(stderr, "\n"), "\n") >= 100 {
		ref.preview = &stderr
	} else if completeStdout && completeStderr {
		ref.preview = new(joinCommandText(stdout, stderr))
	}
	return ref, id != (protocol.RequestID{})
}

func (r *Runtime) loadRetainedCommand(ctx context.Context, ref retainedCommand, output *conversation.Output) {
	if ref.preview != nil {
		output.Text = *ref.preview
	} else if text, err := r.controller.ReadCommandTranscript(ctx, ref.session, ref.turn, ref.id, 100); err == nil {
		output.Text = text
	}
	// The copy action retains only the binding, even when the original
	// captured result supplied a sizeable preview.
	session, turn, id := ref.session, ref.turn, ref.id
	failure := output.Failure
	output.Copy = func(copyContext context.Context) {
		ctx, cancel := context.WithCancel(copyContext)
		stop := context.AfterFunc(r.ctx, cancel)
		if !r.async(func() {
			defer stop()
			text, err := r.controller.ReadCommandTranscript(ctx, session, turn, id, 0)
			if ctx.Err() != nil {
				cancel()
				return
			}
			r.enqueue(func(c *ui.Context) {
				defer cancel()
				if ctx.Err() != nil || r.ctx.Err() != nil {
					return
				}
				if err != nil {
					reason := "unavailable"
					if faultCode(err) == protocol.ErrorCodePermissionDenied {
						reason = "permission_denied"
					} else if faultMessage(err) == "resource host is offline" {
						reason = "resource_offline_action"
					}
					kit.Error(c, i18n.T(c, "tool_copy"), i18n.T(c, reason))
					return
				}
				c.WriteClipboard(joinCommandText(failure, text))
			})
		}) {
			stop()
			cancel()
		}
	}
}

func joinCommandText(stdout, stderr string) string {
	if stdout != "" && stderr != "" && !strings.HasSuffix(stdout, "\n") {
		stdout += "\n"
	}
	return stdout + stderr
}

func (r *Runtime) historicalCommandDetail(ref client.HistoricalCommandReference) conversation.BackgroundDetail {
	if r.commands == nil {
		return conversation.BackgroundDetail{}
	}
	key := commandDetailKey{ref: ref}
	if ref.Detail.Call != nil {
		key.call = *ref.Detail.Call
		key.ref.Detail.Call = nil
	}
	read := r.commands.details[key]
	if read == nil {
		ctx, cancel := context.WithCancel(r.ctx)
		read = &commandDetailRead{cancel: cancel}
		if r.commands.details == nil {
			r.commands.details = map[commandDetailKey]*commandDetailRead{}
		}
		r.commands.details[key] = read
		if !r.async(func() {
			value, err := r.controller.ReadHistoricalCommand(ctx, ref)
			detail := projectHistoricalCommand(value, err)
			if detail.Output != nil && (value.Stdout.Truncated || value.Stderr.Truncated) {
				r.loadRetainedCommand(ctx, retainedCommand{session: ref.Session, turn: ref.Detail.Turn, id: ref.Command}, detail.Output)
			}
			r.enqueue(func(*ui.Context) {
				if ctx.Err() == nil {
					read.value = detail
				}
			})
		}) {
			cancel()
			delete(r.commands.details, key)
			return conversation.BackgroundDetail{}
		}
	}
	read.seen = true
	return read.value
}

func projectHistoricalCommand(value client.HistoricalCommandOutput, err error) conversation.BackgroundDetail {
	if err != nil {
		if errors.Is(err, context.Canceled) || faultCode(err) == protocol.ErrorCodeCancelled {
			return conversation.BackgroundDetail{}
		}
		return conversation.BackgroundDetail{Failure: faultDetail(err)}
	}
	text := joinCommandText(value.Stdout.Text, value.Stderr.Text)
	var detail conversation.BackgroundDetail
	if text != "" || value.Failure != "" || value.Stdout.Truncated || value.Stderr.Truncated {
		detail.Output = &conversation.Output{Command: true, Text: text, Failure: value.Failure}
	}
	return detail
}

func (r *Runtime) pruneCommandDetails() {
	for key, read := range r.commands.details {
		if !read.seen || !r.conversationSeen[key.ref.Detail.Session] {
			read.cancel()
			delete(r.commands.details, key)
		}
		read.seen = false
	}
}
