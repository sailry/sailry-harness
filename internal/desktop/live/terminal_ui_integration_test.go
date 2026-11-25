//go:build integration

package live

import (
	"context"
	desktop "github.com/sailry/sailry-harness/internal/desktop/testassembly"
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
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
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
			w.t.Fatalf("%s did not settle; panel tabs: %v; workspace terminals: %v; panes: %v; Node terminals: %s; visible text: %q",
				label, w.runtime.App.Panel.Tabs, w.runtime.App.WorkspaceTerminalIDs(), w.paneTargets(), w.nodeTerminals(), w.tester.Texts())
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

// corePanes fails unless every terminal surface names a Node terminal.
func (w *terminalUI) corePanes(label string) {
	w.t.Helper()
	for _, id := range append(w.terminalTabs(), w.runtime.App.WorkspaceTerminalIDs()...) {
		if _, err := protocol.ParseTerminalID(id); err != nil || w.status(id) == "" {
			w.t.Fatalf("%s: terminal surface %q names no Node terminal: %s", label, id, w.nodeTerminals())
		}
	}
}

// dragToMain drops a sidebar row or tab onto the main column's lower half.
func (w *terminalUI) dragToMain(from ui.AccessInfo) {
	sx, sy := center(from.Bounds)
	app := w.runtime.App
	left := shell.RailWidth + app.NavWidth
	right := float32(1440)
	if app.PanelOpen {
		right -= app.PanelWidth
	}
	dx, dy := left+(right-left)/2, float32(900)
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

func (w *terminalUI) sidebarRow(name string) ui.AccessInfo {
	w.t.Helper()
	if name == "" {
		w.t.Fatal("a sidebar drag source must have a visible title")
	}
	for _, entry := range w.tester.AccessAll() {
		if entry.Name == name && entry.Bounds.X >= shell.RailWidth && entry.Bounds.X < shell.RailWidth+w.runtime.App.NavWidth && entry.Bounds.Y > shell.HeaderHeight {
			return entry
		}
	}
	w.t.Fatalf("no sidebar row %q: %q", name, w.tester.Texts())
	return ui.AccessInfo{}
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
	runtime, err := NewWithAssembly(ctx, controller, preferences.Open(filepath.Join(profile, "desktop", "preferences.json")), desktop.Full())
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

// TestLiveTerminalIntegration is the terminal acceptance against a real
// Node and real PTYs: creation by the shortcut, input and output, the starting
// folder, split panes, pane and terminal closing, reattachment by a new
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
	const sessionTitle = "Terminal conversation"
	do(protocol.Command{Kind: "rename_session", Data: protocol.CommandRenameSessionPayload{
		Session: session.ID, ExpectedRevision: session.Revision, Title: sessionTitle,
	}})

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

	// (3) The terminal starts in its session's worktree; the tab has no folder bar.
	directory := window.info(first).Directory
	if directory == nil {
		t.Fatalf("the Node reports no starting folder: %+v", window.info(first))
	}
	if resolved, err := filepath.EvalSymlinks(*directory); err != nil || resolved != folder {
		t.Fatalf("the terminal started in %q (%v), want the project folder %q", *directory, err, folder)
	}
	if path, known := window.runtime.terminalFolder(first); !known || path != *directory {
		t.Fatalf("the attached terminal's folder = %q, known %v; Node folder %q", path, known, *directory)
	}
	t.Log("step 3 passed: the terminal starts in " + *directory)

	// (4) A detail terminal cannot become a main pane. A project's New
	// terminal opens a standalone main surface, which can split with a
	// conversation and another terminal.
	tabs := window.buttons("Terminal")
	if len(tabs) == 0 {
		t.Fatalf("no terminal tab to drag; visible text: %q", window.tester.Texts())
	}
	app := window.runtime.App
	window.dragToMain(tabs[0])
	for range 5 {
		window.frame()
	}
	if len(window.paneTargets()) != 0 || len(app.WorkspaceTerminalIDs()) != 0 || !slices.Equal(window.terminalTabs(), []string{first}) || window.runtime.terminalInventory().paned[first] {
		t.Fatalf("a detail tab entered the main workspace: tabs %v, workspace %v, panes %v", window.terminalTabs(), app.WorkspaceTerminalIDs(), window.paneTargets())
	}
	projectRow := window.sidebarRow(project.Name)
	x, y := center(projectRow.Bounds)
	window.tester.RightClickAt(x, y)
	if err := window.tester.ChooseMenuItem("Create terminal"); err != nil {
		t.Fatalf("project New terminal: %v; menu %q", err, window.tester.Menu())
	}
	window.wait("the project menu opens an independent main terminal", func() bool {
		ids := app.WorkspaceTerminalIDs()
		return len(ids) == 1 && window.status(ids[0]) == "running" && app.FocusedConversation() == ""
	})
	workspaceFirst := app.WorkspaceTerminalIDs()[0]
	if app.Sidebar.Selected != session.ID.String() || app.HasPanelTab(panel.Tab{Kind: panel.Terminal, ID: workspaceFirst}) || !slices.Equal(window.terminalTabs(), []string{first}) {
		t.Fatalf("project terminal changed the conversation's detail scope: selected %q, tabs %v", app.Sidebar.Selected, window.terminalTabs())
	}
	if info := window.info(workspaceFirst); info.Worktree == nil || *info.Worktree != session.Worktree {
		t.Fatalf("the project terminal has the wrong worktree: %+v", info)
	}
	const workspaceMarker = "main-terminal-42"
	window.run("project main terminal", workspaceFirst, "echo main-terminal-$((6*7))", workspaceMarker)
	window.wait("the conversation has its sidebar title", func() bool {
		cv := app.Data.Find(session.ID.String())
		return cv != nil && cv.Title == sessionTitle
	})
	conversationRow := window.sidebarRow(sessionTitle)
	window.dragToMain(conversationRow)
	window.wait("the conversation splits beside the main terminal", func() bool {
		return window.paned(workspaceFirst) && window.runtime.terminalInventory().paned[workspaceFirst] &&
			len(window.buttons("Drag to split")) == 2 && strings.Contains(window.screen(workspaceFirst), workspaceMarker)
	})
	if targets := window.paneTargets(); !slices.Contains(targets, panes.Target{Kind: panes.Session, ID: session.ID.String()}) {
		t.Fatalf("the split does not hold the session beside the terminal: %v", targets)
	}
	// PANE-007: each pane's header shows its live title, and a rename on
	// the Node updates the session's.
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
	t.Log("step 4 also passed: the session pane's header followed two Node renames and the main terminal has its own header")
	handles := window.buttons("Drag to split")
	// The conversation was dropped below the terminal, so the first
	// header belongs to the terminal being split.
	header := handles[0]
	window.tester.ClickAt(header.Bounds.X-160, header.Bounds.Y+header.Bounds.H+60)
	window.frame()
	window.tester.Key(ui.Cmd, ui.KeyD)
	window.wait("⌘D creates a second Node terminal beside the pane", func() bool {
		ids := app.WorkspaceTerminalIDs()
		return len(ids) == 2 && slices.ContainsFunc(ids, func(id string) bool {
			return id != workspaceFirst && window.status(id) == "running" && window.paned(id)
		})
	})
	second := ""
	for _, id := range app.WorkspaceTerminalIDs() {
		if id != workspaceFirst {
			second = id
		}
	}
	if second == workspaceFirst || window.status(workspaceFirst) != "running" || !window.paned(workspaceFirst) || !slices.Equal(window.terminalTabs(), []string{first}) {
		t.Fatalf("the split disturbed another terminal: tabs %v, workspace %v, panes %v, Node %s", window.terminalTabs(), app.WorkspaceTerminalIDs(), window.paneTargets(), window.nodeTerminals())
	}
	if info := window.info(second); info.Worktree == nil || *info.Worktree != session.Worktree {
		t.Fatalf("the split terminal is not in the pane's worktree: %+v", info)
	}
	window.wait("both terminals render in panes", func() bool {
		inventory := window.runtime.terminalInventory()
		return inventory.paned[workspaceFirst] && inventory.paned[second] && !inventory.paned[first] && len(window.buttons("Drag to split")) == 3 &&
			window.runtime.packages.terminals[second] != nil && window.runtime.packages.terminals[second].terminal != nil
	})
	t.Log("step 4 passed: a detail terminal was excluded from splitting, the project terminal split with a conversation, and ⌘D created main terminal " + second)

	// (5) Closing a main terminal surface detaches it immediately. The
	// process survives and project navigation can attach it again.
	handles = window.buttons("Drag to split")
	x, y = center(window.beside(handles[0], "Close").Bounds)
	window.tester.ClickAt(x, y)
	window.wait("the main terminal surface detaches", func() bool {
		return !window.paned(workspaceFirst) && !app.HasWorkspaceTerminal(workspaceFirst) && window.runtime.packages.terminals[workspaceFirst] == nil
	})
	for range 5 {
		window.frame()
	}
	if window.status(workspaceFirst) != "running" || len(window.buttons("Cancel")) != 0 || app.HasPanelTab(panel.Tab{Kind: panel.Terminal, ID: workspaceFirst}) {
		t.Fatalf("closing a main view stopped the process or opened a detail tab: tabs %v, Node %s", window.terminalTabs(), window.nodeTerminals())
	}
	if !window.paned(second) || window.status(second) != "running" {
		t.Fatalf("closing one pane disturbed the other: panes %v, Node %s", window.paneTargets(), window.nodeTerminals())
	}
	window.runtime.showWorkspaceTerminal(workspaceFirst)
	window.wait("the project terminal reattaches in the main workspace", func() bool {
		return app.HasWorkspaceTerminal(workspaceFirst) && !window.paned(workspaceFirst) &&
			window.runtime.terminalInventory().focus != workspaceFirst && strings.Contains(window.screen(workspaceFirst), workspaceMarker)
	})
	window.tester.Type("exit")
	window.tester.Key(0, ui.KeyEnter)
	window.wait("exit removes the reattached main terminal", func() bool {
		return window.status(workspaceFirst) == "" && !app.HasWorkspaceTerminal(workspaceFirst) && window.runtime.packages.terminals[workspaceFirst] == nil
	})
	t.Log("step 5 passed: the main view closed without stopping its shell, reopened with scrollback, then left when its shell exited")

	// (6) Opening the other main terminal from activity moves it into a
	// detail tab. Closing that tab asks first and then ends its shell.
	window.runtime.showLiveTerminal(second)
	window.wait("the second terminal's tab", func() bool {
		return app.Panel.Active.ID == second && len(window.buttons("Terminal")) == 2 && !app.HasWorkspaceTerminal(second) && !window.paned(second)
	})
	var secondTab ui.AccessInfo
	window.wait("the second terminal's tab at rest", func() bool {
		found := window.buttons("Terminal")
		if len(found) != 2 {
			return false
		}
		previous := secondTab.Bounds.X
		secondTab = found[1]
		return secondTab.Bounds.X == previous && secondTab.Bounds.X+secondTab.Bounds.W <= 1440
	})
	closeX, closeY := secondTab.Bounds.X+secondTab.Bounds.W-14, secondTab.Bounds.Y+secondTab.Bounds.H/2
	window.tester.Move(closeX, closeY)
	window.frame()
	window.tester.ClickAt(closeX, closeY)
	window.wait("terminal close confirmation", func() bool { return len(window.buttons("Cancel")) == 1 })
	if window.status(second) != "running" || !slices.Contains(window.terminalTabs(), second) || app.HasWorkspaceTerminal(second) || window.paned(second) {
		t.Fatal("the terminal closed before confirmation")
	}
	confirmClose := window.buttons("Close")
	cx, cy := center(confirmClose[len(confirmClose)-1].Bounds)
	window.tester.ClickAt(cx, cy)
	window.wait("the confirmed close ends the Node terminal", func() bool {
		return window.status(second) == "" && !slices.Contains(window.terminalTabs(), second) && !window.paned(second)
	})
	if window.runtime.packages.terminals[second] != nil {
		t.Fatal("the closed terminal is still attached to the window")
	}
	if window.status(first) != "running" || !slices.Equal(window.terminalTabs(), []string{first}) {
		t.Fatalf("closing one terminal disturbed the other: tabs %v, Node %s", window.terminalTabs(), window.nodeTerminals())
	}
	t.Log("step 6 passed: activity moved the main terminal exclusively to a detail tab, whose confirmed close ended Node terminal " + second)

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
	// Reattaching starts none, and the ended terminal stays out of the inventory.
	if window.status(first) != "running" || window.status(second) != "" || len(window.controller.Snapshot().Terminals) != 1 {
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
	// The strip's + menu creates a terminal in the selected conversation.
	var add ui.AccessInfo
	window.wait("the panel add button at rest", func() bool {
		for _, entry := range window.tester.AccessAll() {
			if entry.Role != ui.RoleMenuButton || entry.Name != "Details" || entry.Bounds.X < 1440-app.PanelWidth || entry.Bounds.Y+entry.Bounds.H > shell.HeaderHeight {
				continue
			}
			previous := add.Bounds.X
			add = entry
			return add.Bounds.X == previous && add.Bounds.X+add.Bounds.W <= 1440
		}
		return false
	})
	addX, addY := center(add.Bounds)
	window.tester.ClickAt(addX, addY)
	var createTerminal ui.AccessInfo
	window.wait("the panel menu offers Terminal", func() bool {
		for _, entry := range window.tester.AccessAll() {
			if (entry.Role == ui.RoleMenuItem || entry.Role == ui.RoleMenuItemCheckBox || entry.Role == ui.RoleMenuItemRadio) && entry.Name == "Terminal" {
				createTerminal = entry
				return true
			}
		}
		return false
	})
	createX, createY := center(createTerminal.Bounds)
	window.tester.ClickAt(createX, createY)
	window.wait("the panel menu creates a Node terminal", func() bool {
		tabs := window.terminalTabs()
		return len(tabs) == 2 && window.status(tabs[1]) == "running"
	})
	third := window.terminalTabs()[1]
	window.run("terminal with the saved variable", third, "echo got-${"+name+"}-end", "got-"+value+"-end")
	t.Log("step 8 passed: the page read the Node's settings and shells, saved " + name + " and Node terminal " + third + " saw it")

	// (9) The same exit handling removes a detail tab. It never moves
	// that terminal into the main workspace.
	if window.paned(third) || app.HasWorkspaceTerminal(third) {
		t.Fatal("a detail terminal joined the main workspace")
	}
	window.tester.Type("exit")
	window.tester.Key(0, ui.KeyEnter)
	window.wait("exit closes the terminal and its detail tab", func() bool {
		return window.status(third) == "" && !slices.Contains(window.terminalTabs(), third) && !app.HasWorkspaceTerminal(third) && !window.paned(third)
	})
	if window.runtime.packages.terminals[third] != nil {
		t.Fatal("the exited terminal is still attached to the window")
	}
	if window.status(first) != "running" || !slices.Equal(window.terminalTabs(), []string{first}) {
		t.Fatalf("the exit disturbed the other terminal: tabs %v, Node %s", window.terminalTabs(), window.nodeTerminals())
	}
	window.corePanes("after exit")
	t.Log("step 9 passed: exit closed Node terminal " + third + " and removed its detail tab")

	// (9b) Terminals are tabs of their conversation's panel. Terminal opens
	// another terminal every time, each with its own shell; another
	// conversation shows none of them while they keep running, and coming
	// back shows them again. Closing a tab asks, then ends that shell alone.
	app.Panel.Open(panel.Terminal)
	window.wait("Terminal opens a second terminal tab", func() bool {
		tabs := window.terminalTabs()
		return len(tabs) == 2 && tabs[0] == first && window.status(tabs[1]) == "running" && len(window.buttons("Terminal")) == 2
	})
	fourth := window.terminalTabs()[1]
	if app.Panel.Active != (panel.Tab{Kind: panel.Terminal, ID: fourth}) {
		t.Fatalf("the new terminal is not shown: %v", app.Panel.Active)
	}
	window.run("second tab", fourth, "echo tab-two-$((6*7))", "tab-two-42")
	window.runtime.showLiveTerminal(first)
	window.run("first tab", first, "echo tab-one-$((5*5))", "tab-one-25")
	if strings.Contains(window.screen(first), "tab-two-42") || strings.Contains(window.screen(fourth), "tab-one-25") ||
		strings.Contains(window.mirror(first), "tab-two-42") || strings.Contains(window.mirror(fourth), "tab-one-25") {
		t.Fatalf("the two terminals share input or output:\nfirst %q\nsecond %q", window.screen(first), window.screen(fourth))
	}
	otherOutput := do(protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}})
	other, valid := detail[protocol.Session](otherOutput.Data)
	if !valid {
		t.Fatal("the second session was not returned")
	}
	window.wait("another conversation shows no terminal of this one", func() bool {
		window.runtime.OpenSession(other.ID.String())
		return app.Sidebar.Selected == other.ID.String() && len(app.Panel.Tabs) == 0 && !app.PanelOpen && len(window.buttons("Terminal")) == 0
	})
	// Hidden, both keep running and stay attached to the window.
	for range 10 {
		window.frame()
	}
	for _, id := range []string{first, fourth} {
		if window.status(id) != "running" || window.runtime.packages.terminals[id] == nil || !app.HasPanelTab(panel.Tab{Kind: panel.Terminal, ID: id}) {
			t.Fatalf("hidden terminal %s did not keep running: Node %s", id, window.nodeTerminals())
		}
	}
	app.PanelOpen = true
	app.Panel.Open(panel.Terminal)
	window.wait("the second conversation's own terminal", func() bool {
		tabs := window.terminalTabs()
		return len(tabs) == 1 && tabs[0] != first && tabs[0] != fourth && window.status(tabs[0]) == "running"
	})
	fifth := window.terminalTabs()[0]
	window.wait("coming back shows the conversation's terminals", func() bool {
		window.runtime.OpenSession(session.ID.String())
		return app.Sidebar.Selected == session.ID.String() && slices.Equal(window.terminalTabs(), []string{first, fourth}) && app.PanelOpen &&
			app.Panel.Active == (panel.Tab{Kind: panel.Terminal, ID: first}) && len(window.buttons("Terminal")) == 2
	})
	if !strings.Contains(window.screen(first), "tab-one-25") || !strings.Contains(window.screen(fourth), "tab-two-42") {
		t.Fatal("the terminals lost their screens while hidden")
	}
	// The second tab's close button: the confirmation, then the Node ends
	// that shell and the tab leaves.
	// The panel slides back in: the tab is ready once it has come to rest.
	var strip ui.AccessInfo
	window.wait("the second terminal's tab at rest", func() bool {
		found := window.buttons("Terminal")
		if len(found) != 2 {
			return false
		}
		previous := strip.Bounds.X
		strip = found[1]
		return strip.Bounds.X == previous && strip.Bounds.X+strip.Bounds.W <= 1440
	})
	closeX, closeY = strip.Bounds.X+strip.Bounds.W-14, strip.Bounds.Y+strip.Bounds.H/2
	window.tester.Move(closeX, closeY)
	window.frame()
	window.tester.ClickAt(closeX, closeY)
	window.wait("tab close confirmation", func() bool { return len(window.buttons("Cancel")) == 1 })
	if window.status(fourth) != "running" || len(window.terminalTabs()) != 2 {
		t.Fatalf("the tab closed before it was confirmed: %s", window.nodeTerminals())
	}
	confirm := window.buttons("Close")
	cx, cy = center(confirm[len(confirm)-1].Bounds)
	window.tester.ClickAt(cx, cy)
	window.wait("closing the tab ends its Node terminal", func() bool {
		return window.status(fourth) == "" && slices.Equal(window.terminalTabs(), []string{first}) && window.runtime.packages.terminals[fourth] == nil
	})
	if window.status(first) != "running" || window.status(fifth) != "running" {
		t.Fatalf("closing one tab disturbed another terminal: %s", window.nodeTerminals())
	}
	// Archiving the other conversation ends its terminal and drops its tabs.
	var current protocol.Session
	if err := setup.Refresh(ctx); err != nil {
		t.Fatal(err)
	}
	for _, candidate := range setup.Snapshot().Sessions {
		if candidate.ID == other.ID {
			current = candidate
		}
	}
	do(protocol.Command{Kind: "set_session_archived", Data: protocol.CommandSetSessionArchivedPayload{Session: other.ID, ExpectedRevision: current.Revision, Archived: true}})
	window.wait("archiving a conversation ends its terminals", func() bool {
		return window.status(fifth) == "" && len(app.ConversationTabs(other.ID.String())) == 0 && !app.HasPanelTab(panel.Tab{Kind: panel.Terminal, ID: fifth})
	})
	if window.status(first) != "running" || !slices.Equal(window.terminalTabs(), []string{first}) {
		t.Fatalf("the archive disturbed the shown conversation: tabs %v, Node %s", window.terminalTabs(), window.nodeTerminals())
	}
	t.Log("step 9b passed: two terminals ran as two tabs with separate shells, stayed running while another conversation was shown, came back, one closed by its tab after confirmation, and an archived conversation's terminal ended")

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
