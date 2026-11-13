package live

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"slices"
	"strconv"
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	nodeplugins "github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// pluginNativeAssistantTools is the package's own tool calls in the
// assistants its view mounts, read from the Client's conversation
// projection: a mounted assistant has no second history.
type pluginNativeAssistantTools struct {
	mu      sync.Mutex
	body    json.RawMessage
	cursor  string
	changed chan struct{}
	// failed is the details whose read failed: the conversation's own row
	// reports and loads them again, the observer does not retry them.
	failed map[string]bool
	// signature names what the published value is built from — the mounted
	// assistants, their sessions, revisions and available call details — so
	// an idle history is not hashed every frame.
	signature string
}

type pluginAssistantCall struct {
	Key         string          `json:"key"`
	Name        string          `json:"name"`
	Arguments   json.RawMessage `json:"arguments"`
	State       string          `json:"state"`
	TimestampMs int64           `json:"timestamp_ms"`
	Result      json.RawMessage `json:"result"`
	Sequence    *string         `json:"sequence"`
}

type pluginAssistantHistory struct {
	Connected bool                  `json:"connected"`
	Session   *protocol.SessionID   `json:"session,omitempty"`
	Revision  string                `json:"revision,omitempty"`
	Latest    *string               `json:"latest,omitempty"`
	Calls     []pluginAssistantCall `json:"calls"`
}

func (state *pluginNativeState) assistantToolOperations() {
	state.assistantTools = &pluginNativeAssistantTools{changed: make(chan struct{}), failed: map[string]bool{}}
	state.native.Operations["sailry.nextAssistantTools"] = state.nextAssistantTools
	state.refreshAssistantTools()
}

func (state *pluginNativeState) nextAssistantTools(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	seen, err := pluginNativeArgument[string](arguments, 0)
	if err != nil {
		return nil, err
	}
	owner := state.assistantTools
	for {
		owner.mu.Lock()
		body, cursor, changed := owner.body, owner.cursor, owner.changed
		owner.mu.Unlock()
		if cursor != seen {
			return json.Marshal(map[string]any{"cursor": cursor, "conversations": body})
		}
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-state.native.Context.Done():
			return nil, state.native.Context.Err()
		case <-changed:
		}
	}
}

// refreshAssistantTools publishes the mounted assistants' calls each frame.
// The cursor is the digest of the published value, so an unchanged history
// wakes no waiter.
func (state *pluginNativeState) refreshAssistantTools() {
	owner := state.assistantTools
	if owner == nil {
		return
	}
	prefix := nodeplugins.ToolAlias(state.scope.Package.Name, "")
	conversations := map[string]pluginAssistantHistory{}
	var signature strings.Builder
	waiting := false
	identities := make([]string, 0, len(state.conversations))
	for identity := range state.conversations {
		identities = append(identities, identity)
	}
	slices.Sort(identities)
	for _, identity := range identities {
		mounted := state.conversations[identity]
		session := mounted.session
		if session == (protocol.SessionID{}) && mounted.view != nil {
			if captured, exists := state.runtime.session(mounted.view.ID); exists {
				session = captured.ID
			}
		}
		history := pluginAssistantHistory{Calls: []pluginAssistantCall{}}
		revision, connected := uint64(0), false
		if session != (protocol.SessionID{}) {
			if current, exists := state.runtime.controller.Conversation(session); exists {
				var pending bool
				history, pending = state.assistantHistory(current, prefix)
				waiting = waiting || pending
				revision, connected = current.Snapshot.Page.Revision, current.Connected
			}
		}
		fmt.Fprintf(&signature, "%q/%s/%d/%t", identity, session, revision, connected)
		// Detail reads populate the Client cache without changing the page
		// revision. Include the calls now available, including after a
		// failed read is retried by the conversation's own tool row.
		for _, call := range history.Calls {
			fmt.Fprintf(&signature, "/%q", call.Key)
		}
		signature.WriteByte(';')
		conversations[identity] = history
	}
	// A detail still loading leaves the published value short: it is
	// rebuilt until every call it holds has its arguments.
	if signature.String() == owner.signature && !waiting {
		return
	}
	owner.signature = signature.String()
	body, err := json.Marshal(conversations)
	if err != nil {
		return
	}
	digest := blake3.Sum256(body)
	cursor := hex.EncodeToString(digest[:])
	owner.mu.Lock()
	defer owner.mu.Unlock()
	if cursor == owner.cursor {
		return
	}
	owner.body, owner.cursor = body, cursor
	close(owner.changed)
	owner.changed = make(chan struct{})
}

