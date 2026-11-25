package live

import (
	"context"
	"encoding/json"
	"sync"

	myterminal "github.com/egoist/mygo/plugins/terminal"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginNativeTerminal struct {
	id      protocol.TerminalID
	state   *liveTerminal
	display preferences.Terminal
	cancel  context.CancelFunc
	mu      sync.Mutex
	view    *myterminal.Terminal
	closed  bool
}

type embeddedTerminalConn struct {
	*terminalConn
	execute func(context.Context, protocol.Command) (protocol.Output, error)
}

func (mounted *pluginNativeTerminal) retain(view *myterminal.Terminal) bool {
	mounted.mu.Lock()
	defer mounted.mu.Unlock()
	if mounted.closed {
		_ = view.Close()
		return false
	}
	mounted.view = view
	return true
}

func (mounted *pluginNativeTerminal) release() {
	mounted.mu.Lock()
	if mounted.closed {
		mounted.mu.Unlock()
		return
	}
	mounted.closed = true
	view := mounted.view
	mounted.view = nil
	mounted.mu.Unlock()
	mounted.cancel()
	if view != nil {
		_ = view.Close()
	}
}

func (mounted *pluginNativeTerminal) draw(view *ui.Context, display preferences.Terminal, suspended bool, focus *string) ui.Element {
	if mounted.state.reading || mounted.state.terminal == nil {
		return ui.Row(view).Fill().Center().Children(func() { kit.Loader(view, 20) })
	}
	if display != mounted.display {
		mounted.state.terminal.SetFont(terminalFont(display))
		mounted.display = display
	}
	terminal := myterminal.View(view, mounted.state.terminal).Grow(1).MinHeight(0).FillWidth().Disabled(suspended)
	if !suspended && *focus == mounted.id.String() {
		terminal.Focus()
		*focus = ""
	}
	return terminal
}

func (state *pluginNativeState) terminalOperations() {
	state.native.Operations["sailry.focusTerminal"] = func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		id, err := pluginNativeArgument[protocol.TerminalID](arguments, 0)
		if err != nil {
			return nil, err
		}
		return state.effect(func(*ui.Context) { state.terminalFocus = id.String() })
	}
	state.native.Operations["sailry.releaseTerminal"] = func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		id, err := pluginNativeArgument[protocol.TerminalID](arguments, 0)
		if err != nil {
			return nil, err
		}
		return state.effect(func(*ui.Context) {
			for identity, mounted := range state.terminals {
				if mounted.id == id {
					mounted.release()
					delete(state.terminals, identity)
				}
			}
			if state.terminalFocus == id.String() {
				state.terminalFocus = ""
			}
		})
	}
}

func (state *pluginNativeState) renderTerminal(view *ui.Context, element plugins.NativeElement, _ func(), _ func(plugins.CallbackRef, json.RawMessage)) (ui.Element, error) {
	identity, props, err := pluginNativeProps(element)
	if err != nil {
		return ui.Element{}, err
	}
	var id protocol.TerminalID
	if raw := props["terminal"]; len(raw) == 0 || string(raw) == "null" || json.Unmarshal(raw, &id) != nil {
		if mounted := state.terminals[identity]; mounted != nil {
			mounted.release()
			delete(state.terminals, identity)
		}
		return ui.Box(view).Size(0, 0), nil
	}
	mounted := state.terminals[identity]
	suspended := state.native.Suspended()
	var local, remote bool
	if suspended {
		if mounted == nil || mounted.id != id {
			return ui.Box(view).Size(0, 0), nil
		}
	} else if local, remote, err = state.terminalAuthority(); err != nil {
		return ui.Element{}, err
	}
	if mounted != nil && mounted.id != id {
		mounted.release()
		delete(state.terminals, identity)
		mounted = nil
	}
	if mounted == nil {
		mounted = state.runtime.mountEmbeddedTerminal(state.native.Context, id, state.execute, func(info protocol.TerminalInfo) bool {
			return pluginTerminalInScope(state.scope, info, local, remote)
		})
		state.terminals[identity] = mounted
	}
	if mounted.state.failed {
		return ui.Element{}, pluginNativeFault(protocol.ErrorCodeUnavailable, "plugin terminal is unavailable")
	}
	return mounted.draw(view, state.runtime.localRuntime().terminalDisplay(), suspended, &state.terminalFocus), nil
}

// terminalAuthority reports the terminals a workspace view's package may
// mount, as the reference does: a worktree's (local) with the terminal
// actions, a saved SSH connection's (remote) with the SSH ones. The Node
// checks each command of either kind against the same actions.
func (state *pluginNativeState) terminalAuthority() (local, remote bool, err error) {
	err = state.workspaceComponent(protocol.PluginActionReadTerminals, protocol.PluginActionControlTerminals)
	local = err == nil
	remote = state.workspaceComponent(protocol.PluginActionReadSsh, protocol.PluginActionControlSsh) == nil
	if local || remote {
		err = nil
	}
	return local, remote, err
}

