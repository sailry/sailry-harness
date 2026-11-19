package live

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) enrichDetails(cv *viewmodel.Conversation, state client.ConversationState, locales ...string) {
	type source struct {
		ref        protocol.ToolDetailReference
		summary    *protocol.ToolDetailSummary
		historical *client.HistoricalCommandReference
	}
	page := state.Snapshot.Page
	sources := map[string]source{}
	pending := map[toolPendingKey][]toolAssociation{}
	for _, entry := range page.Entries {
		for index, part := range entry.Parts {
			if part.Kind != "tool_call" && part.Kind != "tool_result" {
				continue
			}
			var id *string
			var name string
			var summary *protocol.ToolDetailSummary
			key := toolCallKey(entry.ID, uint64(index))
			switch part.Kind {
			case "tool_call":
				if p, ok := detail[protocol.ConversationPartToolCallPayload](part.Data); ok {
					id, name, summary = p.ID, p.Name, p.Summary
					if id != nil {
						k := toolPendingKey{entry.Turn, entry.Branch, *id}
						pending[k] = append(pending[k], toolAssociation{ID: key, Name: name})
					}
				}
			case "tool_result":
				if p, ok := detail[protocol.ConversationPartToolResultPayload](part.Data); ok {
					id, name, summary = p.ID, p.Name, p.Summary
					if id != nil {
						k := toolPendingKey{entry.Turn, entry.Branch, *id}
						matches := pending[k]
						delete(pending, k)
						if len(matches) == 1 && matches[0].Name == name {
							key = matches[0].ID
						}
					}
				}
			}
			if summary == nil {
				continue
			}
			ref := protocol.ToolDetailReference{Session: page.Session, Turn: entry.Turn, Entry: entry.ID, Index: uint64(index), Call: id, Revision: summary.Revision}
			var historical *client.HistoricalCommandReference
			if part.Kind == "tool_result" {
				historical = client.HistoricalCommandDetail(page, ref, name, summary)
			}
			if !summary.HasDetails && historical == nil {
				delete(sources, key)
				continue
			}
			sources[key] = source{ref, summary, historical}
		}
	}
	if len(sources) == 0 {
		return
	}
	for ti := range cv.Turns {
		turn := &cv.Turns[ti]
		for bi := range turn.Reply.Blocks {
			for ci := range turn.Reply.Blocks[bi].Calls {
				call := &turn.Reply.Blocks[bi].Calls[ci]
				source, ok := sources[call.ID]
				if !ok {
					continue
				}
				var loaded bool
				if source.summary.HasDetails {
					var value protocol.ToolDetail
					if value, loaded = r.controller.ConversationDetail(source.ref); loaded {
						r.bindPluginCards(cv, call.ID, value)
						ProjectToolDetail(call, source.summary, value, locales...)
					}
				}
				call.BackgroundDetail = nil
				if ref := source.historical; ref != nil && state.Connected && !call.Pending && call.State != conversation.Denied && call.State != conversation.StateInterrupted {
					call.BackgroundDetail = func() conversation.BackgroundDetail {
						return r.historicalCommandDetail(*ref)
					}
				}
				if !source.summary.HasDetails || loaded {
					continue
				}
				if call.Output == nil {
					call.Output = &conversation.Output{}
				}
				ref := source.ref
				call.Load = func() {
					key := fmt.Sprintf("detail/%s/%s/%d/%s", ref.Session, ref.Entry, ref.Index, ref.Revision)
					if r.busy[key] {
						return
					}
					r.busy[key] = true
					r.async(func() {
						_, err := r.controller.ReadConversationDetail(r.ctx, ref)
						r.enqueue(func(c *ui.Context) {
							delete(r.busy, key)
							if err != nil {
								r.report(c, err, "session_changed")
							} else {
								r.projectConversations()
							}
						})
					})
				}
			}
		}
	}
}

// ProjectToolDetail applies a loaded tool detail to its projected call without
// I/O. Live views and offline acceptance replay share the same row, content and
// failure projection; loading callbacks and plugin cards remain runtime-owned.
func ProjectToolDetail(call *conversation.Call, summary *protocol.ToolDetailSummary, loaded protocol.ToolDetail, locales ...string) {
	language := ""
	if len(locales) > 0 {
		language = locales[0]
	}
	value := protocol.ToolDetailSummary{}
	if summary != nil {
		value = *summary
	}
	value.Resolved = loaded.Resolved
	projected := projectCall(call.Name, &value, language)
	projected.ID = call.ID
	projected.Pending = call.Pending
	projected.Running = call.Running
	projected.Standalone, projected.Terminal = call.Standalone, call.Terminal
	projected.Background = call.Background
	if !projected.Failed {
		projected.State = call.State
	}
	if call.State == conversation.Denied || call.State == conversation.StateInterrupted {
		projected.State = call.State
		projected.Failed = false
		projected.Warning = true
		projected.Running = false
	}
	if call.Pending {
		projected.State = conversation.Pending
		projected.Failed = false
		projected.Running = false
	}
	projectToolContent(&projected, loaded, language)
	if call.Output != nil && len(call.Output.Images) > 0 {
		if projected.Output == nil {
			projected.Output = &conversation.Output{}
		}
		projected.Output.Images = call.Output.Images
	}
	*call = projected
}
