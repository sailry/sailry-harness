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
	"github.com/sailry/sailry-harness/internal/desktop/theme"
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
	// claiming holds while claim_terminal is unanswered; claimed is the
	// revision an unowned terminal was last claimed at, so one refused
	// automatic claim is not repeated every frame.
	claiming bool
	claimed  uint64
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
		for _, info := range owner.snapshot().Terminals {
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
			connection = &terminalConn{owner: owner, ctx: ctx, cancel: cancel, stream: stream, ready: make(chan struct{}), info: info, self: root.controller.Target()}
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
		ui.Row(c.Key("attaching")).Fill().Center().Children(func() { kit.Loader(c, 20) })
		return
	}
	if state == nil || state.failed || state.terminal == nil {
		props := kit.ResultProps{Icon: icons.Terminal, Title: i18n.T(c, "unavailable")}
		if state != nil && state.failed {
			props.Extra = func() {
				kit.Secondary(c.Key("terminal-retry"), i18n.T(c, "chat_retry")).OnClick(func() { root.retryLiveTerminal(id, state) })
			}
		}
		kit.Result(c, props).Grow(1).Justify(ui.Center)
		return
	}
	screen := myterminal.View(c, state.terminal).Grow(1).MinHeight(0).FillWidth()
	if inventory.focus == id {
		screen.Focus()
		inventory.focus = ""
	}
	root.terminalControl(c, id, state)
	root.terminalPasteDialog(c, id, state)
}

// retryLiveTerminal attaches a terminal again after its attach failed, as a
// saved tab's when its Node was unreachable.
func (r *Runtime) retryLiveTerminal(id string, state *liveTerminal) {
	root := r.localRuntime()
	terminalID, err := protocol.ParseTerminalID(id)
	if err != nil || root.packages.terminals[id] != state || !state.failed || state.reading {
		return
	}
	owner, info, exists := root.terminalOwner(terminalID)
	if !exists || info.Status.Kind == "closed" {
		root.Report(pluginNativeFault(protocol.ErrorCodeUnavailable, "terminal is unavailable"))
		return
	}
	state.owner, state.failed, state.reading = owner, false, true
	root.attachLiveTerminal(state, info)
}

// terminalControl keeps this desktop the terminal's controller. A running
// terminal nobody owns is claimed at once; one another controller owns is
// covered, as its input is refused, until Take control claims it.
func (r *Runtime) terminalControl(c *ui.Context, id string, state *liveTerminal) {
	info := state.conn.current()
	if state.conn.controlling() || info.Status.Kind != "running" || info.Ssh != nil {
		return
	}
	if info.Owner == nil {
		if !state.claiming && state.claimed != info.Revision {
			r.claimLiveTerminal(id, state)
		}
		return
	}
	palette := theme.Of(c)
	ui.Column(c.Key("terminal-control")).Absolute().Left(0).Top(0).Right(0).Bottom(0).Center().Background(palette.Background.Alpha(0.9)).Children(func() {
		kit.Result(c, kit.ResultProps{Icon: icons.Terminal, Title: i18n.T(c, "terminal_other_controller"), Extra: func() {
			kit.Primary(c.Key("terminal-take-control"), i18n.T(c, "terminal_take_control"), state.claiming).OnClick(func() { r.claimLiveTerminal(id, state) })
		}})
	})
}

// claimLiveTerminal sends claim_terminal at the revision this view holds. A
// refusal leaves the terminal with its controller.
func (r *Runtime) claimLiveTerminal(id string, state *liveTerminal) {
	root, connection := r.localRuntime(), state.conn
	if connection == nil || state.claiming {
		return
	}
	info := connection.current()
	state.claiming, state.claimed = true, info.Revision
	if !state.owner.async(func() {
		output, err := state.owner.controller.Do(connection.ctx, protocol.Command{Kind: "claim_terminal", Data: protocol.CommandClaimTerminalPayload{Terminal: info.ID, ExpectedRevision: info.Revision}})
		claimed, valid := detail[protocol.TerminalInfo](output.Data)
		if err == nil && valid {
			connection.claimed(claimed)
		}
		root.enqueue(func(c *ui.Context) {
			state.claiming = false
			if root.packages.terminals[id] != state || state.conn != connection || connection.ctx.Err() != nil {
				return
			}
			root.reportKeys(c, err, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
			if err == nil {
				root.terminalInventory().focus = id
			}
		})
	}) {
		state.claiming = false
	}
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
	pending  []byte
	// self is the identity this desktop's commands carry on every Node.
	self protocol.NodeID
	mu   sync.Mutex
	info protocol.TerminalInfo
	// rejected holds while a refused write has been reported and none has
	// been accepted since.
	rejected bool
	// viewport and appearance are what the view last asked for.
	viewport   *protocol.TerminalViewport
	appearance *protocol.TerminalAppearance
}
