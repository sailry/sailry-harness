package live

import (
	"path/filepath"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDraftConnectionFollowsExecutionHost(t *testing.T) {
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	snapshot.Providers = []protocol.ConversationProvider{{ID: protocol.ProviderID{4}, Enabled: true,
		Models: []protocol.ConversationModel{{ID: "fixture", DefaultEffort: protocol.Effort{Kind: "default"}}}}}
	transport := &conversationTestTransport{snapshot: snapshot}
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	runtime, err := NewWithPreferences(t.Context(), controller, preferences.Open(filepath.Join(t.TempDir(), "preferences.json")))
	if err != nil {
		t.Fatal(err)
	}
	defer runtime.Close()
	draft := runtime.App.Data.Find(runtime.App.Sidebar.Selected)
	if draft == nil || !unsentDraft(draft) {
		t.Fatal("the fixture has no unadmitted draft")
	}
	state := runtime.App.Composer(draft.ID)
	state.Draft = "Keep this input"
	state.Host = (protocol.NodeID{2}).String()
	var context *composer.Context
	tester := ui.NewTester(func(view *ui.Context) {
		theme.Apply(view, "", theme.Light)
		context, _ = runtime.conversation(view, draft, state)
		ui.Box(view).Fill()
	}, 800, 600)
	tester.Frame()
	if context.Readonly || !context.SendBlocked || draft.Session.Connection != conversation.Offline {
		t.Fatal("the unavailable Host did not leave an editable, unsendable draft")
	}
	state.Host = snapshot.Node.String()
	tester.Frame()
	if context.Readonly || context.SendBlocked || draft.Session.Connection != conversation.Connected {
		t.Fatalf("the available Host retained stale connection state: readonly=%v blocked=%v connection=%v", context.Readonly, context.SendBlocked, draft.Session.Connection)
	}
	// Reconnection on the same Host also has no session observation yet.
	draft.Session.Connection = conversation.Reconnecting
	tester.Frame()
	if draft.Session.Connection != conversation.Connected || state.Draft != "Keep this input" || !unsentDraft(draft) {
		t.Fatal("draft recovery did not clear only its connection presentation")
	}
}
