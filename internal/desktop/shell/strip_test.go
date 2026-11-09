package shell

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// The strip groups sessions by lane, folds a full group into +N and opens
// a hidden session from its popover.
func TestHeaderStrip(t *testing.T) {
	a, tt := newTester(t)
	// Four finished unread: two show, then +2.
	a.Data.Find("sailry/release").Status = preview.Unread
	tt.Frame()
	for _, name := range []string{
		"Tester coverage for the composer · Needs attention",
		"Invoice PDF rendering · Running",
		"Port the neutral theme tokens · Completed",
		"Windows release lint fixes · Completed",
	} {
		if _, ok := tt.AccessByRole(ui.RoleButton, name); !ok {
			t.Errorf("no strip button %q", name)
		}
	}
	if _, ok := tt.AccessByRole(ui.RoleButton, "Stabilize MCP warm-up tests · Completed"); ok {
		t.Error("a third finished session shows instead of +N")
	}
	snapshot(t, tt, "strip")
	clickButton(t, tt, "Completed · +2")
	tt.Frame()
	snapshot(t, tt, "strip-more")
	clickButton(t, tt, "Translate release notes · Completed")
	tt.Frame()
	if a.Sidebar.Selected != "chat/translate" || a.Page != ConversationPage {
		t.Fatalf("selected %q on %v", a.Sidebar.Selected, a.Page)
	}
	if _, ok := tt.AccessByRole(ui.RoleButton, "Stabilize MCP warm-up tests · Completed"); ok {
		t.Error("the popover stays open after a row opens its session")
	}
}