// pluginTerminalInScope reports whether a view may show a terminal: an SSH
// terminal with the SSH actions, any other in the view's own worktree with
// the terminal actions.
func pluginTerminalInScope(scope protocol.PluginContext, info protocol.TerminalInfo, local, remote bool) bool {
	if info.Ssh != nil {
		return remote
	}
	return local && scope.Worktree != nil && info.Worktree != nil && *info.Worktree == *scope.Worktree
}

func (r *Runtime) mountEmbeddedTerminal(parent context.Context, id protocol.TerminalID, execute func(context.Context, protocol.Command) (protocol.Output, error), allows func(protocol.TerminalInfo) bool) *pluginNativeTerminal {
	ctx, cancel := context.WithCancel(parent)
	display := r.localRuntime().terminalDisplay()
	mounted := &pluginNativeTerminal{id: id, cancel: cancel, display: display, state: &liveTerminal{owner: r, reading: true}}
	if !r.async(func() {
		output, err := execute(ctx, protocol.Command{Kind: "inspect_terminal", Data: protocol.CommandInspectTerminalPayload{Terminal: id}})
		snapshot, valid := detail[protocol.TerminalSnapshot](output.Data)
		if err == nil && (!valid || output.Kind != "terminal_snapshot" || snapshot.Node != r.controller.Target() || snapshot.Info.ID != id ||
			snapshot.Info.Status.Kind == "closed" || !allows(snapshot.Info)) {
			err = pluginNativeFault(protocol.ErrorCodePermissionDenied, "terminal does not match its captured scope")
		}
		var connection *terminalConn
		var terminal *myterminal.Terminal
		if err == nil {
			stream, streamErr := r.controller.Subscribe(ctx, protocol.Topic{Kind: "terminal", Data: id})
			err = streamErr
			if err == nil {
				connection = &terminalConn{
					owner: r, ctx: ctx, cancel: cancel, stream: stream,
					ready: make(chan struct{}), info: snapshot.Info, self: r.localRuntime().controller.Target(),
				}
				scoped := &embeddedTerminalConn{terminalConn: connection, execute: execute}
				terminal, err = myterminal.New(myterminal.Options{
					Conn: scoped, ResizeByConn: true, ClipboardRead: myterminal.ClipboardAsk, Transparent: true,
					Font:    terminalFont(display),
					OnClear: func(myterminal.Clear) bool { connection.clearScreen(); return true },
				})
				if err == nil {
					connection.terminal = terminal
					close(connection.ready)
					context.AfterFunc(ctx, func() { _ = connection.Close() })
					mounted.retain(terminal)
				} else {
					_ = connection.Close()
				}
			}
		}
		if err != nil {
			cancel()
		}
		r.enqueue(func(view *ui.Context) {
			mounted.mu.Lock()
			closed := mounted.closed
			mounted.mu.Unlock()
			if parent.Err() != nil || closed {
				return
			}
			mounted.state.reading = false
			if err != nil {
				mounted.state.failed = true
				r.packageReport(view, err)
				return
			}
			mounted.state.conn, mounted.state.terminal = connection, terminal
			if current := r.localRuntime().terminalDisplay(); current != display {
				terminal.SetFont(terminalFont(current))
				mounted.display = current
			}
		})
	}) {
		mounted.state.reading, mounted.state.failed = false, true
		cancel()
	}
	return mounted
}

func (connection *embeddedTerminalConn) Resize(columns, rows int) error {
	return connection.ResizePixels(columns, rows, 0, 0)
}

func (connection *embeddedTerminalConn) ResizePixels(columns, rows, cellWidth, cellHeight int) error {
	info := connection.current()
	columns, rows = min(max(columns, 1), 500), min(max(rows, 1), 200)
	viewport := protocol.TerminalViewport{
		Columns: uint16(columns), Rows: uint16(rows),
		PixelWidth: uint32(columns * max(cellWidth, 0)), PixelHeight: uint32(rows * max(cellHeight, 0)),
	}
	_, err := connection.execute(connection.ctx, protocol.Command{Kind: "resize_terminal", Data: protocol.CommandResizeTerminalPayload{Terminal: info.ID, Revision: info.Revision, Viewport: viewport}})
	return err
}

func (connection *embeddedTerminalConn) UseTheme(palette *myterminal.Theme) error {
	color := func(value ui.Color) protocol.TerminalRgb {
		return protocol.TerminalRgb{Red: value.R, Green: value.G, Blue: value.B}
	}
	appearance := protocol.TerminalAppearance{Foreground: color(palette.Foreground), Background: color(palette.Background), ColorScheme: protocol.TerminalColorSchemeLight}
	if 299*int(palette.Background.R)+587*int(palette.Background.G)+114*int(palette.Background.B) < 128000 {
		appearance.ColorScheme = protocol.TerminalColorSchemeDark
	}
	for index, entry := range palette.Palette {
		appearance.Palette[index] = color(entry)
	}
	info := connection.current()
	_, err := connection.execute(connection.ctx, protocol.Command{Kind: "set_terminal_appearance", Data: protocol.CommandSetTerminalAppearancePayload{Terminal: info.ID, Revision: info.Revision, Appearance: appearance}})
	return err
}
