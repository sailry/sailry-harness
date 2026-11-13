package acceptance

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	core "github.com/sailry/sailry-harness/internal/acceptance"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func inputFixture(t *testing.T, loading bool) *liveWindow {
	t.Helper()
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = protocol.ProviderID{1}, "fixture-model"
	w := &liveWindow{window: &window{ctx: t.Context(), result: &core.CaseResult{}},
		config: config, session: protocol.Session{ID: protocol.SessionID{2}}}
	if err := w.open(func(data *preview.Data) {
		cv := data.Find(idleSession)
		cv.ID, cv.Turns = w.session.ID.String(), nil
		cv.Session = conversation.Session{Loading: loading, Connection: conversation.Connected}
		data.Models = []viewmodel.Model{{ID: modelID(config), Name: "Fixture model"}}
	}); err != nil {
		t.Fatal(err)
	}
	if err := w.selectConversation(w.session.ID.String()); err != nil {
		t.Fatal(err)
	}
	return w
}

func TestLiveInputWaitsForRenderedConversation(t *testing.T) {
	w := inputFixture(t, true)
	state := client.ConversationState{Connected: true}
	// The observer has completed, but the last UI frame still shows loading.
	if ready, detail := w.readyForInput(state, true); ready {
		t.Fatalf("input became ready before the UI projection: %s", detail)
	}
	w.app.Data.Find(w.session.ID.String()).Session.Loading = false
	w.tester.Frame()
	if ready, detail := w.readyForInput(state, true); !ready {
		t.Fatalf("the rendered connected composer is not ready: %s", detail)
	}
	w.app.Sidebar.Selected = preview.Big
	w.tester.Frame()
	if ready, detail := w.readyForInput(state, true); ready {
		t.Fatalf("another selected conversation satisfied readiness: %s", detail)
	}
}

func TestLiveInputRefreshesAfterLoadingBeforeClick(t *testing.T) {
	w := inputFixture(t, true)
	// Do not render this change here. Without the input helper's refresh,
	// PointerDown targets the loading branch's editor and its ensuing frame
	// replaces it with the welcome branch's editor before text arrives.
	w.app.Data.Find(w.session.ID.String()).Session.Loading = false
	const text = "Acceptance fixture: preserve the first typed message"
	if err := w.typeMessage(text); err != nil {
		t.Fatal(err)
	}
	field, err := w.control(ui.RoleTextField, label("composer"))
	if err != nil {
		t.Fatal(err)
	}
	if draft := w.app.Composer(w.session.ID.String()).Draft; draft != text || field.Value != text || !field.Focused {
		t.Fatalf("the first input was lost across the welcome transition: draft=%q, field=%+v", draft, field)
	}
}

func TestLiveInputRequiresEditorFocus(t *testing.T) {
	w := &window{ctx: t.Context(), result: &core.CaseResult{}, app: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})}
	const original = "PRIVATE-EXISTING-DRAFT"
	entered := original
	w.tester = ui.NewTester(func(c *ui.Context) {
		// Keep the parent focused after each event. The looser
		// Tester.Focused check accepts that parent, but text cannot enter it.
		parent := ui.Column(c.Key("parent")).Fill().Focusable().Focus()
		parent.Children(func() {
			ui.TextAreaBase(c.Key("input"), &entered).Label(label("composer"))
		})
	}, 400, 200)
	err := w.typeMessage("PRIVATE-DRAFT-MUST-NOT-BE-REPORTED")
	if err == nil || !strings.Contains(err.Error(), "keyboard focus") {
		t.Fatalf("typing accepted focus on the parent instead of the editor: %v", err)
	}
	if !w.tester.Focused(label("composer")) || entered != original {
		t.Fatal("the fixture did not keep focus on the parent without typing")
	}
	if len(w.result.Evidence) != 1 || w.result.Evidence[0].Kind != "composer_input" || strings.Contains(w.result.Evidence[0].Value, "PRIVATE-") {
		t.Fatalf("input failure did not retain safe diagnostics: %+v", w.result.Evidence)
	}
}
