package live

import (
	"context"
	"errors"
	"sync"

	myterminal "github.com/egoist/mygo/plugins/terminal"
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
	reading  bool
	failed   bool
	query    string
	find     bool
	selected int
}

func (r *Runtime) initLiveTerminals() {
	r.packages.terminals = map[string]*liveTerminal{}
	r.App.Panel.TerminalActions = &panel.TerminalActions{View: r.terminalView, Close: r.closeLiveTerminal, Search: r.searchLiveTerminal}
}

func (r *Runtime) terminalOwner(id protocol.TerminalID) (*Runtime, protocol.TerminalInfo, bool) {
	root := r.localRuntime()
	owners := []*Runtime{root}
	for _, owner := range root.bindings {
		owners = append(owners, owner)
	}
	for _, owner := range owners {
		for _, info := range owner.controller.Snapshot().Terminals {
			if info.ID == id {
				return owner, info, true
			}
		}
	}
	return nil, protocol.TerminalInfo{}, false
}

func (r *Runtime) openLiveTerminal(owner *Runtime, info protocol.TerminalInfo) {
	root := r.localRuntime()
	id := info.ID.String()
	if existing := root.packages.terminals[id]; existing != nil {
		return
	}
	state := &liveTerminal{owner: owner, reading: true}
	root.packages.terminals[id] = state
	ctx, cancel := context.WithCancel(owner.ctx)
	if !owner.async(func() {
		stream, err := owner.controller.Subscribe(ctx, protocol.Topic{Kind: "terminal", Data: info.ID})
		var terminal *myterminal.Terminal
		var connection *terminalConn
		if err == nil {
			connection = &terminalConn{owner: owner, ctx: ctx, cancel: cancel, stream: stream, ready: make(chan struct{}), info: info}
			terminal, err = myterminal.New(myterminal.Options{Conn: connection, ResizeByConn: true, ClipboardRead: myterminal.ClipboardAsk,
				OnClear: func(myterminal.Clear) bool { connection.clearScreen(); return true },
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
			state.reading = false
			if err != nil {
				state.failed = true
				root.packageReport(c, err)
				return
			}
			state.conn, state.terminal = connection, terminal
		})
	}) {
		cancel()
	}
}

func (r *Runtime) terminalView(c *ui.Context, id string) {
	root := r.localRuntime()
	state := root.packages.terminals[id]
	if state == nil {
		terminalID, err := protocol.ParseTerminalID(id)
		if err == nil {
			if owner, info, exists := root.terminalOwner(terminalID); exists && info.Status.Kind != "closed" {
				root.openLiveTerminal(owner, info)
				state = root.packages.terminals[id]
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
	myterminal.View(c, state.terminal).Grow(1).MinHeight(0).FillWidth()
}

func (r *Runtime) closeLiveTerminal(id string) {
	root := r.localRuntime()
	state := root.packages.terminals[id]
	if state == nil || state.conn == nil {
		root.App.Panel.Close(panel.Tab{Kind: panel.Terminal, ID: id})
		return
	}
	info := state.conn.current()
	if info.Worktree == nil {
		root.Report(errors.New("terminal worktree is unavailable"))
		return
	}
	state.owner.executeScoped("terminal-close/"+id, protocol.Command{Kind: "close_terminal", Data: protocol.CommandCloseTerminalPayload{Worktree: *info.Worktree, Terminal: info.ID}}, func(c *ui.Context, out protocol.Output, err error) {
		root.packageReport(c, err)
		if err == nil {
			_ = state.terminal.Close()
			delete(root.packages.terminals, id)
			root.App.Panel.Close(panel.Tab{Kind: panel.Terminal, ID: id})
		}
	})
}

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
	} else if selected != state.selected {
		if selected < state.selected {
			state.terminal.SearchPrevious()
		} else {
			state.terminal.SearchNext()
		}
	}
	status := state.terminal.SearchStatus()
	state.selected = status.Selected
	return status.Total, status.Selected
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
