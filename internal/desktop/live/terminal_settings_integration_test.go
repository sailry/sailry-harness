//go:build integration

package live

import (
	"context"
	"encoding/json"
	"fmt"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"os"
	"path/filepath"
	"reflect"
	"regexp"
	"slices"
	"strings"
	"sync"
	"testing"

	myterminal "github.com/egoist/mygo/plugins/terminal"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// terminalGate holds the window's commands of a kind until the test lets
// them go, so a state that lasts one round trip can be observed.
type terminalGate struct {
	link.Transport
	mu      sync.Mutex
	held    map[string]chan struct{}
	refused map[string]error
}

// refuse answers the commands of kind with err, without reaching the Node,
// until the returned release is called.
func (gate *terminalGate) refuse(kind string, err error) func() {
	gate.mu.Lock()
	if gate.refused == nil {
		gate.refused = map[string]error{}
	}
	gate.refused[kind] = err
	gate.mu.Unlock()
	return func() {
		gate.mu.Lock()
		delete(gate.refused, kind)
		gate.mu.Unlock()
	}
}

// hold stops the commands of kind until the returned release is called.
func (gate *terminalGate) hold(kind string) func() {
	wait := make(chan struct{})
	gate.mu.Lock()
	gate.held[kind] = wait
	gate.mu.Unlock()
	return func() {
		gate.mu.Lock()
		if gate.held[kind] == wait {
			delete(gate.held, kind)
			close(wait)
		}
		gate.mu.Unlock()
	}
}

func (gate *terminalGate) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	gate.mu.Lock()
	wait := gate.held[request.Command.Kind]
	refused := gate.refused[request.Command.Kind]
	gate.mu.Unlock()
	if refused != nil {
		return link.Admission{}, refused
	}
	if wait != nil {
		select {
		case <-wait:
		case <-ctx.Done():
			return link.Admission{}, ctx.Err()
		}
	}
	return gate.Transport.Dispatch(ctx, request)
}

// named are the visible elements of a role with an accessible name.
func (w *terminalUI) named(role ui.Role, name string) []ui.AccessInfo {
	var found []ui.AccessInfo
	for _, entry := range w.tester.AccessAll() {
		if entry.Role == role && entry.Name == name && entry.Bounds.W > 0 && entry.Bounds.H > 0 {
			found = append(found, entry)
		}
	}
	return found
}

// one is the single visible element of a role with a name.
func (w *terminalUI) one(role ui.Role, name string) ui.AccessInfo {
	w.t.Helper()
	found := w.named(role, name)
	if len(found) != 1 {
		w.t.Fatalf("want one %v named %q, found %d; elements: %s", role, name, len(found), w.elements())
	}
	return found[0]
}

// elements lists the named elements of the frame for a failure message.
func (w *terminalUI) elements() string {
	var lines []string
	for _, entry := range w.tester.AccessAll() {
		if entry.Name == "" && entry.Value == "" && entry.Placeholder == "" {
			continue
		}
		lines = append(lines, fmt.Sprintf("{role %v name %q value %q placeholder %q disabled %v checked %v}", entry.Role, entry.Name, entry.Value, entry.Placeholder, entry.Disabled, entry.Checked))
	}
	return strings.Join(lines, " ")
}

// field is the single visible text input with a placeholder.
func (w *terminalUI) field(placeholder string) ui.AccessInfo {
	w.t.Helper()
	var found []ui.AccessInfo
	for _, entry := range w.tester.AccessAll() {
		if entry.Placeholder == placeholder && entry.Bounds.W > 0 && entry.Bounds.H > 0 {
			found = append(found, entry)
		}
	}
	if len(found) != 1 {
		w.t.Fatalf("want one input with placeholder %q, found %d; elements: %s", placeholder, len(found), w.elements())
	}
	return found[0]
}

func (w *terminalUI) clickOn(entry ui.AccessInfo) {
	x, y := center(entry.Bounds)
	w.tester.ClickAt(x, y)
	w.frame()
}

// replace types text over an input's whole value.
func (w *terminalUI) replace(placeholder, text string) {
	w.t.Helper()
	w.clickOn(w.field(placeholder))
	w.tester.Key(ui.Cmd, ui.KeyA)
	w.tester.Type(text)
	w.frame()
	if value := w.field(placeholder).Value; value != text {
		w.t.Fatalf("input %q holds %q after typing %q", placeholder, value, text)
	}
}

