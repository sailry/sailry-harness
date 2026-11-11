//go:build integration

package live

import (
	"context"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// terminalUI drives one window of the real App over a real Node.
type terminalUI struct {
	t          *testing.T
	ctx        context.Context
	controller *client.Client
	runtime    *Runtime
	tester     *ui.Tester
	gate       *terminalGate
	closed     bool
}

func (w *terminalUI) frame() {
	w.tester.Advance(16 * time.Millisecond)
	w.tester.Frame()
}

// wait builds frames until ready, for at most 15 seconds.
func (w *terminalUI) wait(label string, ready func() bool) {
	w.t.Helper()
	deadline := time.NewTimer(15 * time.Second)
	defer deadline.Stop()
	ticker := time.NewTicker(10 * time.Millisecond)
	defer ticker.Stop()
	for {
		w.frame()
		if ready() {
			return
		}
		select {
		case <-w.ctx.Done():
			w.t.Fatalf("%s: %v; visible text: %q", label, w.ctx.Err(), w.tester.Texts())
		case <-deadline.C:
			w.t.Fatalf("%s did not settle; panel tabs: %v; panes: %v; Node terminals: %s; visible text: %q",
				label, w.runtime.App.Panel.Tabs, w.paneTargets(), w.nodeTerminals(), w.tester.Texts())
		case <-ticker.C:
		}
	}
}

func (w *terminalUI) do(command protocol.Command) protocol.Output {
	w.t.Helper()
	output, err := w.controller.Do(w.ctx, command)
	if err != nil {
		w.t.Fatalf("%s: %v", command.Kind, err)
	}
	return output
}

func (w *terminalUI) nodeTerminals() string {
	var entries []string
	for _, info := range w.controller.Snapshot().Terminals {
		entries = append(entries, info.ID.String()+"="+info.Status.Kind)
	}
	return strings.Join(entries, ",")
}

// status is a Node terminal's status kind, "" when the Node no longer
// reports the terminal.
func (w *terminalUI) status(id string) string {
	for _, info := range w.controller.Snapshot().Terminals {
		if info.ID.String() == id {
			return info.Status.Kind
		}
	}
	return ""
}

func (w *terminalUI) info(id string) protocol.TerminalInfo {
	w.t.Helper()
	for _, info := range w.controller.Snapshot().Terminals {
		if info.ID.String() == id {
			return info
		}
	}
	w.t.Fatalf("the Node does not report terminal %s: %s", id, w.nodeTerminals())
	return protocol.TerminalInfo{}
}

// terminalTabs are the panel's terminal tab IDs in order.
func (w *terminalUI) terminalTabs() []string {
	var ids []string
	for _, tab := range w.runtime.App.Panel.Tabs {
		if tab.Kind == panel.Terminal {
			ids = append(ids, tab.ID)
		}
	}
	return ids
}

// paneTargets are the split workspace's panes as the window would save them.
func (w *terminalUI) paneTargets() []panes.Target {
	var targets []panes.Target
	for _, group := range w.runtime.App.Prefs().Panes {
		targets = append(targets, group.Targets()...)
	}
	return targets
}

func (w *terminalUI) paned(id string) bool {
	return slices.Contains(w.paneTargets(), panes.Target{Kind: panes.Terminal, ID: id})
}

// screen is the text the window's MyGo terminal shows for a tab.
func (w *terminalUI) screen(id string) string {
	state := w.runtime.packages.terminals[id]
	if state == nil || state.terminal == nil {
		return ""
	}
	return state.terminal.Text()
}

// mirror is the Node's own screen of a terminal.
func (w *terminalUI) mirror(id string) string {
	terminalID, err := protocol.ParseTerminalID(id)
	if err != nil {
		w.t.Fatal(err)
	}
	output, err := w.controller.Do(w.ctx, protocol.Command{Kind: "inspect_terminal", Data: protocol.CommandInspectTerminalPayload{Terminal: terminalID}})
	if err != nil {
		return ""
	}
	snapshot, valid := detail[protocol.TerminalSnapshot](output.Data)
	if !valid || snapshot.Screen.VT == nil {
		return ""
	}
	return string(snapshot.Screen.VT.Bytes)
}

// run types a command into the focused terminal id and waits until both the
// window's screen and the Node's mirror show the expected output.
func (w *terminalUI) run(label, id, command, expected string) {
	w.t.Helper()
	w.wait(label+": terminal attaches", func() bool {
		state := w.runtime.packages.terminals[id]
		return state != nil && state.terminal != nil && w.runtime.terminalInventory().focus != id
	})
	w.tester.Type(command)
	w.tester.Key(0, ui.KeyEnter)
	w.wait(label+": output reaches the screen", func() bool {
		return strings.Contains(w.screen(id), expected)
	})
	if mirror := w.mirror(id); !strings.Contains(mirror, expected) {
		w.t.Fatalf("%s: the Node mirror lacks %q: %q", label, expected, mirror)
	}
}

// buttons are the buttons named name in visual order, top to bottom and
// then left to right.
func (w *terminalUI) buttons(name string) []ui.AccessInfo {
	var found []ui.AccessInfo
	for _, entry := range w.tester.AccessAll() {
		if entry.Role == ui.RoleButton && entry.Name == name && entry.Bounds.W > 0 && entry.Bounds.H > 0 {
			found = append(found, entry)
		}
	}
	slices.SortStableFunc(found, func(a, b ui.AccessInfo) int {
		if a.Bounds.Y != b.Bounds.Y {
			return int(a.Bounds.Y - b.Bounds.Y)
		}
		return int(a.Bounds.X - b.Bounds.X)
	})
	return found
}

// corePanes fails unless every terminal tab names a Node terminal.
func (w *terminalUI) corePanes(label string) {
	w.t.Helper()
	for _, id := range w.terminalTabs() {
		if _, err := protocol.ParseTerminalID(id); err != nil || w.status(id) == "" {
			w.t.Fatalf("%s: terminal tab %q names no Node terminal: %s", label, id, w.nodeTerminals())
		}
	}
}

// dragToConversation drags a panel tab onto the lower half of the
// conversation column, where it becomes a split pane.
func (w *terminalUI) dragToConversation(tab ui.AccessInfo) {
	sx, sy := center(tab.Bounds)
	app := w.runtime.App
	left := shell.RailWidth + app.NavWidth
	dx, dy := left+(1440-app.PanelWidth-left)/2, float32(900)
	w.tester.Press(sx, sy)
	for step := 1; step <= 12; step++ {
		fraction := float32(step) / 12
		w.tester.Move(sx+(dx-sx)*fraction, sy+(dy-sy)*fraction)
		w.tester.Frame()
	}
	w.tester.Advance(300 * time.Millisecond)
	w.tester.Frame()
	w.tester.Release(dx, dy)
}

func center(bounds ui.Rect) (float32, float32) {
	return bounds.X + bounds.W/2, bounds.Y + bounds.H/2
}

// beside is the button named name on from's row to its right, as a pane
// header's close button after its drag handle.
func (w *terminalUI) beside(from ui.AccessInfo, name string) ui.AccessInfo {
	w.t.Helper()
	_, y := center(from.Bounds)
	for _, candidate := range w.buttons(name) {
		if _, cy := center(candidate.Bounds); cy > y-4 && cy < y+4 && candidate.Bounds.X > from.Bounds.X {
			return candidate
		}
	}
	w.t.Fatalf("no %q button beside %q at %+v; visible text: %q", name, from.Name, from.Bounds, w.tester.Texts())
	return ui.AccessInfo{}
}

func (w *terminalUI) click(label string) {
	w.t.Helper()
	if err := w.tester.Click(label); err != nil {
		w.t.Fatalf("click %q: %v; visible text: %q", label, err, w.tester.Texts())
	}
}

// openTerminalWindow starts a Client and a Runtime on owner, as a launch of
// the desktop does, and shows session.
func openTerminalWindow(t *testing.T, ctx context.Context, owner *node.Node, profile string, session protocol.Session, prefs *shell.Prefs) *terminalUI {
	t.Helper()
	gate := &terminalGate{Transport: pluginUITransport{Transport: link.NewLocal(owner.ID(), owner.ID(), owner), test: t}, held: map[string]chan struct{}{}}
	controller := client.New(gate)
	if err := controller.Start(ctx); err != nil {
		t.Fatal(err)
	}
	runtime, err := NewWithPreferences(ctx, controller, preferences.Open(filepath.Join(profile, "desktop", "preferences.json")))
	if err != nil {
		_ = controller.Close()
		t.Fatal(err)
	}
	if prefs != nil {
		runtime.App.SetPrefs(*prefs)
	}
	runtime.OpenSession(session.ID.String())
	runtime.App.NavOpen = true
	tester := ui.NewTester(runtime.App.View, 1440, 1000)
	tester.SetLocale("en")
	window := &terminalUI{t: t, ctx: ctx, controller: controller, runtime: runtime, tester: tester, gate: gate}
	window.wait("session workspace", func() bool {
		return runtime.App.Sidebar.Selected == session.ID.String() && runtime.workspace != nil && runtime.workspace.selected == session.Worktree
	})
	return window
}

func (w *terminalUI) close() {
	if w.closed {
		return
	}
	w.closed = true
	_ = w.runtime.Close()
	_ = w.controller.Close()
}

// TestLiveTerminalIntegration is the B10 terminal acceptance against a real
// Node and real PTYs: creation by the shortcut, input and output, the folder
// label, split panes, pane and terminal closing, reattachment by a new
// window and the Node's terminal settings. terminalFindAndSettings goes on
// with the find bar, the rest of Settings › Terminal and the terminal
// preferences of the window.
func TestLiveTerminalIntegration(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 300*time.Second)
	defer cancel()
	profile := filepath.Join(t.TempDir(), "profile")
	owner, err := node.Start(ctx, profile)
	if err != nil {
		t.Fatal(err)
	}
	defer owner.Stop(context.Background())
	setup := client.New(link.NewLocal(owner.ID(), owner.ID(), owner))
	if err := setup.Start(ctx); err != nil {
		t.Fatal(err)
	}
	defer setup.Close()
	do := func(command protocol.Command) protocol.Output {
		t.Helper()
		output, err := setup.Do(ctx, command)
		if err != nil {
			t.Fatalf("%s: %v", command.Kind, err)
		}
		return output
	}
	// The profile stays the default one: its bundled commands package is
	// enabled and declares a terminal renderer with a creation contribution.
	// Local terminals are core-owned, so that renderer must never be mounted.
	declared := false
	for _, summary := range setup.Snapshot().Plugins {
		if !summary.Enabled {
			continue
		}
		info, valid := detail[protocol.PluginInfo](do(protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: summary.Name}}).Data)
		if !valid {
			t.Fatalf("package %q was not returned", summary.Name)
		}
		if info.Extension == nil || info.Extension.Desktop == nil {
			continue
		}
		for _, renderer := range info.Extension.Desktop.Renderers {
			if renderer.Resource == "terminal" && renderer.Create != nil {
				declared = true
				t.Logf("fixture: enabled package %q declares a terminal renderer created by %q", summary.Name, *renderer.Create)
			}
		}
	}
	if !declared {
		t.Fatal("the default profile enables no package that declares a terminal renderer")
	}
	folder, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	projectOutput := do(protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Terminal workspace", Path: folder, Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"},
	}})
	project, valid := detail[protocol.Project](projectOutput.Data)
	if !valid {
		t.Fatal("registered project was not returned")
	}
	provider, model := fixtureProvider("Terminal fixture")
	secret := protocol.NewSecret("fixture-key")
	do(protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}})
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Effort = provider.ID, model.ID, model.DefaultEffort
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionFull
	sessionOutput := do(protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}})
	session, valid := detail[protocol.Session](sessionOutput.Data)
	if !valid {
		t.Fatal("created session was not returned")
	}

	window := openTerminalWindow(t, ctx, owner, profile, session, nil)
	closed := false
	defer func() {
		if !closed {
			window.close()
		}
	}()

	// (1) ⌃` creates a Node terminal whose panel tab is named by its ID.
	if tabs := window.terminalTabs(); len(tabs) != 0 || len(window.controller.Snapshot().Terminals) != 0 {
		t.Fatalf("terminals before the shortcut: tabs %v, Node %s", tabs, window.nodeTerminals())
	}
	window.tester.Key(ui.Ctrl, ui.KeyBackquote)
	window.wait("⌃` creates a Node terminal", func() bool {
		tabs := window.terminalTabs()
		return len(tabs) == 1 && window.status(tabs[0]) == "running"
	})
	first := window.terminalTabs()[0]
	if running := window.controller.Snapshot().Terminals; len(running) != 1 || running[0].ID.String() != first {
		t.Fatalf("panel tab %q does not name the one Node terminal: %s", first, window.nodeTerminals())
	}
	if !window.runtime.App.PanelOpen || window.runtime.App.Panel.Active != (panel.Tab{Kind: panel.Terminal, ID: first}) {
		t.Fatalf("the shortcut did not show the terminal tab: open %v, active %v", window.runtime.App.PanelOpen, window.runtime.App.Panel.Active)
	}
	if info := window.info(first); info.Worktree == nil || *info.Worktree != session.Worktree {
		t.Fatalf("the terminal is not in the session's worktree: %+v", info)
	}
	if window.runtime.packages.terminals[first] == nil {
		t.Fatal("the terminal tab is not a core live terminal")
	}
	window.corePanes("after ⌃`")
	t.Log("step 1 passed: ⌃` created Node terminal " + first + " and its tab carries that ID")

	// (2) Typed input reaches the shell and its output shows on the screen.
	// The marker is computed by the shell, so the echoed command line cannot
	// satisfy the check.
	const marker = "sailry-b10-42-scrollback"
	window.run("first terminal", first, "echo sailry-b10-$((40+2))-scrollback", marker)
	t.Log("step 2 passed: the shell's output is on the window's screen and in the Node mirror")

	// (3) The toolbar names the folder the terminal started in.
	directory := window.info(first).Directory
	if directory == nil {
		t.Fatalf("the Node reports no starting folder: %+v", window.info(first))
	}
	if resolved, err := filepath.EvalSymlinks(*directory); err != nil || resolved != folder {
		t.Fatalf("the terminal started in %q (%v), want the project folder %q", *directory, err, folder)
	}
	window.wait("toolbar folder label", func() bool { return slices.Contains(window.tester.Texts(), *directory) })
	t.Log("step 3 passed: the toolbar shows " + *directory)

	// (4) The tab dragged onto the conversation shows its terminal in a
	// pane; ⌘D in that pane starts a second Node terminal beside it.
	tabs := window.buttons("Terminal")
	if len(tabs) == 0 {
		t.Fatalf("no terminal tab to drag; visible text: %q", window.tester.Texts())
	}
	app := window.runtime.App
	window.dragToConversation(tabs[0])
	window.wait("terminal renders in a split pane", func() bool {
		return window.paned(first) && window.runtime.terminalInventory().paned[first] &&
			len(window.buttons("Drag to split")) == 2 && strings.Contains(window.screen(first), marker)
	})
	if targets := window.paneTargets(); !slices.Contains(targets, panes.Target{Kind: panes.Session, ID: session.ID.String()}) {
		t.Fatalf("the split does not hold the session beside the terminal: %v", targets)
	}
	// PANE-007: each pane's header shows its live title, the session's or
	// the terminal tab's, and a rename on the Node updates the session's.
	headed := func(title string) int {
		count := 0
		for _, handle := range window.buttons("Drag to split") {
			_, y := center(handle.Bounds)
			for _, text := range window.named(ui.RoleText, title) {
				if _, ty := center(text.Bounds); ty > y-14 && ty < y+14 && text.Bounds.X < handle.Bounds.X && handle.Bounds.X-text.Bounds.X < 2000 {
					count++
					break
				}
			}
		}
		return count
	}
	for _, title := range []string{"Pane title one", "Pane title two"} {
		var current protocol.Session
		for _, candidate := range setup.Snapshot().Sessions {
			if candidate.ID == session.ID {
				current = candidate
			}
		}
		do(protocol.Command{Kind: "rename_session", Data: protocol.CommandRenameSessionPayload{Session: session.ID, ExpectedRevision: current.Revision, Title: title}})
		window.wait("the session pane's header shows "+title, func() bool {
			return headed(title) == 1 && headed("Terminal") == 1 && (title == "Pane title one" || headed("Pane title one") == 0)
		})
	}
	t.Log("step 4 also passed: the session pane's header followed two Node renames and the terminal pane's header shows its tab title Terminal")
	handles := window.buttons("Drag to split")
	header := handles[len(handles)-1]
	window.tester.ClickAt(header.Bounds.X-160, header.Bounds.Y+header.Bounds.H+60)
	window.frame()
	window.tester.Key(ui.Cmd, ui.KeyD)
	window.wait("⌘D creates a second Node terminal beside the pane", func() bool {
		tabs := window.terminalTabs()
		return len(tabs) == 2 && window.status(tabs[1]) == "running" && window.paned(tabs[1])
	})
	second := window.terminalTabs()[1]
	if second == first || window.status(first) != "running" || !window.paned(first) {
		t.Fatalf("the split disturbed the first terminal: tabs %v, panes %v, Node %s", window.terminalTabs(), window.paneTargets(), window.nodeTerminals())
	}
	if info := window.info(second); info.Worktree == nil || *info.Worktree != session.Worktree {
		t.Fatalf("the split terminal is not in the pane's worktree: %+v", info)
	}
	window.wait("both terminals render in panes", func() bool {
		inventory := window.runtime.terminalInventory()
		return inventory.paned[first] && inventory.paned[second] && len(window.buttons("Drag to split")) == 3 &&
			window.runtime.packages.terminals[second] != nil && window.runtime.packages.terminals[second].terminal != nil
	})
	t.Log("step 4 passed: the terminal rendered in a pane and ⌘D created Node terminal " + second + " beside it")

	// (5) Closing the first terminal's pane keeps its Node terminal and tab.
	handles = window.buttons("Drag to split")
	lower := handles[1:]
	slices.SortFunc(lower, func(a, b ui.AccessInfo) int { return int(a.Bounds.X - b.Bounds.X) })
	x, y := center(window.beside(lower[0], "Close").Bounds)
	window.tester.ClickAt(x, y)
	window.wait("pane close confirmation", func() bool { return len(window.buttons("Cancel")) == 1 })
	x, y = center(window.beside(window.buttons("Cancel")[0], "Close").Bounds)
	window.tester.ClickAt(x, y)
	window.wait("the first terminal's pane closes", func() bool { return !window.paned(first) })
	for range 5 {
		window.frame()
	}
	if window.status(first) != "running" || !slices.Contains(window.terminalTabs(), first) {
		t.Fatalf("closing the pane ended its terminal: tabs %v, Node %s", window.terminalTabs(), window.nodeTerminals())
	}
	if !window.paned(second) || window.status(second) != "running" {
		t.Fatalf("closing one pane disturbed the other: panes %v, Node %s", window.paneTargets(), window.nodeTerminals())
	}
	app.Panel.Show(panel.Tab{Kind: panel.Terminal, ID: first})
	window.wait("the terminal shows in its tab again", func() bool {
		return !window.runtime.terminalInventory().paned[first] && strings.Contains(window.screen(first), marker)
	})
	t.Log("step 5 passed: the pane closed, Node terminal " + first + " kept running and its tab stayed")

	// (6) Close terminal ends the Node terminal and removes its tab and pane.
	app.Panel.Show(panel.Tab{Kind: panel.Terminal, ID: second})
	window.wait("the second terminal's tab", func() bool {
		return app.Panel.Active.ID == second && len(window.buttons("Close terminal")) == 1
	})
	window.click("Close terminal")
	window.wait("Close terminal ends the Node terminal", func() bool {
		status := window.status(second)
		return (status == "" || status == "closed") && !slices.Contains(window.terminalTabs(), second) && !window.paned(second)
	})
	if window.runtime.packages.terminals[second] != nil {
		t.Fatal("the closed terminal is still attached to the window")
	}
	if window.status(first) != "running" || !slices.Equal(window.terminalTabs(), []string{first}) {
		t.Fatalf("closing one terminal disturbed the other: tabs %v, Node %s", window.terminalTabs(), window.nodeTerminals())
	}
	t.Log("step 6 passed: Close terminal ended Node terminal " + second + " and removed its tab and pane")

	// (7) A new window on the same Node with the saved tabs shows the
	// terminal that kept running, with its scrollback.
	saved := app.Prefs()
	saved.PanelTabs = slices.Clone(saved.PanelTabs)
	if !slices.Contains(saved.PanelTabs, panel.Tab{Kind: panel.Terminal, ID: first}) {
		t.Fatalf("the saved panel tabs lack the terminal: %v", saved.PanelTabs)
	}
	window.close()
	closed = true
	if err := setup.Refresh(ctx); err != nil {
		t.Fatal(err)
	}
	if !slices.ContainsFunc(setup.Snapshot().Terminals, func(info protocol.TerminalInfo) bool {
		return info.ID.String() == first && info.Status.Kind == "running"
	}) {
		t.Fatalf("the terminal did not outlive its window: %+v", setup.Snapshot().Terminals)
	}
	window = openTerminalWindow(t, ctx, owner, profile, session, &saved)
	defer window.close()
	app = window.runtime.App
	window.wait("the saved tab reattaches with its scrollback", func() bool {
		return slices.Equal(window.terminalTabs(), []string{first}) && strings.Contains(window.screen(first), marker)
	})
	// The Node keeps the closed terminal listed; reattaching starts none.
	if window.status(first) != "running" || window.status(second) != "closed" || len(window.controller.Snapshot().Terminals) != 2 {
		t.Fatalf("reattaching changed the Node's terminals: %s", window.nodeTerminals())
	}
	if !app.PanelOpen || app.Panel.Active != (panel.Tab{Kind: panel.Terminal, ID: first}) {
		t.Fatalf("the saved tab is not shown: open %v, active %v", app.PanelOpen, app.Panel.Active)
	}
	t.Log("step 7 passed: a new Runtime and App reattached Node terminal " + first + " with its scrollback marker")

	// (8) Settings › Terminal reads the Node's settings and shells; a saved
	// environment variable reaches a new terminal.
	offered, valid := detail[[]protocol.TerminalShell](window.do(protocol.Command{Kind: "list_shells"}).Data)
	if !valid || len(offered) == 0 {
		t.Fatalf("the Node offers no shells: %+v", offered)
	}
	before, valid := detail[protocol.TerminalSettings](window.do(protocol.Command{Kind: "read_terminal_settings"}).Data)
	if !valid {
		t.Fatal("the Node returned no terminal settings")
	}
	// The Node reserves the SAILRY_ prefix, so the name avoids it.
	const name, value = "B10_ACCEPTANCE_VARIABLE", "b10value"
	if _, exists := before.Environment[name]; exists {
		t.Fatalf("the variable exists before it is added: %+v", before.Environment)
	}
	app.Page = shell.SettingsPage
	app.Settings.Open(settings.Terminal)
	window.wait("Settings › Terminal reads the Node", func() bool {
		return app.Settings.SaveTerminal != nil && window.tester.HasText("Global environment variables")
	})
	if app.Settings.Host.Revision != before.Revision || len(app.Settings.Host.Draft.Env) != len(before.Environment) {
		t.Fatalf("the page does not hold the Node's settings: %+v, Node %+v", app.Settings.Host, before)
	}
	for _, candidate := range offered {
		if !slices.ContainsFunc(app.Settings.Shells, func(choice viewmodel.Shell) bool {
			return choice.Path == candidate.Path && slices.Equal(choice.Args, candidate.Arguments)
		}) {
			t.Fatalf("the page lacks the Node's shell %+v: %+v", candidate, app.Settings.Shells)
		}
	}
	window.click("Add")
	window.wait("environment variable dialog", func() bool { return window.tester.HasText("Add environment variable") })
	for _, field := range []struct{ placeholder, text string }{{"e.g. LANG", name}, {"Enter value", value}} {
		index := slices.IndexFunc(window.tester.AccessAll(), func(entry ui.AccessInfo) bool { return entry.Placeholder == field.placeholder })
		if index < 0 {
			t.Fatalf("no input with placeholder %q; visible text: %q", field.placeholder, window.tester.Texts())
		}
		x, y := center(window.tester.AccessAll()[index].Bounds)
		window.tester.ClickAt(x, y)
		window.frame()
		window.tester.Type(field.text)
		window.frame()
	}
	window.click("Save")
	window.wait("the Node saves the variable", func() bool {
		return !app.Settings.Host.Pending && app.Settings.Host.Revision == before.Revision+1 && window.tester.HasText(name)
	})
	after, valid := detail[protocol.TerminalSettings](window.do(protocol.Command{Kind: "read_terminal_settings"}).Data)
	if !valid || after.Revision != before.Revision+1 || after.Environment[name] != value {
		t.Fatalf("the Node's settings after the save: %+v", after)
	}
	app.Navigate(shell.ConversationPage)
	window.runtime.OpenSession(session.ID.String())
	// The panel slides back in beside the conversation: the toolbar is
	// ready once its button has come to rest inside the window.
	var rest float32
	window.wait("terminal toolbar", func() bool {
		found := window.buttons("Create terminal")
		if len(found) != 1 {
			return false
		}
		previous := rest
		rest = found[0].Bounds.X
		return rest == previous && rest+found[0].Bounds.W <= 1440
	})
	window.click("Create terminal")
	window.wait("New terminal creates a Node terminal", func() bool {
		tabs := window.terminalTabs()
		return len(tabs) == 2 && window.status(tabs[1]) == "running"
	})
	third := window.terminalTabs()[1]
	window.run("terminal with the saved variable", third, "echo got-${"+name+"}-end", "got-"+value+"-end")
	t.Log("step 8 passed: the page read the Node's settings and shells, saved " + name + " and Node terminal " + third + " saw it")

	// (9) A shell that exits closes its terminal: typing exit in a pane
	// removes the pane and the tab, and the Node reports the terminal closed.
	tabs = window.buttons("Terminal")
	if len(tabs) != 2 {
		t.Fatalf("want the two terminal tabs to drag the second; found %d, visible text: %q", len(tabs), window.tester.Texts())
	}
	window.dragToConversation(tabs[1])
	window.wait("the third terminal renders in a split pane", func() bool {
		return window.paned(third) && window.runtime.terminalInventory().paned[third] && len(window.buttons("Drag to split")) == 2
	})
	handles = window.buttons("Drag to split")
	header = handles[len(handles)-1]
	window.tester.ClickAt(header.Bounds.X-160, header.Bounds.Y+header.Bounds.H+60)
	window.frame()
	window.tester.Type("exit")
	window.tester.Key(0, ui.KeyEnter)
	window.wait("exit closes the terminal, its tab and its pane", func() bool {
		return window.status(third) == "closed" && !slices.Contains(window.terminalTabs(), third) && !window.paned(third)
	})
	if window.runtime.packages.terminals[third] != nil {
		t.Fatal("the exited terminal is still attached to the window")
	}
	if window.status(first) != "running" || !slices.Equal(window.terminalTabs(), []string{first}) {
		t.Fatalf("the exit disturbed the other terminal: tabs %v, Node %s", window.terminalTabs(), window.nodeTerminals())
	}
	window.corePanes("after exit")
	t.Log("step 9 passed: exit closed Node terminal " + third + " and removed its tab and pane")

	terminalFindAndSettings(t, ctx, window, owner, profile, session, setup, first)
}

// fixtureProvider is an enabled provider with one model that a session can
// be created on. No request is ever sent to it.
func fixtureProvider(name string) (protocol.ConversationProvider, protocol.ConversationModel) {
	provider := protocol.DefaultConversationProvider()
	provider.ID = protocol.NewProviderID()
	provider.Name, provider.Api = name, protocol.ConversationModelApiChatCompletions
	provider.Authentication, provider.Endpoint = protocol.AuthenticationApiKey, "http://127.0.0.1:9/v1"
	provider.Enabled, provider.DefaultModel = true, "fixture"
	model := protocol.DefaultConversationModel()
	model.ID, model.Context, model.Output = "fixture", 200000, 8192
	model.Reasoning, model.DefaultEffort = true, protocol.Effort{Kind: "none"}
	model.Efforts = []protocol.Effort{model.DefaultEffort}
	provider.Models = []protocol.ConversationModel{model}
	return provider, model
}
