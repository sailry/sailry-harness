package live

import (
	"strings"
	"time"

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

// terminalState tracks the window's terminal surfaces, either workspace
// panes or conversation panel tabs, by their Node terminal IDs.
type terminalState struct {
	// pending tabs wait for their terminal's first snapshot and are not
	// pruned: a command's output can arrive before the Node's update.
	pending map[string]bool
	// nodes is the Node each terminal was last seen on; a detached terminal
	// with chrome keeps its owner so it can be recognized when it exits.
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

type terminalPlacement uint8

const (
	terminalPanel terminalPlacement = iota
	terminalWorkspace
)

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
	actions := r.App.Sidebar.Actions
	actions.NewTerminal = func(project *viewmodel.Project) { r.newProjectTerminal(project, "") }
	actions.Terminals = r.projectTerminals
	// A sidebar row opens its terminal as a session row does: a kept tab.
	actions.OpenTerminal = func(id string) {
		r.showWorkspaceTerminal(id)
		r.localRuntime().App.KeepWorkspaceTerminal(id)
	}
	actions.CloseTerminal = r.closeLiveTerminal
}

func panelTerminalWorktree(owner *Runtime) protocol.WorktreeID {
	if owner == nil || !owner.controller.Connected() || owner.workspace == nil {
		return protocol.WorktreeID{}
	}
	return owner.workspace.selected
}

func (r *Runtime) createPanelTerminal() {
	owner := r.selectedRuntime()
	r.createLiveTerminal(owner, panelTerminalWorktree(owner), protocol.ProcessAccessUnrestricted, terminalPanel)
}

// createLiveTerminal starts a shell in worktree on owner's Node and shows
// its surface at once using the request's ID. A failed creation removes it.
func (r *Runtime) createLiveTerminal(owner *Runtime, worktree protocol.WorktreeID, access protocol.ProcessAccess, placement terminalPlacement) (string, bool) {
	root := r.localRuntime()
	if owner == nil || !root.terminalAssembled() || !owner.controller.Connected() || worktree == (protocol.WorktreeID{}) {
		root.terminalUnavailable()
		return "", false
	}
	inventory := root.terminalInventory()
	request := owner.controller.Prepare(protocol.Command{Kind: "create_terminal", Data: protocol.TerminalLaunch{
		Worktree: worktree, Access: access, Viewport: terminalViewport, Appearance: inventory.appearance,
	}})
	id := protocol.TerminalID(request.ID).String()
	state := &liveTerminal{owner: owner, reading: true}
	root.packages.terminals[id] = state
	inventory.pending[id], inventory.focus = true, id
	inventory.nodes[id] = owner.controller.Target()
	if placement == terminalWorkspace {
		root.App.OpenWorkspaceTerminal(id, "")
	} else {
		root.App.RevealConversationPanel()
		root.App.Panel.Show(panel.Tab{Kind: panel.Terminal, ID: id})
	}
	if !owner.async(func() {
		output, err := owner.controller.Execute(owner.ctx, request)
		root.enqueue(func(c *ui.Context) {
			if root.packages.terminals[id] != state {
				return
			}
			info, valid := detail[protocol.TerminalInfo](output.Data)
			if err == nil && !valid {
				err = pluginNativeFault(protocol.ErrorCodeInternal, "terminal creation returned no terminal")
			}
			if err != nil {
				root.dropTerminalTab(id)
				root.reportKeys(c, err, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
				return
			}
			if state.closing {
				command := protocol.Command{Kind: "close_terminal", Data: protocol.CommandCloseTerminalPayload{Worktree: worktree, Terminal: info.ID}}
				root.closeTerminalCommand(owner, id, command, func() {
					state.closing = false
					if root.packages.terminals[id] == state {
						root.attachLiveTerminal(state, info)
					}
				})
				return
			}
			root.attachLiveTerminal(state, info)
		})
	}) {
		root.dropTerminalTab(id)
		return "", false
	}
	return id, true
}

// terminalUnavailable is the toast of a terminal that cannot start: no
// connected Node has a worktree for it.
func (r *Runtime) terminalUnavailable() {
	r.localRuntime().enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "terminal_unavailable")) })
}

// splitLiveTerminal starts a terminal beside terminal id, on its Node, in its
// worktree and with its access mode. A peer that omits the mode cannot grant full access.
func (r *Runtime) splitLiveTerminal(id string) (string, bool) {
	root := r.localRuntime()
	if _, err := protocol.ParseTerminalID(id); err != nil {
		return "", false
	}
	owner, info, exists := root.liveTerminalInfo(id)
	if !exists || info.Worktree == nil {
		root.terminalUnavailable()
		return "", false
	}
	access := protocol.ProcessAccessWorkspace
	if info.Access != nil {
		access = *info.Access
	}
	return root.createLiveTerminal(owner, *info.Worktree, access, terminalWorkspace)
}

// liveTerminalInfo is terminal id's Node and latest report: its attachment's,
// else its Node's snapshot. A terminal still being created has its Node and no
// report yet.
func (r *Runtime) liveTerminalInfo(id string) (*Runtime, protocol.TerminalInfo, bool) {
	root := r.localRuntime()
	var state *liveTerminal
	if root.packages != nil {
		state = root.packages.terminals[id]
	}
	if state != nil && state.conn != nil {
		return state.owner, state.conn.current(), true
	}
	if terminalID, err := protocol.ParseTerminalID(id); err == nil {
		if owner, info, exists := root.terminalOwner(terminalID); exists {
			return owner, info, true
		}
	}
	if state != nil {
		return state.owner, protocol.TerminalInfo{}, false
	}
	return nil, protocol.TerminalInfo{}, false
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
	root.App.DropWorkspaceTerminal(id)
	root.App.RevealConversationPanel()
	root.App.Panel.Show(panel.Tab{Kind: panel.Terminal, ID: id})
}