// nodeSettings are the terminal settings the Node holds now, read by a
// Client other than the window's.
func nodeSettings(t *testing.T, ctx context.Context, controller *client.Client) protocol.TerminalSettings {
	t.Helper()
	output, err := controller.Do(ctx, protocol.Command{Kind: "read_terminal_settings"})
	if err != nil {
		t.Fatalf("read_terminal_settings: %v", err)
	}
	value, valid := detail[protocol.TerminalSettings](output.Data)
	if !valid {
		t.Fatal("the Node returned no terminal settings")
	}
	return value
}

// terminalFontOf is the font a MyGo terminal draws with.
func terminalFontOf(terminal *myterminal.Terminal) (string, float64) {
	font := reflect.ValueOf(terminal).Elem().FieldByName("opts").FieldByName("Font")
	return font.FieldByName("Family").String(), font.FieldByName("Size").Float()
}

// settingsPage shows Settings › Terminal and waits until it holds the
// Node's current settings.
func (w *terminalUI) settingsPage(setup *client.Client) protocol.TerminalSettings {
	w.t.Helper()
	app := w.runtime.App
	app.Page = shell.SettingsPage
	app.Settings.Open(settings.Terminal)
	var current protocol.TerminalSettings
	w.wait("Settings › Terminal holds the Node's settings", func() bool {
		current = nodeSettings(w.t, w.ctx, setup)
		state := &w.runtime.terminalSettings
		return app.Settings.SaveTerminal != nil && !state.reading && !app.Settings.Host.Pending && app.Settings.Host.Revision == current.Revision &&
			w.tester.HasText("Global environment variables")
	})
	return current
}

// showTerminal brings a terminal's tab forward beside the conversation and
// gives its screen the focus.
func (w *terminalUI) showTerminal(session protocol.Session, id string) {
	w.t.Helper()
	app := w.runtime.App
	if app.Page != shell.ConversationPage {
		app.Navigate(shell.ConversationPage)
		w.runtime.OpenSession(session.ID.String())
	}
	app.PanelOpen = true
	app.Panel.Show(panel.Tab{Kind: panel.Terminal, ID: id})
	w.runtime.terminalInventory().focus = id
	var rest float32
	w.wait("terminal "+id+" shows with the focus", func() bool {
		state := w.runtime.packages.terminals[id]
		found := w.buttons("Create terminal")
		if state == nil || state.terminal == nil || w.runtime.terminalInventory().focus == id || len(found) != 1 {
			return false
		}
		previous := rest
		rest = found[0].Bounds.X
		return rest == previous && rest+found[0].Bounds.W <= 1440
	})
}

// paste sends ⌘V to the focused terminal with text on the clipboard.
func (w *terminalUI) paste(text string) {
	w.tester.SetClipboard(text)
	w.tester.Key(ui.Cmd, ui.KeyV)
	w.frame()
}