// assistantHistory lists the package's calls of one conversation. A call's
// arguments and result are in its tool detail, which the Client reads on
// demand: a call whose detail is not loaded yet is left out until it is, as
// a call without arguments is.
func (state *pluginNativeState) assistantHistory(current client.ConversationState, prefix string) (pluginAssistantHistory, bool) {
	page := current.Snapshot.Page
	history := pluginAssistantHistory{Connected: current.Connected, Calls: []pluginAssistantCall{}}
	if page.Session == (protocol.SessionID{}) {
		return history, false
	}
	session := page.Session
	history.Session, history.Revision = &session, strconv.FormatUint(page.Revision, 10)
	entries := map[string]protocol.ConversationEntry{}
	var latest *uint64
	for _, entry := range page.Entries {
		entries[entry.ID] = entry
		if latest == nil || entry.Sequence > *latest {
			sequence := entry.Sequence
			latest = &sequence
		}
	}
	if latest != nil {
		text := strconv.FormatUint(*latest, 10)
		history.Latest = &text
	}
	waiting := false
	for _, call := range client.ToolCalls(page) {
		name, owned := strings.CutPrefix(call.Name, prefix)
		source, sourced := entries[call.Source.Entry]
		if !owned || !sourced || call.Reference == nil || call.Summary == nil || !call.Summary.HasDetails {
			continue
		}
		detail, loaded := state.runtime.controller.ConversationDetail(*call.Reference)
		if !loaded {
			state.loadAssistantDetail(*call.Reference)
			waiting = waiting || !state.assistantTools.failed[assistantDetailKey(*call.Reference)]
			continue
		}
		if len(detail.Arguments) == 0 || string(detail.Arguments) == "null" {
			continue
		}
		projected := pluginAssistantCall{
			Key:  fmt.Sprintf("%s:%s-%d", call.Turn, call.Source.Entry, call.Source.Index),
			Name: name, Arguments: detail.Arguments, State: call.State,
			TimestampMs: source.TimestampMs, Result: json.RawMessage("null"),
		}
		if detail.Result != nil {
			projected.Result = plugins.PublicToolResult(*detail.Result)
		}
		if call.Response != nil {
			if response, exists := entries[call.Response.Entry]; exists {
				sequence := strconv.FormatUint(response.Sequence, 10)
				projected.Sequence = &sequence
			}
		}
		history.Calls = append(history.Calls, projected)
	}
	return history, waiting
}

// assistantDetailKey names a call's detail as the conversation's own rows do,
// so both share one load and one pending entry.
func assistantDetailKey(ref protocol.ToolDetailReference) string {
	return fmt.Sprintf("detail/%s/%s/%d/%s", ref.Session, ref.Entry, ref.Index, ref.Revision)
}

// loadAssistantDetail reads a call's detail once, under the key the
// conversation's own rows load it by, and redraws when it arrives.
func (state *pluginNativeState) loadAssistantDetail(ref protocol.ToolDetailReference) {
	r := state.runtime
	key := assistantDetailKey(ref)
	if r.busy[key] || state.assistantTools.failed[key] {
		return
	}
	r.busy[key] = true
	r.async(func() {
		_, err := r.controller.ReadConversationDetail(r.ctx, ref)
		r.enqueue(func(*ui.Context) {
			delete(r.busy, key)
			if err != nil {
				state.assistantTools.failed[key] = true
			}
		})
	})
}
