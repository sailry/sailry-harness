package live

import (
	"context"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
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
		if faultCode(err) == protocol.ErrorCodePermissionDenied {
			return conversation.BackgroundDetail{Denied: true}
		}
		return conversation.BackgroundDetail{Status: &conversation.Background{Outcome: conversation.OutcomeUnknown}}
	}
	status := &conversation.Background{Running: value.Running, Outcome: backgroundOutcome(value.Outcome)}
	if value.ExitCode != nil {
		status.Exit = new(int(*value.ExitCode))
	}
	text := value.Stdout.Text
	if value.Stderr.Text != "" {
		if text != "" && !strings.HasSuffix(text, "\n") {
			text += "\n"
		}
		text += value.Stderr.Text
	}
	detail := conversation.BackgroundDetail{Status: status}
	partial := value.Stdout.Truncated || value.Stderr.Truncated
	if text != "" || value.Failure != "" || partial {
		detail.Output = &conversation.Output{Command: true, Text: text, Failure: value.Failure, Partial: partial}
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
