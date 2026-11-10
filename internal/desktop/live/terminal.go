package live

import (
	"context"
	"sync"

	myterminal "github.com/egoist/mygo/plugins/terminal"
	"github.com/egoist/mygo/transfer"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type liveTerminal struct {
	owner    *Runtime
	conn     *terminalConn
	terminal *myterminal.Terminal
	cancel   context.CancelFunc
	reading  bool
	failed   bool
	query    string
	find     bool
	selected int
}

func (r *Runtime) initLiveTerminals() {
	r.packages.terminals = map[string]*liveTerminal{}
	r.App.Panel.TerminalActions = &panel.TerminalActions{
		View: r.terminalView, Pane: r.terminalPane, Close: r.closeLiveTerminal, Search: r.searchLiveTerminal,
		Folder: r.terminalFolder, Split: r.splitLiveTerminal,
	}
	r.initTerminalActions()
}

func (r *Runtime) terminalOwners() []*Runtime {
	root := r.localRuntime()
	owners := []*Runtime{root}
	for _, owner := range root.bindings {
		owners = append(owners, owner)
	}
	return owners
}

func (r *Runtime) terminalOwner(id protocol.TerminalID) (*Runtime, protocol.TerminalInfo, bool) {
	for _, owner := range r.terminalOwners() {
		for _, info := range owner.controller.Snapshot().Terminals {
			if info.ID == id {
				return owner, info, true
			}
		}
	}
	return nil, protocol.TerminalInfo{}, false
}

// openLiveTerminal attaches the panel tab of a terminal its Node already
// reported. The tab stays until that terminal appears in a snapshot.
func (r *Runtime) openLiveTerminal(owner *Runtime, info protocol.TerminalInfo) {
	root := r.localRuntime()
	id := info.ID.String()
	if existing := root.packages.terminals[id]; existing != nil {
		return
	}
	state := &liveTerminal{owner: owner, reading: true}
	root.packages.terminals[id] = state
	root.terminalInventory().pending[id] = true
	root.attachLiveTerminal(state, info)
}

// attachLiveTerminal subscribes state to its Node terminal. A terminal that
// is not running, as a saved tab's after its Node restarted, is opened again
// under the same ID first.
func (r *Runtime) attachLiveTerminal(state *liveTerminal, info protocol.TerminalInfo) {
	root, owner := r.localRuntime(), state.owner
	id := info.ID.String()
	inventory := root.terminalInventory()
	display, appearance := inventory.display, inventory.appearance
	ctx, cancel := context.WithCancel(owner.ctx)
	state.cancel = cancel
	if !owner.async(func() {
		if info.Status.Kind != "running" {
			output, err := owner.controller.Do(ctx, protocol.Command{Kind: "open_terminal", Data: protocol.CommandOpenTerminalPayload{Terminal: info.ID, Viewport: terminalViewport, Appearance: appearance}})
			if opened, valid := detail[protocol.TerminalInfo](output.Data); err == nil && valid {
				info = opened
			}
		}
		stream, err := owner.controller.Subscribe(ctx, protocol.Topic{Kind: "terminal", Data: info.ID})
		var terminal *myterminal.Terminal
		var connection *terminalConn
		if err == nil {
			connection = &terminalConn{owner: owner, ctx: ctx, cancel: cancel, stream: stream, ready: make(chan struct{}), info: info}
			terminal, err = myterminal.New(myterminal.Options{Conn: connection, ResizeByConn: true, ClipboardRead: myterminal.ClipboardAsk, Font: terminalFont(display),
				OnClear: func(myterminal.Clear) bool { connection.clearScreen(); return true },
				OnPaste: func(data transfer.Data) bool { return root.holdTerminalPaste(id, data) },
			})
			if err == nil {
				connection.terminal = terminal
				close(connection.ready)
				context.AfterFunc(ctx, func() { _ = terminal.Close() })
			} else {
				_ = connection.Close()
			}
		} else {
			cancel()
		}
		root.enqueue(func(c *ui.Context) {
			if root.packages.terminals[id] != state {
				if connection != nil {
					_ = connection.Close()
				}
				return
			}
			state.reading = false
			if err != nil {
				state.failed = true
				root.reportKeys(c, err, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
				return
			}
			state.conn, state.terminal = connection, terminal
			if current := root.terminalInventory().display; current != display {
				terminal.SetFont(terminalFont(current))
			}
		})
	}) {
		cancel()
		state.reading, state.failed = false, true
	}
}

// releaseLiveTerminal detaches this window from a terminal. The Node
// terminal keeps running.
func (r *Runtime) releaseLiveTerminal(id string) {
	root := r.localRuntime()
	state := root.packages.terminals[id]
	if state == nil {
		return
	}
	delete(root.packages.terminals, id)
	if state.cancel != nil {
		state.cancel()
	}
	if state.conn != nil {
		_ = state.conn.Close()
	}
	if state.terminal != nil {
		_ = state.terminal.Close()
	}
}

// terminalView is a terminal tab's screen. While the terminal's split pane is
// displayed the pane owns the screen, as MyGo shows a terminal in one view at
// a time, and the tab's screen stays empty.
func (r *Runtime) terminalView(c *ui.Context, id string) {
	root := r.localRuntime()
	if root.terminalInventory().paned[id] {
		return
	}
	root.terminalScreen(c, id)
}

// terminalPane is the same terminal's screen in a split pane.
func (r *Runtime) terminalPane(c *ui.Context, id string) {
	root := r.localRuntime()
	root.terminalInventory().paned[id] = true
	root.terminalScreen(c, id)
}

func (r *Runtime) terminalScreen(c *ui.Context, id string) {
	root := r.localRuntime()
	inventory := root.terminalInventory()
	state := root.packages.terminals[id]
	if state == nil {
		terminalID, err := protocol.ParseTerminalID(id)
		if err == nil {
			if owner, info, exists := root.terminalOwner(terminalID); exists && info.Status.Kind != "closed" {
				state = &liveTerminal{owner: owner, reading: true}
				root.packages.terminals[id] = state
				root.attachLiveTerminal(state, info)
			}
		}
	}
	if state != nil && state.reading {
		ui.Spinner(c).Label(i18n.T(c, "terminal"))
		return
	}
	if state == nil || state.failed || state.terminal == nil {
		kit.Result(c, kit.ResultProps{Icon: icons.Terminal, Title: i18n.T(c, "unavailable")}).Grow(1).Justify(ui.Center)
		return
	}
	screen := myterminal.View(c, state.terminal).Grow(1).MinHeight(0).FillWidth()
	if inventory.focus == id {
		screen.Focus()
		inventory.focus = ""
	}
	root.terminalPasteDialog(c, id, state)
}

// closeLiveTerminal is Close terminal: the Node closes the shell, then the
// tab goes, and with it the terminal's split pane.
func (r *Runtime) closeLiveTerminal(id string) {
	root := r.localRuntime()
	inventory := root.terminalInventory()
	terminalID, err := protocol.ParseTerminalID(id)
	if err != nil {
		root.dropTerminalTab(id)
		return
	}
	owner, info, exists := root.terminalOwner(terminalID)
	if state := root.packages.terminals[id]; state != nil && state.conn != nil {
		owner, info, exists = state.owner, state.conn.current(), true
	}
	if !exists {
		// Its creation has not answered yet: the tab leaves when it does.
		if !inventory.pending[id] {
			root.dropTerminalTab(id)
		}
		return
	}
	command := protocol.Command{Kind: "close_ssh_terminal", Data: protocol.CommandCloseSshTerminalPayload{Terminal: info.ID}}
	if info.Worktree != nil {
		command = protocol.Command{Kind: "close_terminal", Data: protocol.CommandCloseTerminalPayload{Worktree: *info.Worktree, Terminal: info.ID}}
	} else if info.Ssh == nil {
		root.Report(pluginNativeFault(protocol.ErrorCodeUnavailable, "terminal worktree is unavailable"))
		return
	}
	owner.executeScoped("terminal-close/"+id, command, func(c *ui.Context, _ protocol.Output, err error) {
		if err != nil && faultCode(err) != protocol.ErrorCodeNotFound {
			root.reportKeys(c, err, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
			return
		}
		root.dropTerminalTab(id)
	})
}

// searchLiveTerminal runs a terminal tab's find bar: it returns the matches
// and the selected one's index. The bar counts its matches from 0 and steps
// round their ends; the terminal counts from 1, 0 while none is selected.
func (r *Runtime) searchLiveTerminal(id, query string, find bool, selected int) (int, int) {
	state := r.localRuntime().packages.terminals[id]
	if state == nil || state.terminal == nil {
		return 0, 0
	}
	if !find {
		query = ""
	}
	if state.query != query || state.find != find {
		state.terminal.Search(query)
		state.query, state.find = query, find
	} else if total := state.terminal.SearchStatus().Total; total > 0 && selected != state.selected {
		if selected == (state.selected+1)%total {
			state.terminal.SearchNext()
		} else {
			state.terminal.SearchPrevious()
		}
	}
	status := state.terminal.SearchStatus()
	state.selected = max(status.Selected-1, 0)
	return status.Total, state.selected
}

type terminalConn struct {
	owner    *Runtime
	ctx      context.Context
	cancel   context.CancelFunc
	stream   link.Subscription
	ready    chan struct{}
	terminal *myterminal.Terminal
	close    sync.Once
	read     sync.Mutex
	mu       sync.Mutex
	info     protocol.TerminalInfo
	pending  []byte
}
