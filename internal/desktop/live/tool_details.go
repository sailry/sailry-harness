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
	language := ""
	if len(locales) > 0 {
		language = locales[0]
	}
	type source struct {
		ref     protocol.ToolDetailReference
		name    string
		summary *protocol.ToolDetailSummary
	}
	sources := map[string]source{}
	pending := map[string][]toolAssociation{}
	for _, entry := range state.Snapshot.Page.Entries {
		for index, part := range entry.Parts {
			var id *string
			var name string
			var summary *protocol.ToolDetailSummary
			key := toolCallKey(entry.ID, uint64(index))
			switch part.Kind {
			case "tool_call":
				if p, ok := detail[protocol.ConversationPartToolCallPayload](part.Data); ok {
					id, name, summary = p.ID, p.Name, p.Summary
					if id != nil {
						k := toolPendingKey(entry.Turn, entry.Branch, *id)
						pending[k] = append(pending[k], toolAssociation{ID: key, Name: name})
					}
				}
			case "tool_result":
				if p, ok := detail[protocol.ConversationPartToolResultPayload](part.Data); ok {
					id, name, summary = p.ID, p.Name, p.Summary
					if id != nil {
						k := toolPendingKey(entry.Turn, entry.Branch, *id)
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
			if !summary.HasDetails {
				delete(sources, key)
				continue
			}
			ref := protocol.ToolDetailReference{Session: state.Snapshot.Page.Session, Turn: entry.Turn, Entry: entry.ID, Index: uint64(index), Call: id, Revision: summary.Revision}
			sources[key] = source{ref, name, summary}
		}
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
				if loaded, ok := r.controller.ConversationDetail(source.ref); ok {
					r.bindPluginCards(cv, call.ID, loaded)
					summary := *source.summary
					summary.Resolved = loaded.Resolved
					projected := projectCall(source.name, &summary, language)
					projected.ID = call.ID
					projected.Pending = call.Pending
					projected.Running = call.Running
					projected.Standalone, projected.Terminal = call.Standalone, call.Terminal
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
					projectToolContent(&projected, loaded)
					if call.Output != nil && len(call.Output.Images) > 0 {
						if projected.Output == nil {
							projected.Output = &conversation.Output{}
						}
						projected.Output.Images = call.Output.Images
					}
					*call = projected
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
