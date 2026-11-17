package engine

import (
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestTurnContextAppendsOnlyChangedSections(t *testing.T) {
	m := &scriptedModel{scripts: []script{response(call("title", "set_session_title", `{"title":"Brief conversation"}`), finish())}}
	for range 8 {
		m.scripts = append(m.scripts, response(text("Here"), finish()))
	}
	f := newFixture(t, m)
	config := f.session.Config
	config.Mode = protocol.WorkModeFlagship
	f.session = f.command("set_session_config", &protocol.CommandSetSessionConfigPayload{Session: f.session.ID, ExpectedRevision: f.session.Revision, Config: config}).Data.(protocol.Session)
	f.start()
	var turns []protocol.TurnID
	for range 8 {
		turn := f.submit("Hello")
		turns = append(turns, turn)
		f.waitRun(turn, protocol.ConversationStatusCompleted)
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	if len(m.calls) != 9 {
		t.Fatalf("model requests = %d, want 9", len(m.calls))
	}
	for i := 1; i < len(m.calls); i++ {
		previous, current := m.calls[i-1].Prompt, m.calls[i].Prompt
		if len(current) < len(previous) || !reflect.DeepEqual(previous, current[:len(previous)]) {
			t.Fatalf("request %d rewrote an earlier prompt prefix", i+1)
		}
	}
	last := promptText(m.calls[len(m.calls)-1])
	for _, section := range []string{"Active work mode: Flagship", "Session environment"} {
		if count := strings.Count(last, section); count != 1 {
			t.Errorf("stable section %q occurs %d times, want 1", section, count)
		}
	}
	var contexts []canonical.Record
	for _, record := range f.state().Records {
		if runtime := record.Metadata.Runtime; runtime != nil && runtime.TurnContext {
			contexts = append(contexts, record)
		}
	}
	if len(contexts) != 9 {
		t.Fatalf("context updates = %d, want baseline, title and seven turn identities", len(contexts))
	}
	for index, record := range contexts[1:] {
		key := "execution_identity"
		if index == 0 {
			key = "title"
		}
		parts := record.Metadata.Runtime.Notes.Context.Parts
		if len(parts) != 1 || parts[key] == "" {
			t.Fatalf("context update %d resent unchanged sections: %v", index+1, parts)
		}
		if key == "execution_identity" {
			value := record.Message().Content[0].Data.(*canonical.TextContent).Text
			if !strings.Contains(value, "Turn ID: "+turns[index].String()) || !strings.Contains(value, "Session ID: "+f.session.ID.String()) {
				t.Fatalf("context update %d did not identify the current turn", index+1)
			}
		}
	}
}
