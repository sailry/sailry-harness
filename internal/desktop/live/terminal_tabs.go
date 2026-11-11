package live

import (
	"slices"
	"strings"

	myterminal "github.com/egoist/mygo/plugins/terminal"
	"github.com/egoist/mygo/transfer"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// terminalViewport is a terminal's size before its view measures it.
var terminalViewport = protocol.TerminalViewport{Columns: 80, Rows: 24}

// terminalState is the window's inventory of terminal tabs. A tab's ID is
// its Node terminal's ID; the root runtime owns the inventory of every Node.
type terminalState struct {
	// pending tabs wait for their terminal's first snapshot and are not
	// pruned: a command's output can arrive before the Node's update.
	pending map[string]bool
	// exited holds terminals already asked to close after their shell exited.
	exited map[string]bool
	// nodes is the Node each tab's terminal was last seen on.
	nodes map[string]protocol.NodeID
	// paned terminals showed in a split pane this frame.
	paned      map[string]bool
	appearance protocol.TerminalAppearance
	display    preferences.Terminal
	// focus is the terminal that takes the focus once its screen shows.
	focus string
	// paste is a multi-line paste waiting for confirmation.
	paste *terminalPaste
}

type terminalPaste struct {
	terminal string
	data     transfer.Data
}

func (r *Runtime) terminalInventory() *terminalState {
	root := r.localRuntime()
	if root.terminals == nil {
		root.terminals = &terminalState{
			pending: map[string]bool{}, nodes: map[string]protocol.NodeID{}, paned: map[string]bool{},
			appearance: protocol.TerminalAppearance{ColorScheme: protocol.TerminalColorSchemeLight}, display: root.terminalDisplay(),
		}
	}
	return root.terminals
}

// terminalDisplay is the one place the terminal display preferences are
// read: the font's size and family and paste protection.
func (r *Runtime) terminalDisplay() preferences.Terminal {
	if r.Preferences == nil {
		return preferences.Terminal{PasteProtection: true}
	}
	data := &r.Preferences.Data
	return preferences.Terminal{
		FontSize: uint8(data.TerminalFontSize()), FontFamily: data.TerminalFontFamily(), PasteProtection: data.TerminalPasteProtection(),
	}
}

func terminalFont(display preferences.Terminal) myterminal.Font {
	return myterminal.Font{Family: display.FontFamily, Size: float32(display.FontSize)}
}

func (r *Runtime) initTerminalActions() {
	r.App.Sidebar.Actions.NewTerminal = func(project *viewmodel.Project) { r.newProjectTerminal(project, "") }
}

// createLiveTerminal starts a shell in worktree on owner's Node and shows
// its tab at once: the terminal's ID is the request's, known before the Node
// answers. A failed creation takes the tab back.
func (r *Runtime) createLiveTerminal(owner *Runtime, worktree protocol.WorktreeID) (string, bool) {
	root := r.localRuntime()
	inventory := root.terminalInventory()
	if owner == nil || root.packages == nil || root.packages.terminals == nil || !owner.controller.Connected() || worktree == (protocol.WorktreeID{}) {
		root.Report(pluginNativeFault(protocol.ErrorCodeUnavailable, "terminal creation requires an available workspace"))
		return "", false
	}
	request := owner.controller.Prepare(protocol.Command{Kind: "create_terminal", Data: protocol.TerminalLaunch{
		Worktree: worktree, Access: protocol.ProcessAccessUnrestricted, Viewport: terminalViewport, Appearance: inventory.appearance,
	}})
	id := protocol.TerminalID(request.ID).String()
	state := &liveTerminal{owner: owner, reading: true}
	root.packages.terminals[id] = state
	inventory.pending[id], inventory.focus = true, id
	inventory.nodes[id] = owner.controller.Target()
	root.App.Panel.Show(panel.Tab{Kind: panel.Terminal, ID: id})
	if !owner.async(func() {
		output, err := owner.controller.Execute(owner.ctx, request)
		root.enqueue(func(c *ui.Context) {
			info, valid := detail[protocol.TerminalInfo](output.Data)
			if err == nil && !valid {
				err = pluginNativeFault(protocol.ErrorCodeInternal, "terminal creation returned no terminal")
			}
			if err != nil {
				root.dropTerminalTab(id)
				root.reportKeys(c, err, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
				return
			}
			if root.packages.terminals[id] == state {
				root.attachLiveTerminal(state, info)
			}
		})
	}) {
		root.dropTerminalTab(id)
		return "", false
	}
	return id, true
}

// splitLiveTerminal starts a terminal beside terminal id, on its Node and in
// its worktree.
func (r *Runtime) splitLiveTerminal(id string) (string, bool) {
	root := r.localRuntime()
	terminalID, err := protocol.ParseTerminalID(id)
	if err != nil {
		return "", false
	}
	owner, info, exists := root.terminalOwner(terminalID)
	if state := root.packages.terminals[id]; state != nil && state.conn != nil {
		owner, info, exists = state.owner, state.conn.current(), true
	}
	if !exists || info.Worktree == nil {
		root.Report(pluginNativeFault(protocol.ErrorCodeUnavailable, "terminal worktree is unavailable"))
		return "", false
	}
	return root.createLiveTerminal(owner, *info.Worktree)
}

// terminalFolder is the folder a tab's terminal runs in.
func (r *Runtime) terminalFolder(id string) (string, bool) {
	root := r.localRuntime()
	if state := root.packages.terminals[id]; state != nil && state.conn != nil {
		info := state.conn.current()
		return workspaceString(info.Directory), info.Directory != nil
	}
	terminalID, err := protocol.ParseTerminalID(id)
	if err != nil {
		return "", false
	}
	_, info, exists := root.terminalOwner(terminalID)
	return workspaceString(info.Directory), exists && info.Directory != nil
}

// showLiveTerminal shows the tab of a terminal a Node reports.
func (r *Runtime) showLiveTerminal(id string) {
	root := r.localRuntime()
	terminalID, err := protocol.ParseTerminalID(id)
	if err != nil {
		return
	}
	if _, info, exists := root.terminalOwner(terminalID); !exists || info.Status.Kind == "closed" {
		root.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Conflict, i18n.T(c, "workspace_changed")) })
		return
	}
	root.terminalInventory().focus = id
	root.App.Panel.Show(panel.Tab{Kind: panel.Terminal, ID: id})
	root.App.PanelOpen = true
}

