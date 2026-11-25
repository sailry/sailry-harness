package live

import (
	"context"
	"encoding/json"
	"fmt"
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	nodeplugins "github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/sdk/desktop/view"
)

// A compiled workspace watches the same Client projection as the docked
// conversation. It never subscribes to an arbitrary session named by a script.
type dockedToolObserver struct {
	parent    context.Context
	ctx       context.Context
	cancel    context.CancelFunc
	names     []string
	legacy    string
	publish   func(view.ConversationToolHistory)
	session   protocol.SessionID
	source    assistantToolSource
	connected bool
	seen      bool
	loads     map[string]bool
}

func newDockedToolObserver(ctx context.Context, spec view.ConversationToolObserver) *dockedToolObserver {
	return &dockedToolObserver{parent: ctx, publish: spec.Publish, names: slices.Clone(spec.Names), legacy: spec.LegacyPackage}
}

func (mounted *windowDockedConversation) observeTools(available bool) {
	r, observer := mounted.runtime, mounted.tools
	var session protocol.SessionID
	var prefix string
	if available && !r.App.DemoOn() && mounted.session != (protocol.SessionID{}) {
		if current, ok := r.sessionWithID(mounted.session); ok && !r.sessionHidden(current.ID) && mounted.match(current) {
			session = current.ID
			binding := current.Config.Assistant
			if binding != nil && binding.Package.Name == observer.legacy && nodeplugins.IsLegacyDesktopPackage(binding.Package.Name, binding.Package.Digest) {
				prefix = nodeplugins.ToolAlias(observer.legacy, "")
			}
		}
	}
	observer.refresh(r, session, prefix)
}

func (o *dockedToolObserver) refresh(r *Runtime, session protocol.SessionID, prefix string) {
	if o.ctx == nil || o.session != session {
		o.close()
		o.ctx, o.cancel = context.WithCancel(o.parent)
		o.session, o.loads = session, make(map[string]bool)
		o.seen = false
	}
	connected := r.controller.Connected()
	source := assistantToolSource{session: session}
	var current client.ConversationState
	var exists bool
	if session != (protocol.SessionID{}) {
		current, exists = r.conversationState(session)
		if r.conversationFeeds[session] != nil {
			source.generation = r.conversationCache[session].generation
		} else {
			source.generation = r.conversationCopies[session].generation
		}
	}
	if o.seen && o.source == source && o.connected == connected {
		return
	}
	history := view.ConversationToolHistory{Connected: connected, Calls: []view.ConversationToolCall{}}
	if exists {
		ctx := o.ctx
		match := func(name string) (string, bool) {
			name = strings.TrimPrefix(name, prefix)
			return name, slices.Contains(o.names, name)
		}
		// Match the Client's detail cache bound. Keep a read attempt only
		// while its reference is in this window of calls.
		const detailWindow = 64
		loads := make(map[string]bool, min(len(o.loads), detailWindow))
		var waiting bool
		history, waiting = r.projectConversationTools(current, match, detailWindow, func(ref protocol.ToolDetailReference, loaded bool) bool {
			key := assistantDetailKey(ref)
			if loaded {
				loads[key] = false
				return false
			}
			if pending, known := o.loads[key]; known {
				loads[key] = pending
				return pending
			}
			// The Client shares the read with a tool row, but the observer's
			// own waiter stops when its resource or session is replaced.
			pending := r.async(func() {
				_, _ = r.controller.ReadConversationDetail(ctx, ref)
				r.enqueue(func(*ui.Context) {
					if ctx.Err() != nil || !o.loads[key] {
						return
					}
					o.loads[key] = false
					o.seen = false
				})
			})
			loads[key] = pending
			return pending
		})
		o.loads = loads
		if waiting {
			o.source, o.connected, o.seen = source, connected, true
			return
		}
	}
	o.source, o.connected, o.seen = source, connected, true
	o.publish(history)
}

func (o *dockedToolObserver) close() {
	if o.cancel != nil {
		o.cancel()
	}
}

// projectConversationTools reads only details for matching calls. Arguments
// and results come from the Client's canonical history and detail cache.
func (r *Runtime) projectConversationTools(current client.ConversationState, match func(string) (string, bool), limit int, observe func(protocol.ToolDetailReference, bool) bool) (view.ConversationToolHistory, bool) {
	page := current.Snapshot.Page
	history := view.ConversationToolHistory{Connected: current.Connected, Calls: []view.ConversationToolCall{}}
	if page.Session == (protocol.SessionID{}) {
		return history, false
	}
	session := page.Session
	history.Session, history.Revision = &session, strconv.FormatUint(page.Revision, 10)
	entries := make(map[string]protocol.ConversationEntry, len(page.Entries))
	var latest uint64
	for i, entry := range page.Entries {
		entries[entry.ID] = entry
		if i == 0 || entry.Sequence > latest {
			latest = entry.Sequence
		}
	}
	if len(page.Entries) != 0 {
		history.Latest = new(strconv.FormatUint(latest, 10))
	}
	waiting := false
	calls := client.ToolCalls(page)
	if limit > 0 {
		start := len(calls)
		for start > 0 && limit > 0 {
			start--
			if _, owned := match(calls[start].Name); owned {
				limit--
			}
		}
		calls = calls[start:]
	}
	for _, call := range calls {
		name, owned := match(call.Name)
		source, sourced := entries[call.Source.Entry]
		if !owned || !sourced || call.Reference == nil || call.Summary == nil || !call.Summary.HasDetails {
			continue
		}
		detail, loaded := r.controller.ConversationDetail(*call.Reference)
		waiting = observe(*call.Reference, loaded) || waiting
		if !loaded {
			continue
		}
		if len(detail.Arguments) == 0 || string(detail.Arguments) == "null" {
			continue
		}
		projected := view.ConversationToolCall{
			Key:  fmt.Sprintf("%s:%s-%d", call.Turn, call.Source.Entry, call.Source.Index),
			Name: name, Arguments: detail.Arguments, State: call.State,
			TimestampMs: source.TimestampMs, Result: json.RawMessage("null"),
		}
		if detail.Result != nil {
			projected.Result = plugins.PublicToolResult(*detail.Result)
		}
		if call.Response != nil {
			if response, exists := entries[call.Response.Entry]; exists {
				projected.Sequence = new(strconv.FormatUint(response.Sequence, 10))
			}
		}
		history.Calls = append(history.Calls, projected)
	}
	return history, waiting
}

func assistantDetailKey(ref protocol.ToolDetailReference) string {
	return fmt.Sprintf("detail/%s/%s/%d/%s", ref.Session, ref.Entry, ref.Index, ref.Revision)
}

func (r *Runtime) loadConversationToolDetail(ctx context.Context, ref protocol.ToolDetailReference, failed func()) {
	key := assistantDetailKey(ref)
	if r.busy[key] {
		return
	}
	r.busy[key] = true
	if !r.async(func() {
		_, err := r.controller.ReadConversationDetail(ctx, ref)
		r.enqueue(func(*ui.Context) {
			delete(r.busy, key)
			if err != nil {
				failed()
			}
		})
	}) {
		delete(r.busy, key)
	}
}
