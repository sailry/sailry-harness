package live

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"strings"
	"sync"

	"github.com/sailry/sailry-harness/internal/client"
	nodeplugins "github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/sdk/desktop/view"
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
	// Generations include detail arrivals as well as new conversation events.
	// Keep only the identities of the sources already published to the plugin.
	sources map[string]assistantToolSource
}

type assistantToolSource struct {
	session    protocol.SessionID
	generation uint64
}

type pluginAssistantCall = view.ConversationToolCall
type pluginAssistantHistory = view.ConversationToolHistory

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
	if len(state.conversations) == 0 && len(owner.sources) == 0 && owner.body == nil {
		return
	}
	if owner.sources != nil && len(owner.sources) == len(state.conversations) {
		unchanged := true
		for identity, mounted := range state.conversations {
			source := state.assistantToolSource(mounted)
			previous, ok := owner.sources[identity]
			if !ok || previous != source || source.session != (protocol.SessionID{}) && source.generation == 0 {
				unchanged = false
				break
			}
		}
		if unchanged {
			return
		}
	}
	prefix := nodeplugins.ToolAlias(state.scope.Package.Name, "")
	conversations := make(map[string]pluginAssistantHistory, len(state.conversations))
	sources := make(map[string]assistantToolSource, len(state.conversations))
	waiting := false
	for identity, mounted := range state.conversations {
		source := state.assistantToolSource(mounted)
		history := pluginAssistantHistory{Calls: []pluginAssistantCall{}}
		if source.session != (protocol.SessionID{}) {
			if current, exists := state.runtime.conversationState(source.session); exists {
				var pending bool
				history, pending = state.assistantHistory(current, prefix)
				waiting = waiting || pending
			}
			source.generation = state.runtime.conversationCopies[source.session].generation
		}
		sources[identity] = source
		conversations[identity] = history
	}
	body, err := json.Marshal(conversations)
	if err != nil {
		return
	}
	if waiting {
		owner.sources = nil
	} else {
		owner.sources = sources
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

func (state *pluginNativeState) assistantToolSource(mounted *pluginNativeConversation) assistantToolSource {
	session := mounted.session
	if session == (protocol.SessionID{}) && mounted.view != nil {
		if captured, exists := state.runtime.session(mounted.view.ID); exists {
			session = captured.ID
		}
	}
	return assistantToolSource{session: session, generation: state.runtime.conversationCopies[session].generation}
}

// assistantHistory lists the package's calls of one conversation. A call's
// arguments and result are in its tool detail, which the Client reads on
// demand: a call whose detail is not loaded yet is left out until it is, as
// a call without arguments is.
func (state *pluginNativeState) assistantHistory(current client.ConversationState, prefix string) (pluginAssistantHistory, bool) {
	r := state.runtime
	return r.projectConversationTools(current, func(name string) (string, bool) {
		return strings.CutPrefix(name, prefix)
	}, 0, func(ref protocol.ToolDetailReference, loaded bool) bool {
		if loaded {
			return false
		}
		key := assistantDetailKey(ref)
		if state.assistantTools.failed[key] {
			return false
		}
		r.loadConversationToolDetail(r.ctx, ref, func() { state.assistantTools.failed[key] = true })
		return true
	})
}