// newProjectTerminal is New terminal of a project's menu and overview: a
// shell in the project's worktree at relative, its main one when empty.
func (r *Runtime) newProjectTerminal(project *viewmodel.Project, relative string) {
	root := r.localRuntime()
	owner := root.projectOverviewRuntime(project)
	if owner == nil {
		root.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "unavailable")) })
		return
	}
	worktree, exists := owner.projectWorktree(project, relative)
	if !exists {
		root.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "unavailable")) })
		return
	}
	if _, created := root.createLiveTerminal(owner, worktree.ID); created {
		root.App.PanelOpen = true
	}
}

// projectTerminals are the terminals of a project's worktrees its Node has
// not closed.
func (r *Runtime) projectTerminals(project *viewmodel.Project) []shell.ProjectTerminal {
	owner := r.localRuntime().projectOverviewRuntime(project)
	if owner == nil {
		return nil
	}
	snapshot := owner.snapshot()
	worktrees := map[protocol.WorktreeID]bool{}
	for _, worktree := range snapshot.Worktrees {
		if worktree.Project != nil && worktree.Project.String() == project.ID {
			worktrees[worktree.ID] = true
		}
	}
	var terminals []shell.ProjectTerminal
	for _, info := range snapshot.Terminals {
		if info.Worktree != nil && worktrees[*info.Worktree] && info.Status.Kind != "closed" {
			terminals = append(terminals, shell.ProjectTerminal{ID: info.ID.String(), Title: workspaceString(info.Title), Path: workspaceString(info.Directory)})
		}
	}
	return terminals
}

// dropTerminalTab removes a terminal's tab and detaches the window from it.
// The split workspace drops the tab's pane as it follows the panel.
func (r *Runtime) dropTerminalTab(id string) {
	root := r.localRuntime()
	inventory := root.terminalInventory()
	root.releaseLiveTerminal(id)
	delete(inventory.pending, id)
	delete(inventory.nodes, id)
	if inventory.focus == id {
		inventory.focus = ""
	}
	state := root.App.Panel
	state.Tabs, state.Active = panel.Closed(state.Tabs, panel.Tab{Kind: panel.Terminal, ID: id}, state.Active)
}

// terminalsFrame keeps the terminal tabs in step with the Nodes: a tab whose
// terminal its Node closed or no longer reports leaves, and a terminal whose
// tab is gone is detached. Tabs of a Node that is not connected stay.
func (r *Runtime) terminalsFrame(c *ui.Context) {
	if r.parent != nil || r.packages == nil || r.packages.terminals == nil {
		return
	}
	inventory := r.terminalInventory()
	clear(inventory.paned)
	if c != nil {
		inventory.appearance = pluginNativeAppearance(theme.Of(c))
	}
	if display := r.terminalDisplay(); display != inventory.display {
		inventory.display = display
		for _, state := range r.packages.terminals {
			if state.terminal != nil {
				state.terminal.SetFont(terminalFont(display))
			}
		}
	}
	owners := r.terminalOwners()
	settled := r.terminalOwnersSettled()
	tabs := map[string]bool{}
	for _, tab := range slices.Clone(r.App.Panel.Tabs) {
		if tab.Kind != panel.Terminal {
			continue
		}
		if r.terminalTabCurrent(owners, settled, tab.ID) {
			tabs[tab.ID] = true
			r.closeExitedTerminal(owners, tab.ID)
		} else {
			r.dropTerminalTab(tab.ID)
		}
	}
	for id := range r.packages.terminals {
		if !tabs[id] {
			r.releaseLiveTerminal(id)
		}
	}
	for id := range inventory.nodes {
		if !tabs[id] {
			delete(inventory.nodes, id)
		}
	}
	for id := range inventory.pending {
		if !tabs[id] {
			delete(inventory.pending, id)
		}
	}
	for id := range inventory.exited {
		if !tabs[id] {
			delete(inventory.exited, id)
		}
	}
	if inventory.paste != nil && !tabs[inventory.paste.terminal] {
		inventory.paste = nil
	}
}

