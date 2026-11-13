package live

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/link"
	nodeplugins "github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestNativeAssistantTools asserts a mounted assistant publishes the
// package's own calls, with their arguments and result from the tool detail,
// and that a waiter holding the current cursor waits for a change.
func TestNativeAssistantTools(t *testing.T) {
	node, session, turn := protocol.NodeID{1}, protocol.SessionID{9}, protocol.TurnID{7}
	reference := protocol.PluginReference{Name: "databases-fixture", Digest: strings.Repeat("a", 64)}
	name := nodeplugins.ToolAlias(reference.Name, "run_sql")
	id := "call-1"
	summary := &protocol.ToolDetailSummary{Revision: "r1", HasDetails: true}
	foreign := nodeplugins.ToolAlias("another-package", "run_sql")
	transport := &conversationTestTransport{snapshot: protocol.DefaultSnapshot()}
	transport.snapshot.Node = node
	transport.conversation = protocol.DefaultConversationSnapshot()
	transport.conversation.Node = node
	transport.conversation.Page.Session, transport.conversation.Page.Revision = session, 4
	transport.conversation.Page.Entries = []protocol.ConversationEntry{
		{Sequence: 5, ID: "e1", Turn: turn, TimestampMs: 1700, Parts: []protocol.ConversationPart{
			{Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{ID: &id, Name: name, Summary: summary}},
			{Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{Name: foreign, Summary: summary}},
		}},
		{Sequence: 6, ID: "e2", Turn: turn, TimestampMs: 1800, Parts: []protocol.ConversationPart{
			{Kind: "tool_result", Data: protocol.ConversationPartToolResultPayload{ID: &id, Name: name, Summary: summary}},
		}},
	}
	detail, _ := json.Marshal(protocol.ToolDetail{Arguments: json.RawMessage(`{"sql":"select 1"}`), Result: new(json.RawMessage(`{"rows":[[1]]}`)), Images: []protocol.ConversationImage{}})
	reads := 0
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		if request.Command.Kind != "read_tool_detail" {
			return link.Response{}, false
		}
		reads++
		input := request.Command.Data.(protocol.CommandReadToolDetailPayload)
		page := protocol.ToolDetailPage{
			Reference:  protocol.ToolDetailReference{Session: input.Session, Turn: input.Turn, Entry: input.Entry, Index: input.Index, Call: input.Call, Revision: input.ExpectedRevision},
			TotalBytes: uint64(len(detail)), Text: string(detail),
		}
		return link.Response{Output: protocol.Output{Kind: "tool_detail", Data: page}}, true
	}
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	watched := make(chan struct{}, 8)
	stop, err := controller.WatchConversation(t.Context(), session, func() { watched <- struct{}{} })
	if err != nil {
		t.Fatal(err)
	}
	defer stop()
	for {
		if current, exists := controller.Conversation(session); exists && current.Snapshot.Page.Session == session {
			break
		}
		<-watched
	}
	runtime, err := NewWithPreferences(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"))
	if err != nil {
		t.Fatal(err)
	}
	defer runtime.Close()
	native := plugins.NewNativeBinding(t.Context())
	native.Operations = map[string]plugins.HostOperation{}
	scope := protocol.DefaultPluginContext()
	scope.Package = reference
	state := &pluginNativeState{runtime: runtime, scope: scope, native: &native, conversations: map[string]*pluginNativeConversation{
		"chat": {session: session},
	}}
	state.assistantToolOperations()
	next := native.Operations["sailry.nextAssistantTools"]
	read := func(ctx context.Context, cursor string) (string, map[string]pluginAssistantHistory, error) {
		t.Helper()
		body, err := next(ctx, []json.RawMessage{json.RawMessage(`"` + cursor + `"`)})
		if err != nil {
			return "", nil, err
		}
		var value struct {
			Cursor        string                            `json:"cursor"`
			Conversations map[string]pluginAssistantHistory `json:"conversations"`
		}
		if err := json.Unmarshal(body, &value); err != nil {
			t.Fatal(err)
		}
		return value.Cursor, value.Conversations, nil
	}
	// The detail is read on demand: until it arrives the call is left out.
	first, conversations, err := read(t.Context(), "")
	if err != nil || len(conversations["chat"].Calls) != 0 || !conversations["chat"].Connected {
		t.Fatalf("a call without its detail was published: %+v %v", conversations, err)
	}
	runtime.workers.Wait()
	state.refreshAssistantTools()
	second, conversations, err := read(t.Context(), first)
	if err != nil || second == first {
		t.Fatalf("the loaded detail did not change the cursor: %q %q %v", first, second, err)
	}
	history := conversations["chat"]
	if len(history.Calls) != 1 || history.Latest == nil || *history.Latest != "6" || history.Revision != "4" ||
		history.Session == nil || *history.Session != session {
		t.Fatalf("unexpected history: %+v", history)
	}
	call := history.Calls[0]
	if call.Key != turn.String()+":e1-0" || call.Name != "run_sql" || call.State != "returned" || call.TimestampMs != 1700 ||
		string(call.Arguments) != `{"sql":"select 1"}` || string(call.Result) != `{"rows":[[1]]}` ||
		call.Sequence == nil || *call.Sequence != "6" {
		t.Fatalf("unexpected call: %+v", call)
	}
	// A frame with no change keeps the cursor; a waiter on it waits.
	state.refreshAssistantTools()
	cancelled, cancel := context.WithCancel(t.Context())
	cancel()
	if _, _, err := read(cancelled, second); !errors.Is(err, context.Canceled) {
		t.Fatalf("a current cursor returned without a change: %v", err)
	}
	transport.mu.Lock()
	defer transport.mu.Unlock()
	if reads != 1 {
		t.Fatalf("the detail was read %d times", reads)
	}
}