// terminalFindAndSettings continues TestLiveTerminalIntegration on the
// terminal first, which the window shows in its tab: the find bar
// (PANEL-054), Settings › Terminal's variable editing and deletion, empty
// state, shell select and save outcomes (STERM-003 to STERM-007), and the
// window's own paste protection and font (STERM-008 to STERM-010). It
// closes the window it is given and the one it opens.
func terminalFindAndSettings(t *testing.T, ctx context.Context, window *terminalUI, owner *node.Node, profile string, session protocol.Session, setup *client.Client, first string) {
	const name = "B10_ACCEPTANCE_VARIABLE"
	defer func() { window.close() }()
	app := window.runtime.App
	attached := func() *liveTerminal {
		t.Helper()
		state := window.runtime.packages.terminals[first]
		if state == nil || state.terminal == nil {
			t.Fatalf("terminal %s is not attached to the window", first)
		}
		return state
	}

	// (10) Find: ⌘F with the focus in the terminal opens the find bar; the
	// query's matches, which scrolled off the screen, are counted and one is
	// selected; Enter goes to the next; the toolbar button and the native
	// menu's command open the same bar.
	window.showTerminal(session, first)
	window.run("find fixture", first, "for i in 1 2 3; do echo b10find-$((6+1)); done; seq 1 120; echo b10done-$((2*4))", "b10done-8")
	// The terminal's text includes its scrollback: the matches are there
	// once more lines follow them than the screen has rows.
	history := window.screen(first)
	_, rows := attached().terminal.Size()
	if below := strings.Count(history[strings.LastIndex(history, "b10find-7"):], "\n"); strings.Count(history, "b10find-7") != 3 || below <= rows {
		t.Fatalf("the find fixture did not scroll off the %d-row screen (%d lines follow it): %q", rows, below, history)
	}
	if len(window.named(ui.RoleTextField, "Find")) != 0 || attached().find {
		t.Fatalf("the find bar is open before ⌘F: %s", window.elements())
	}
	window.tester.Key(ui.Cmd, ui.KeyF)
	window.wait("⌘F opens the terminal's find bar", func() bool {
		return attached().find && len(window.named(ui.RoleTextField, "Find")) == 1
	})
	// MyGo's find bar names its own field "Find".
	if bar := window.one(ui.RoleTextField, "Find"); !bar.Focused || !bar.Search {
		t.Fatalf("the find bar's field is not the focused search field: %+v", bar)
	}
	window.tester.Type("b10find-7")
	// The bar and the terminal agree on the selected match through every
	// step, round both ends of the three.
	selects := func(label string, selected int) {
		t.Helper()
		window.wait(label, func() bool {
			status := attached().terminal.SearchStatus()
			return status.Text == "b10find-7" && status.Total == 3 && status.Selected == selected && !status.Searching && window.tester.HasText(fmt.Sprintf("%d of 3", selected))
		})
		for range 5 {
			window.frame()
		}
		if status := attached().terminal.SearchStatus(); status.Selected != selected || !window.tester.HasText(fmt.Sprintf("%d of 3", selected)) {
			t.Fatalf("%s: the selection moved without input: %+v; visible text: %q", label, status, window.tester.Texts())
		}
	}
	selects("the scrollback's matches are counted and the newest is selected", 1)
	window.tester.Key(0, ui.KeyEnter)
	selects("Enter selects the second match", 2)
	window.tester.Key(0, ui.KeyEnter)
	selects("Enter selects the third match", 3)
	window.tester.Key(0, ui.KeyEnter)
	selects("Enter goes round to the first match", 1)
	window.tester.Key(ui.Shift, ui.KeyEnter)
	selects("Shift+Enter goes back round to the third match", 3)
	window.clickOn(window.one(ui.RoleButton, "Previous"))
	selects("Previous selects the second match", 2)
	window.clickOn(window.one(ui.RoleButton, "Next"))
	selects("Next selects the third match", 3)
	window.tester.Key(0, ui.KeyEscape)
	window.wait("Escape closes the find bar and ends the search", func() bool {
		return !attached().find && attached().terminal.SearchStatus().Text == "" && len(window.named(ui.RoleTextField, "Find")) == 0
	})
	window.clickOn(window.one(ui.RoleButton, "Search terminal"))
	window.wait("the toolbar's Find opens the find bar", func() bool {
		return attached().find && attached().terminal.SearchStatus().Total == 3 && len(window.named(ui.RoleTextField, "Find")) == 1
	})
	window.tester.Key(0, ui.KeyEscape)
	window.wait("the find bar closes again", func() bool { return !attached().find })
	window.runtime.terminalInventory().focus = first
	window.wait("the terminal takes the focus back", func() bool { return window.runtime.terminalInventory().focus == "" })
	app.Do("conversation.find")
	window.wait("the native menu's Find opens the terminal's find bar", func() bool {
		return attached().find && attached().terminal.SearchStatus().Total == 3 && len(window.named(ui.RoleTextField, "Find")) == 1
	})
	window.tester.Key(0, ui.KeyEscape)
	window.wait("the find bar closes a third time", func() bool { return !attached().find })
	// KEY-013: from the conversation's input ⌘F opens the conversation's
	// find bar instead.
	window.clickOn(window.one(ui.RoleTextField, "Describe what you want to accomplish…"))
	window.tester.Key(ui.Cmd, ui.KeyF)
	window.wait("⌘F in the conversation's input opens the conversation's find bar", func() bool {
		return app.Messages.SearchSession() == session.ID.String() && !attached().find
	})
	// The session has no turn, so the bar has nothing to draw; it is closed
	// as its Done does.
	app.Messages.CloseSearch()
	window.frame()
	t.Log("step 10 passed: ⌘F, the toolbar and the native menu's command opened the find bar; 3 scrollback matches were counted and stepped through round both ends, the bar's count following the terminal's selection; ⌘F from the conversation's input opened the conversation's find instead")

	// (11) Paste protection is on in a new profile: a paste of several
	// lines asks first, Cancel sends nothing and Paste sends the lines.
	if !app.Settings.PasteProtection || !window.runtime.terminalInventory().display.PasteProtection {
		t.Fatalf("paste protection is off in a new profile: %+v", window.runtime.terminalInventory().display)
	}
	const guarded = "echo b10guard-$((1+1))-a\necho b10guard-$((2+2))-b\n"
	window.showTerminal(session, first)
	window.paste(guarded)
	window.wait("a multi-line paste asks first", func() bool {
		return window.runtime.terminalInventory().paste != nil && window.tester.HasText("Paste multiple lines") && window.tester.HasText("Multiple lines may execute commands immediately")
	})
	window.clickOn(window.one(ui.RoleButton, "Cancel"))
	window.wait("Cancel drops the paste", func() bool {
		return window.runtime.terminalInventory().paste == nil && !window.tester.HasText("Paste multiple lines")
	})
	window.run("a command after the cancelled paste", first, "echo b10after-$((5+5))", "b10after-10")
	if screen := window.screen(first) + window.mirror(first); strings.Contains(screen, "b10guard") {
		t.Fatalf("the cancelled paste reached the shell: %q", screen)
	}
	window.paste(guarded)
	window.wait("the paste asks again", func() bool { return window.tester.HasText("Paste multiple lines") })
	window.clickOn(window.one(ui.RoleButton, "Paste"))
	// A shell that asks for bracketed pastes holds the lines at its prompt
	// until Enter; either way both have reached it.
	window.wait("Paste sends the lines to the shell", func() bool {
		screen := window.screen(first)
		return window.runtime.terminalInventory().paste == nil && strings.Contains(screen, "b10guard-$((1+1))-a") && strings.Contains(screen, "b10guard-$((2+2))-b")
	})
	window.tester.Key(0, ui.KeyEnter)
	window.wait("the shell runs the pasted lines", func() bool {
		screen := window.screen(first)
		return strings.Contains(screen, "b10guard-2-a") && strings.Contains(screen, "b10guard-4-b")
	})
	window.showTerminal(session, first)
	window.paste("echo b10single-$((3*3))")
	window.tester.Key(0, ui.KeyEnter)
	window.wait("a single-line paste goes without asking", func() bool {
		if window.runtime.terminalInventory().paste != nil {
			t.Fatal("a single-line paste asked for confirmation")
		}
		return strings.Contains(window.screen(first), "b10single-9")
	})
	t.Log("step 11 passed: with paste protection on, a multi-line paste waited for the dialog; Cancel sent nothing, Paste sent both lines, one line went at once")

	// (12) Edit: the row's dialog opens prefilled; while the save is on its
	// way the controls are locked; the edited value is the Node's.
	before := window.settingsPage(setup)
	if before.Environment[name] != "b10value" || !window.tester.HasText(name) {
		t.Fatalf("the variable to edit is not the Node's: %+v", before)
	}
	releaseSave := window.gate.hold("save_terminal_settings")
	defer releaseSave()
	window.clickOn(window.one(ui.RoleButton, "Edit"))
	window.wait("the edit dialog", func() bool { return window.tester.HasText("Add environment variable") })
	if key, value := window.field("e.g. LANG").Value, window.field("Enter value").Value; key != name || value != "b10value" {
		t.Fatalf("the edit dialog is not prefilled: %q = %q", key, value)
	}
	window.replace("Enter value", "b10edited")
	window.click("Save")
	window.wait("the save is pending", func() bool {
		return app.Settings.Host.Pending && !window.tester.HasText("Add environment variable")
	})
	for range 5 {
		window.frame()
	}
	for _, control := range []struct {
		role ui.Role
		name string
	}{{ui.RoleButton, "Add"}, {ui.RoleButton, "Edit"}, {ui.RoleButton, "Delete"}} {
		if entry := window.one(control.role, control.name); !entry.Disabled {
			t.Fatalf("%q takes input while the save is pending: %+v", control.name, entry)
		}
	}
	if chooser := window.one(ui.RoleMenuButton, "Default shell"); !chooser.Disabled {
		t.Fatalf("the shell select takes input while the save is pending: %+v", chooser)
	}
	if held := nodeSettings(t, ctx, setup); held.Revision != before.Revision || held.Environment[name] != "b10value" {
		t.Fatalf("the Node changed while the save was held: %+v", held)
	}
	releaseSave()
	window.wait("the Node saves the edited value", func() bool {
		return !app.Settings.Host.Pending && app.Settings.Host.Revision == before.Revision+1 && window.tester.HasText("b10edited")
	})
	edited := nodeSettings(t, ctx, setup)
	if edited.Revision != before.Revision+1 || edited.Environment[name] != "b10edited" || len(edited.Environment) != len(before.Environment) {
		t.Fatalf("the Node's settings after the edit: %+v", edited)
	}
	if entry := window.one(ui.RoleButton, "Add"); entry.Disabled {
		t.Fatal("the controls stay locked after the save")
	}
	for _, toast := range []string{"Terminal settings changed; your edits are kept", "Could not read or save terminal settings", "Save result unconfirmed; reload and review"} {
		if window.tester.HasText(toast) {
			t.Fatalf("a successful save toasted %q", toast)
		}
	}
	t.Log("step 12 passed: Edit opened prefilled, Pending locked Add, Edit, Delete and the shell select, and the Node holds the edited value at the next revision")

	// (13) Shell select: a shell from the Node's list is saved as the
	// Node's shell.
	login := filepath.Base(os.Getenv("SHELL"))
	titles := map[string]int{}
	for _, choice := range app.Settings.Shells {
		titles[filepath.Base(choice.Path)]++
	}
	var chosen viewmodel.Shell
	for _, base := range []string{"bash", "zsh", "ksh", "dash"} {
		index := slices.IndexFunc(app.Settings.Shells, func(choice viewmodel.Shell) bool { return filepath.Base(choice.Path) == base })
		if index > 0 && base != login && titles[base] == 1 && chosen.Path == "" {
			chosen = app.Settings.Shells[index]
		}
	}
	if chosen.Path == "" {
		t.Fatalf("the Node offers no shell other than the login shell %q to choose: %+v", login, app.Settings.Shells)
	}
	if edited.Shell != "" || app.Settings.Host.Draft.Shell != 0 {
		t.Fatalf("the shell is not the host default before the choice: %+v, page %d", edited, app.Settings.Host.Draft.Shell)
	}
	if chooser := window.one(ui.RoleMenuButton, "Default shell"); chooser.Disabled {
		t.Fatal("the shell select stays locked after the save")
	} else {
		window.clickOn(chooser)
	}
	window.wait("the shell select's options", func() bool {
		return window.tester.HasText("Host default") && window.tester.HasText(filepath.Base(chosen.Path))
	})
	window.click(filepath.Base(chosen.Path))
	var selected protocol.TerminalSettings
	window.wait("the Node saves the chosen shell", func() bool {
		selected = nodeSettings(t, ctx, setup)
		return selected.Shell == chosen.Path && !app.Settings.Host.Pending && app.Settings.Host.Revision == selected.Revision
	})
	if !slices.Equal(selected.ShellArguments, chosen.Args) && len(selected.ShellArguments)+len(chosen.Args) != 0 || selected.Environment[name] != "b10edited" {
		t.Fatalf("the Node's settings after the shell choice: %+v, chosen %+v", selected, chosen)
	}
	if picked := app.Settings.Shells[app.Settings.Host.Draft.Shell]; picked.Path != chosen.Path {
		t.Fatalf("the page shows %+v, not the chosen shell %+v", picked, chosen)
	}
	t.Log("step 13 passed: the select offered the Node's shells and the Node saved " + chosen.Path)

	// (14) The window's own preferences: paste protection off, the font
	// size slider bounded at 8 and 32, and a font family; all written to
	// the profile's preferences.
	state := attached()
	columns, _ := state.terminal.Size()
	family, size := terminalFontOf(state.terminal)
	t.Logf("terminal font before the change: %q %v, %d columns", family, size, columns)
	guard := window.one(ui.RoleSwitch, "Multiline paste protection")
	if !guard.Checked {
		t.Fatalf("the paste protection switch is off: %+v", guard)
	}
	window.clickOn(guard)
	window.wait("the switch turns paste protection off", func() bool {
		return !app.Settings.PasteProtection && !window.one(ui.RoleSwitch, "Multiline paste protection").Checked
	})
	slider := window.one(ui.RoleSlider, "Font size")
	window.clickOn(slider)
	for _, bound := range []struct {
		key  ui.Key
		past ui.Key
		want float64
	}{{ui.KeyEnd, ui.KeyRight, 32}, {ui.KeyHome, ui.KeyLeft, 8}} {
		window.tester.Key(0, bound.key)
		window.frame()
		window.tester.Key(0, bound.past)
		window.frame()
		if app.Settings.FontSize != bound.want {
			t.Fatalf("the font size slider stops at %v, want %v: %+v", app.Settings.FontSize, bound.want, window.one(ui.RoleSlider, "Font size"))
		}
	}
	for app.Settings.FontSize < 20 {
		previous := app.Settings.FontSize
		window.tester.Key(0, ui.KeyRight)
		window.frame()
		if app.Settings.FontSize <= previous {
			t.Fatalf("the font size slider does not move past %v", previous)
		}
	}
	if app.Settings.FontSize != 20 || !window.tester.HasText("20") {
		t.Fatalf("the font size is %v, want 20", app.Settings.FontSize)
	}
	pickOption(window, "Font", "Courier New")
	stored := func() (terminal struct {
		FontSize        uint8  `json:"font_size"`
		FontFamily      string `json:"font_family"`
		PasteProtection bool   `json:"paste_protection"`
	}, found bool) {
		content, err := os.ReadFile(filepath.Join(profile, "desktop", "preferences.json"))
		var file map[string]json.RawMessage
		if err != nil || json.Unmarshal(content, &file) != nil || file["terminal"] == nil {
			return terminal, false
		}
		return terminal, json.Unmarshal(file["terminal"], &terminal) == nil
	}
	window.wait("the preferences file holds the terminal preferences", func() bool {
		terminal, found := stored()
		return found && terminal.FontSize == 20 && terminal.FontFamily == "Courier New" && !terminal.PasteProtection
	})
	t.Log("step 14 passed: the switch, the slider (8 to 32) and the font field changed the preferences, written to desktop/preferences.json")

	// (15) The terminal draws with that font, a multi-line paste goes
	// without asking, and a new terminal runs the chosen shell with the
	// edited variable.
	window.showTerminal(session, first)
	window.wait("the terminal draws with the chosen font", func() bool {
		family, size := terminalFontOf(attached().terminal)
		narrowed, _ := attached().terminal.Size()
		return family == "Courier New" && size == 20 && narrowed < columns
	})
	narrowed, _ := attached().terminal.Size()
	if viewport := window.info(first); viewport.ID.String() != first {
		t.Fatalf("the Node lost the terminal: %+v", viewport)
	}
	window.paste("echo b10free-$((3+3))-a\necho b10free-$((4+4))-b\n")
	window.wait("a multi-line paste goes without asking", func() bool {
		if window.runtime.terminalInventory().paste != nil || window.tester.HasText("Paste multiple lines") {
			t.Fatal("a multi-line paste asked for confirmation with paste protection off")
		}
		screen := window.screen(first)
		return strings.Contains(screen, "b10free-$((3+3))-a") && strings.Contains(screen, "b10free-$((4+4))-b")
	})
	window.tester.Key(0, ui.KeyEnter)
	window.wait("the shell runs the unasked paste", func() bool {
		screen := window.screen(first)
		return strings.Contains(screen, "b10free-6-a") && strings.Contains(screen, "b10free-8-b")
	})
	window.click("Create terminal")
	window.wait("New terminal creates a Node terminal", func() bool {
		tabs := window.terminalTabs()
		return len(tabs) == 2 && window.status(tabs[1]) == "running"
	})
	fresh := window.terminalTabs()[1]
	window.wait("the new terminal attaches", func() bool {
		state := window.runtime.packages.terminals[fresh]
		return state != nil && state.terminal != nil && window.runtime.terminalInventory().focus != fresh
	})
	if family, size := terminalFontOf(window.runtime.packages.terminals[fresh].terminal); family != "Courier New" || size != 20 {
		t.Fatalf("the new terminal draws with %q %v", family, size)
	}
	window.tester.Type("echo \"sh=$(ps -p $$ -o comm=)=$" + name + "=end\"")
	window.tester.Key(0, ui.KeyEnter)
	started := regexp.MustCompile(`sh=(-|[^\s=]*/)?` + regexp.QuoteMeta(filepath.Base(chosen.Path)) + `=b10edited=end`)
	window.wait("the new terminal runs the chosen shell with the edited variable", func() bool {
		return started.MatchString(window.screen(fresh))
	})
	window.clickOn(window.one(ui.RoleButton, "Close terminal"))
	window.wait("the new terminal closes", func() bool {
		return slices.Equal(window.terminalTabs(), []string{first}) && window.status(fresh) == "closed"
	})
	t.Logf("step 15 passed: the terminal drew with Courier New 20 (%d columns, was %d), a multi-line paste went unasked, and Node terminal %s ran %s with %s=b10edited", narrowed, columns, fresh, chosen.Path, name)

	// (16) A revision conflict keeps the draft and toasts: another
	// controller saves first, then the page deletes the variable at the
	// revision it read.
	current := window.settingsPage(setup)
	releaseRead := window.gate.hold("read_terminal_settings")
	defer releaseRead()
	other := current
	other.Environment = map[string]string{name: "b10edited", "B10_OTHER_CONTROLLER": "other"}
	if _, err := setup.Do(ctx, protocol.Command{Kind: "save_terminal_settings", Data: other}); err != nil {
		t.Fatalf("the other controller's save: %v", err)
	}
	window.clickOn(window.one(ui.RoleButton, "Delete"))
	window.wait("the conflict toast", func() bool {
		return !app.Settings.Host.Pending && window.tester.HasText("Terminal settings changed; your edits are kept")
	})
	if window.tester.HasText("Could not read or save terminal settings") || window.tester.HasText("Save result unconfirmed; reload and review") {
		t.Fatalf("the conflict toasted another outcome: %q", window.tester.Texts())
	}
	if len(app.Settings.Host.Draft.Env) != 0 || window.tester.HasText(name) {
		t.Fatalf("the conflict replaced the draft: %+v", app.Settings.Host.Draft)
	}
	conflicted := nodeSettings(t, ctx, setup)
	if conflicted.Revision != current.Revision+1 || conflicted.Environment[name] != "b10edited" || conflicted.Environment["B10_OTHER_CONTROLLER"] != "other" {
		t.Fatalf("the conflicting save changed the Node: %+v", conflicted)
	}
	releaseRead()
	window.wait("the page reads the Node's newer settings", func() bool {
		return !window.runtime.terminalSettings.reading && !window.runtime.terminalSettings.stale && app.Settings.Host.Revision == conflicted.Revision
	})
	for range 5 {
		window.frame()
	}
	if len(app.Settings.Host.Draft.Env) != 0 || len(app.Settings.Host.Saved.Env) != 2 || app.Settings.Host.Pending {
		t.Fatalf("reading the newer settings replaced the kept draft: %+v", app.Settings.Host)
	}
	if after := nodeSettings(t, ctx, setup); after.Revision != conflicted.Revision {
		t.Fatalf("the kept draft was saved without a change: %+v", after)
	}
	t.Log("step 16 passed: a save at a stale revision toasted the conflict, changed nothing on the Node and kept the draft through the re-read")

	// (17) Delete and the empty state: the kept draft is saved by the next
	// change, the row's Delete removes the last variable, and the page
	// shows the empty state for the Node's empty environment.
	if !window.tester.HasText("No items") {
		t.Fatalf("the draft without variables does not show the empty state: %q", window.tester.Texts())
	}
	window.clickOn(window.one(ui.RoleButton, "Add"))
	window.wait("environment variable dialog", func() bool { return window.tester.HasText("Add environment variable") })
	window.replace("e.g. LANG", "B10_SECOND_VARIABLE")
	window.replace("Enter value", "second")
	window.click("Save")
	var second protocol.TerminalSettings
	window.wait("the next change saves the kept draft", func() bool {
		second = nodeSettings(t, ctx, setup)
		return second.Revision == conflicted.Revision+1 && !app.Settings.Host.Pending && app.Settings.Host.Revision == second.Revision && window.tester.HasText("B10_SECOND_VARIABLE")
	})
	if _, exists := second.Environment[name]; exists || second.Environment["B10_SECOND_VARIABLE"] != "second" || second.Shell != chosen.Path {
		t.Fatalf("the Node's settings after the kept draft was saved: %+v", second)
	}
	if window.tester.HasText("No items") {
		t.Fatal("the empty state shows beside a variable")
	}
	window.clickOn(window.one(ui.RoleButton, "Delete"))
	var emptied protocol.TerminalSettings
	window.wait("Delete removes the variable on the Node", func() bool {
		emptied = nodeSettings(t, ctx, setup)
		return emptied.Revision == second.Revision+1 && !app.Settings.Host.Pending && app.Settings.Host.Revision == emptied.Revision
	})
	if len(emptied.Environment) != 0 || len(app.Settings.Host.Draft.Env) != 0 || len(app.Settings.Host.Saved.Env) != 0 {
		t.Fatalf("variables remain after Delete: Node %+v, page %+v", emptied, app.Settings.Host)
	}
	window.wait("the empty state", func() bool {
		return window.tester.HasText("No items") && !window.tester.HasText("B10_SECOND_VARIABLE") && len(window.named(ui.RoleButton, "Delete")) == 0
	})
	t.Log("step 17 passed: Delete removed the variable on the Node and the page shows the empty state")

	// (18) A new Runtime on the same profile: the preferences, the font of
	// the reattached terminal, the deleted variables and the shell remain.
	saved := app.Prefs()
	saved.PanelTabs = slices.Clone(saved.PanelTabs)
	window.close()
	window = openTerminalWindow(t, ctx, owner, profile, session, &saved)
	app = window.runtime.App
	if app.Settings.FontSize != 20 || app.Settings.Font != "Courier New" || app.Settings.PasteProtection {
		t.Fatalf("the new Runtime's terminal preferences: size %v, font %q, paste protection %v", app.Settings.FontSize, app.Settings.Font, app.Settings.PasteProtection)
	}
	window.showTerminal(session, first)
	if family, size := terminalFontOf(attached().terminal); family != "Courier New" || size != 20 {
		t.Fatalf("the reattached terminal draws with %q %v", family, size)
	}
	if again, _ := attached().terminal.Size(); again != narrowed {
		t.Fatalf("the reattached terminal has %d columns, want %d as before the relaunch", again, narrowed)
	}
	window.paste("echo b10again-$((7+7))-a\necho b10again-$((8+8))-b\n")
	window.wait("a multi-line paste still goes without asking", func() bool {
		if window.runtime.terminalInventory().paste != nil {
			t.Fatal("paste protection came back on after the relaunch")
		}
		screen := window.screen(first)
		return strings.Contains(screen, "b10again-$((7+7))-a") && strings.Contains(screen, "b10again-$((8+8))-b")
	})
	window.tester.Key(0, ui.KeyEnter)
	window.wait("the shell runs the paste after the relaunch", func() bool {
		screen := window.screen(first)
		return strings.Contains(screen, "b10again-14-a") && strings.Contains(screen, "b10again-16-b")
	})
	relaunched := window.settingsPage(setup)
	window.wait("the relaunched page shows the empty state", func() bool { return window.tester.HasText("No items") })
	if len(relaunched.Environment) != 0 || len(app.Settings.Host.Draft.Env) != 0 || relaunched.Shell != chosen.Path || app.Settings.Shells[app.Settings.Host.Draft.Shell].Path != chosen.Path {
		t.Fatalf("the settings after the relaunch: Node %+v, page %+v", relaunched, app.Settings.Host)
	}
	if guard := window.one(ui.RoleSwitch, "Multiline paste protection"); guard.Checked {
		t.Fatal("the paste protection switch is on after the relaunch")
	}
	if value := app.Settings.Font; value != "Courier New" || !window.tester.HasText("Courier New") || !window.tester.HasText("20") {
		t.Fatalf("the page shows font %q after the relaunch", value)
	}
	t.Log("step 18 passed: a new Runtime kept paste protection off and Courier New 20 on the reattached terminal; the Node kept no variables and " + chosen.Path)
}