// terminalTabCurrent reports whether a terminal tab stays: its terminal is
// in a Node's snapshot and not closed, or its Node cannot say yet.
func (r *Runtime) terminalTabCurrent(owners []*Runtime, settled bool, id string) bool {
	inventory := r.terminalInventory()
	terminalID, err := protocol.ParseTerminalID(id)
	if err != nil {
		// A tab saved by the preview names no Node terminal.
		return false
	}
	for _, owner := range owners {
		for _, info := range owner.snapshot().Terminals {
			if info.ID != terminalID {
				continue
			}
			delete(inventory.pending, id)
			inventory.nodes[id] = owner.controller.Target()
			return info.Status.Kind != "closed"
		}
	}
	if inventory.pending[id] {
		return true
	}
	node, known := inventory.nodes[id]
	if !known {
		// A saved tab no Node has claimed: it leaves once every Node answered.
		return !settled
	}
	for _, owner := range owners {
		if owner.controller.Target() == node {
			return owner.ctx.Err() == nil && !owner.controller.Connected()
		}
	}
	return true
}

// terminalOwnersSettled reports whether the local Node and every discovered
// peer have delivered a snapshot, so a terminal none of them reports is gone.
func (r *Runtime) terminalOwnersSettled() bool {
	if !r.controller.Connected() {
		return false
	}
	if r.network == nil {
		return true
	}
	for _, peer := range r.network.snapshot() {
		if peer.inspecting || peer.controller != nil && (!peer.controller.Connected() || r.bindings[peer.address.ID.String()] == nil) {
			return false
		}
	}
	return true
}

// holdTerminalPaste is paste protection: a paste of several lines waits for
// confirmation, as a shell runs each line at once.
func (r *Runtime) holdTerminalPaste(id string, data transfer.Data) bool {
	inventory := r.terminalInventory()
	if !inventory.display.PasteProtection {
		return false
	}
	text, err := data.Read(transfer.Text)
	if err != nil || !strings.ContainsAny(string(text), "\n\r") {
		return false
	}
	inventory.paste = &terminalPaste{terminal: id, data: data.Snapshot()}
	r.wake()
	return true
}

func (r *Runtime) terminalPasteDialog(c *ui.Context, id string, state *liveTerminal) {
	inventory := r.terminalInventory()
	paste := inventory.paste
	if paste == nil || paste.terminal != id {
		return
	}
	palette := theme.Of(c)
	open := true
	kit.Dialog(c.Key("terminal-paste"), &open, func() {
		ui.Text(c, i18n.T(c, "terminal_paste_confirm")).FontSize(16).FontWeight(600).TextColor(palette.PopoverForeground)
		ui.Text(c, i18n.T(c, "terminal_paste_warning")).FontSize(14).MaxWidth(400).TextColor(palette.MutedForeground)
		ui.Row(c.Key("actions")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).OnClick(func() { inventory.paste = nil })
			kit.Primary(c.Key("paste"), i18n.T(c, "terminal_paste"), false).Children(func() { ui.Text(c, i18n.T(c, "terminal_paste")).FontSize(14) }).OnClick(func() {
				inventory.paste = nil
				state.terminal.PasteData(paste.data)
			})
		})
	})
	if !open {
		inventory.paste = nil
	}
}

// closeExitedTerminal closes a terminal whose shell has exited, once: its tab
// and panes leave when the Node confirms. A failed or stopped terminal stays,
// so its error shows and a stopped one can be reopened.
func (r *Runtime) closeExitedTerminal(owners []*Runtime, id string) {
	inventory := r.terminalInventory()
	if inventory.exited[id] {
		return
	}
	terminalID, err := protocol.ParseTerminalID(id)
	if err != nil {
		return
	}
	for _, owner := range owners {
		for _, info := range owner.snapshot().Terminals {
			if info.ID == terminalID && info.Status.Kind == "exited" {
				if inventory.exited == nil {
					inventory.exited = map[string]bool{}
				}
				inventory.exited[id] = true
				r.closeLiveTerminal(id)
				return
			}
		}
	}
}