// showWorkspaceTerminal opens a project's terminal as an independent main
// surface. Moving it from a panel keeps one renderer attached to the Node.
func (r *Runtime) showWorkspaceTerminal(id string) {
	root := r.localRuntime()
	terminalID, err := protocol.ParseTerminalID(id)
	if err != nil {
		return
	}
	_, info, exists := root.terminalOwner(terminalID)
	if !exists || info.Status.Kind == "closed" {
		root.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Conflict, i18n.T(c, "workspace_changed")) })
		return
	}
	root.terminalInventory().focus = id
	root.App.TakePanelTab(panel.Tab{Kind: panel.Terminal, ID: id})
	root.App.OpenWorkspaceTerminal(id, workspaceString(info.Title))
}

// newProjectTerminal is New terminal of a project's menu and overview: a
// shell in the project's worktree at relative, its main one when empty.
func (r *Runtime) newProjectTerminal(project *viewmodel.Project, relative string) {
	root := r.localRuntime()
	owner := root.projectOverviewRuntime(project)
	if owner == nil || !owner.controller.Connected() {
		root.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "resource_offline_action")) })
		return
	}
	worktree, exists := owner.projectWorktree(project, relative)
	if !exists {
		root.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "unavailable")) })
		return
	}
	root.createLiveTerminal(owner, worktree.ID, protocol.ProcessAccessUnrestricted, terminalWorkspace)
}

// projectTerminals lists the running shells in a project's worktrees on its
// Node, including those another controller such as Mobile started, so this
// window can open one it has no surface for.
func (r *Runtime) projectTerminals(project *viewmodel.Project) []shell.ProjectTerminal {
	owner := r.localRuntime().projectOverviewRuntime(project)
	if owner == nil {
		return nil
	}
	snapshot := owner.snapshot()
	paths := map[protocol.WorktreeID]string{}
	for _, tree := range snapshot.Worktrees {
		if tree.Project != nil && tree.Project.String() == project.ID {
			paths[tree.ID] = tree.Path
		}
	}
	terminals := []shell.ProjectTerminal{}
	for _, info := range snapshot.Terminals {
		if info.Worktree == nil || info.Ssh != nil || info.Tool != nil || info.Status.Kind != "running" {
			continue
		}
		path, inProject := paths[*info.Worktree]
		if !inProject {
			continue
		}
		if info.Directory != nil {
			path = *info.Directory
		}
		listed := shell.ProjectTerminal{ID: info.ID.String(), Title: workspaceString(info.Title), Path: path, Created: info.CreationOrder}
		if info.UpdatedMs != 0 {
			listed.Updated = time.UnixMilli(info.UpdatedMs)
		} else {
			// Older Nodes identify project shells by the originating launch request.
			listed.Updated, _ = protocol.RequestID(info.ID).CreatedAt()
		}
		terminals = append(terminals, listed)
	}
	return terminals
}

// dropTerminalTab removes every surface of a terminal and detaches the window.
func (r *Runtime) dropTerminalTab(id string) {
	root := r.localRuntime()
	inventory := root.terminalInventory()
	root.releaseLiveTerminal(id)
	delete(inventory.pending, id)
	delete(inventory.nodes, id)
	if inventory.focus == id {
		inventory.focus = ""
	}
	// The tab is in the panel of the conversation that opened it, shown or not.
	tab := panel.Tab{Kind: panel.Terminal, ID: id}
	root.App.DropPanelTab(tab)
	root.App.DropWorkspaceTerminal(id)
}

// terminalsFrame follows both main workspace surfaces and conversation panel
// tabs. Node closure removes a surface; closing the last surface releases its
// attachment. Disconnected Nodes and hidden conversations retain their surfaces.
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
	for _, id := range r.App.WorkspaceTerminalIDs() {
		if r.terminalTabCurrent(owners, settled, id) {
			tabs[id] = true
		} else {
			r.dropTerminalTab(id)
		}
	}
	for _, tab := range r.App.AllPanelTabs() {
		if tab.Kind != panel.Terminal {
			continue
		}
		if r.terminalTabCurrent(owners, settled, tab.ID) {
			tabs[tab.ID] = true
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
		if tabs[id] {
			continue
		}
		if !inventory.pending[id] && r.App.Panel.HasTerminalChrome(id) {
			if r.terminalTabCurrent(owners, settled, id) {
				continue
			}
			r.App.Panel.DiscardTabChrome(panel.Tab{Kind: panel.Terminal, ID: id})
		}
		delete(inventory.nodes, id)
	}
	for id := range inventory.pending {
		if !tabs[id] {
			delete(inventory.pending, id)
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
	open := true
	if kit.AlertDialog(c.Key(paste), &open, i18n.T(c, "terminal_paste_confirm"), i18n.T(c, "terminal_paste_warning"),
		i18n.T(c, "settings_cancel"), i18n.T(c, "terminal_paste")) == 1 {
		inventory.paste = nil
		state.terminal.PasteData(paste.data)
	}
	if !open {
		inventory.paste = nil
	}
}
