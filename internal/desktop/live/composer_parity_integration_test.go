//go:build integration

package live

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// auditSessionOverview opens the header's overview on a session of the real
// Node: its inventory read is answered and accepted, an untouched session
// lists nothing, and ⌘L then focuses the composer. It is a step of
// TestPluginUIIntegration.
func auditSessionOverview(t *testing.T, runtime *Runtime, tester *ui.Tester, session protocol.SessionID, wait func(string, func() bool)) {
	t.Helper()
	text := func(key string) string { return i18n.In("en", key) }
	var button ui.AccessInfo
	wait("the overview button", func() bool {
		for _, e := range tester.AccessAll() {
			// The header's, not the rail's page of the same name.
			if e.Role == ui.RoleButton && e.Name == text("chat_assets_title") && e.Bounds.Y < shell.HeaderHeight && !e.Disabled {
				button = e
				return true
			}
		}
		return false
	})
	tester.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
	wait("the Node's conversation inventory", func() bool {
		read := runtime.overviews[session]
		if read != nil && read.failed {
			t.Fatalf("the real Node's inventory was refused: %q", tester.Texts())
		}
		return read != nil && read.open && read.loaded && !read.loading
	})
	for _, key := range []string{"chat_assets_activity", "chat_assets_no_activity", "chat_assets_progress", "chat_assets_no_progress", "chat_assets_output", "chat_assets_no_output", "chat_assets_sources", "chat_assets_no_sources"} {
		if !tester.HasText(text(key)) {
			t.Fatalf("the overview of an untouched session lacks %s: %q", key, tester.Texts())
		}
	}
	if tester.HasText(text("chat_assets_failed")) || tester.HasText(text("chat_assets_more")) {
		t.Fatalf("the overview of an untouched session: %q", tester.Texts())
	}
	tester.Key(0, ui.KeyEscape)
	wait("the overview closes", func() bool { return !runtime.overviews[session].open })
	tester.Key(ui.Cmd, ui.KeyL)
	wait("⌘L focuses the composer", func() bool { return tester.Focused(text("composer")) })
	t.Log("overview: the real Node answered list_conversation_assets for the session; ⌘L focused the composer")
}
