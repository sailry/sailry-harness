//go:build integration

package live

import (
	"context"
	"fmt"
	"maps"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/node"
)

// TestSystemShortcutsUI is the B13 acceptance of the system shortcuts'
// Desktop side: KEY-022, KEY-023, KEY-024, SKEY-009, SHELL-035 and
// SHELL-070, as far as the process can be driven without the system.
//
// Substitutions and limits: no key is registered with the system here. The
// test proves the set of accelerators the entry point reads after each
// change, the conflict toast through Runtime.ReportShortcutConflict, which
// the entry point calls when a registration fails, and the commands the
// registered callbacks send. cmd/sailry-desktop's
// TestSystemShortcutRegistration proves what the entry point registers and
// unregisters for such a set.
// The front app's selection is the package variable selectionReader
// replaced by a fixed text: no other application is read. The window's
// activation is the callback handed to Runtime.CaptureSelection, counted.
func TestSystemShortcutsUI(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 180*time.Second)
	defer cancel()
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	profile := filepath.Join(root, "profile")
	desk := newSystemDesk(t, ctx, profile, node.Options{})
	provider, model := desk.provider("http://127.0.0.1:9/v1")
	session := desk.session(desk.project("Shortcut workspace"), provider, model, "Shortcut notes")

	var selected atomic.Value // [2]string: the text and the refusal's key
	var reads atomic.Int32
	selected.Store([2]string{"", "selection_empty"})
	previous := selectionReader
	selectionReader = func() (string, string) {
		reads.Add(1)
		value := selected.Load().([2]string)
		if value[1] != "" {
			return "", value[1]
		}
		return boundedSelection(value[0])
	}
	t.Cleanup(func() { selectionReader = previous })

	activate, choose := kit.KeyText(ui.Ctrl|ui.Alt, ui.KeySpace), kit.KeyText(ui.Ctrl|ui.Alt, ui.KeyS)
	saved := func() map[string]string {
		if value := preferences.Open(filepath.Join(profile, "desktop", "preferences.json")).Data.Shortcuts; value != nil {
			return *value
		}
		return nil
	}
	system := func(w *terminalUI, want map[string]string, label string) {
		t.Helper()
		settle(w, label, systemUIWait, func() bool { return maps.Equal(w.runtime.App.SystemShortcuts(), want) })
	}
	record := func(w *terminalUI, row, shows string) {
		t.Helper()
		clickRow(w, row, shows)
		settle(w, "the row records", systemUIWait, func() bool {
			_, found := rowAction(w, row, "Press shortcut")
			return found && w.runtime.App.Settings.Keys.Active != ""
		})
	}

	// Step 1: the page lists the two system shortcuts on their defaults.
	window := openTerminalWindow(t, ctx, desk.owner, profile, session, nil)
	showSection(window, settings.Shortcuts)
	settle(window, "the shortcuts page", systemUIWait, func() bool {
		_, shown := rowAction(window, "Show Sailry", activate)
		_, unset := rowAction(window, "Use selected text", "Not set")
		return shown && unset
	})
	system(window, map[string]string{"Ctrl+Alt+Space": "system.activate"}, "the default system shortcuts")
	t.Logf("step 1 passed: the page shows Show Sailry on %s and Use selected text as Not set; the entry point would register only Ctrl+Alt+Space for system.activate", activate)

	// Step 2: recording a key for the selection shortcut saves it.
	record(window, "Use selected text", "Not set")
	window.tester.Key(ui.Ctrl|ui.Alt, ui.KeyS)
	settle(window, "the recorded key shows", systemUIWait, func() bool {
		_, found := rowAction(window, "Use selected text", choose)
		return found && window.runtime.App.Settings.Keys.Active == ""
	})
	system(window, map[string]string{"Ctrl+Alt+Space": "system.activate", "Ctrl+Alt+S": "system.selection"}, "the selection shortcut joins the system's")
	settle(window, "the key reaches preferences.json", systemUIWait, func() bool { return saved()["system.selection"] == "ctrl-alt-s" })
	window.close()
	window = openTerminalWindow(t, ctx, desk.owner, profile, session, nil)
	defer window.close()
	showSection(window, settings.Shortcuts)
	settle(window, "a new Runtime shows the recorded key", systemUIWait, func() bool {
		_, found := rowAction(window, "Use selected text", choose)
		return found
	})
	system(window, map[string]string{"Ctrl+Alt+Space": "system.activate", "Ctrl+Alt+S": "system.selection"}, "a new Runtime's system shortcuts")
	t.Logf("step 2 passed: clicking the row and pressing %s saved system.selection as ctrl-alt-s in preferences.json; the entry point would register Ctrl+Alt+S, and a new Runtime shows and reports the same", choose)

	// Step 3: a key another shortcut has is refused and keeps recording.
	record(window, "Use selected text", choose)
	window.tester.Key(ui.Ctrl|ui.Alt, ui.KeySpace)
	toast(window, "Shortcut conflicts with an existing action")
	if window.runtime.App.Settings.Keys.Active != "system.selection" || saved()["system.selection"] != "ctrl-alt-s" {
		t.Fatalf("a refused key ended the recording (%q) or changed the saved key %v", window.runtime.App.Settings.Keys.Active, saved())
	}
	window.tester.Key(0, ui.KeyEscape)
	settle(window, "Escape cancels the recording", systemUIWait, func() bool {
		_, found := rowAction(window, "Use selected text", choose)
		return found && window.runtime.App.Settings.Keys.Active == ""
	})
	// The system's refusal of a registration reaches the window this way.
	window.runtime.ReportShortcutConflict()
	toast(window, "Shortcut conflicts with an existing action")
	system(window, map[string]string{"Ctrl+Alt+Space": "system.activate", "Ctrl+Alt+S": "system.selection"}, "a conflict leaves the keys")
	t.Logf("step 3 passed: recording Show Sailry's key for the selection shortcut showed the conflict toast and kept recording with the saved key; Escape cancelled; ReportShortcutConflict, the entry point's report of a refused registration, showed the same toast and left the keys")

	// Step 4: rebinding and clearing change what the entry point registers.
	record(window, "Show Sailry", activate)
	window.tester.Key(ui.Ctrl|ui.Alt, ui.KeyK)
	system(window, map[string]string{"Ctrl+Alt+K": "system.activate", "Ctrl+Alt+S": "system.selection"}, "the rebound activate shortcut")
	settle(window, "the rebound key reaches preferences.json", systemUIWait, func() bool { return saved()["system.activate"] == "ctrl-alt-k" })
	record(window, "Use selected text", choose)
	press(window, "Clear shortcut")
	system(window, map[string]string{"Ctrl+Alt+K": "system.activate"}, "the cleared selection shortcut")
	settle(window, "the cleared key reaches preferences.json", systemUIWait, func() bool {
		value, exists := saved()["system.selection"]
		return exists && value == ""
	})
	if _, found := rowAction(window, "Use selected text", "Not set"); !found {
		t.Fatalf("the cleared shortcut does not show Not set; elements: %s", window.elements())
	}
	record(window, "Show Sailry", kit.KeyText(ui.Ctrl|ui.Alt, ui.KeyK))
	press(window, "Reset to default")
	system(window, map[string]string{"Ctrl+Alt+Space": "system.activate"}, "the reset activate shortcut")
	t.Logf("step 4 passed: rebinding Show Sailry replaced Ctrl+Alt+Space by Ctrl+Alt+K in the set the entry point registers, Clear removed the selection shortcut and saved it as off, and Reset put Show Sailry back on its default")

	// Step 5: the activate shortcut's command opens the search.
	app := window.runtime.App
	app.Navigate(shell.ConversationPage)
	window.runtime.OpenSession(session.ID.String())
	settle(window, "the conversation shows", systemUIWait, func() bool { return app.Page == shell.ConversationPage && app.Sidebar.Selected == session.ID.String() })
	fields := func() []ui.AccessInfo {
		var found []ui.AccessInfo
		for _, entry := range window.tester.AccessAll() {
			if entry.Role == ui.RoleTextField && entry.Placeholder == "Search" {
				found = append(found, entry)
			}
		}
		return found
	}
	if len(fields()) != 0 {
		t.Fatalf("a search field shows before the shortcut; elements: %s", window.elements())
	}
	app.Do("system.activate")
	settle(window, "the search opens", systemUIWait, func() bool {
		found := fields()
		return len(found) == 1 && found[0].Focused
	})
	window.tester.Key(0, ui.KeyEscape)
	settle(window, "Escape closes the search", systemUIWait, func() bool { return len(fields()) == 0 })
	t.Logf("step 5 passed: system.activate, the command the registered callback sends after showing and focusing the window, opened the search with its field focused; Escape closed it")

	// Step 6: the selection shortcut shows the captured text and quotes it.
	var activations atomic.Int32
	window.runtime.SetUpdate(func(run func()) { run() })
	capture := func() {
		// What the entry point's callback does for system.selection.
		window.runtime.CaptureSelection(func() { activations.Add(1) })
	}
	capture()
	toast(window, "No text selected")
	if reads.Load() != 1 || activations.Load() != 1 || shown(window, "Selected text") != 0 {
		t.Fatalf("an empty selection: %d reads, %d activations, dialog shown %d", reads.Load(), activations.Load(), shown(window, "Selected text"))
	}
	selected.Store([2]string{"", "selection_permission"})
	capture()
	toast(window, "Allow Accessibility access in system settings")
	selected.Store([2]string{strings.Repeat("x", 65537), ""})
	capture()
	toast(window, "Selected text is too large")
	const quoted = "Harbour ledger line selected in another app"
	selected.Store([2]string{quoted, ""})
	capture()
	settle(window, "the selection dialog", systemUIWait, func() bool { return shown(window, "Selected text") > 0 && shown(window, quoted) == 1 })
	if reads.Load() != 4 || activations.Load() != 4 {
		t.Fatalf("four captures read %d times and activated %d times", reads.Load(), activations.Load())
	}
	pressIn(window, "Quote")
	settle(window, "the quote reaches the composer", systemUIWait, func() bool {
		return app.Composer(session.ID.String()).Draft == "> "+quoted && shown(window, "Selected text") == 0
	})
	// Help › Selected text and the window's own command take the same path.
	selected.Store([2]string{"Second selection", ""})
	app.Do("system.selection")
	settle(window, "the command's selection dialog", systemUIWait, func() bool { return shown(window, "Second selection") == 1 })
	pressIn(window, "Quote")
	settle(window, "the second quote follows the first", systemUIWait, func() bool {
		return app.Composer(session.ID.String()).Draft == "> "+quoted+"\n\n> Second selection"
	})
	if reads.Load() != 5 {
		t.Fatalf("the reader ran %d times", reads.Load())
	}
	t.Logf("step 6 passed: with the injected reader an empty selection, a missing permission and an oversized text each showed their toast and no dialog; a text showed in the Selected text dialog after the window's activation callback ran once per capture, and Quote put %q into the conversation's composer; the system.selection command quoted a second text after it", fmt.Sprintf("> %s", quoted))

	// Step 7: the new-conversation command, a window shortcut and menu item.
	app.Do("conversation.new")
	settle(window, "a new conversation", systemUIWait, func() bool {
		return app.Page == shell.ConversationPage && app.Sidebar.Selected != session.ID.String()
	})
	t.Logf("step 7 passed: conversation.new left the session for a new conversation draft (selected %q); it is a window command, not one of the two system shortcuts of the registry", app.Sidebar.Selected)
}
