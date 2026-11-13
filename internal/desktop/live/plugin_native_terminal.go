package live

import (
	"context"
	"encoding/json"
	"sync"

	myterminal "github.com/egoist/mygo/plugins/terminal"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginNativeTerminal struct {
	id     protocol.TerminalID
	state  *liveTerminal
	cancel context.CancelFunc
	mu     sync.Mutex
	view   *myterminal.Terminal
	closed bool
}

type pluginNativeTerminalConn struct {
	*terminalConn
	native *pluginNativeState
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
		mounted = state.mountTerminal(identity, id, local, remote)
	}
	if mounted.state.failed {
		return ui.Element{}, pluginNativeFault(protocol.ErrorCodeUnavailable, "plugin terminal is unavailable")
	}
	if mounted.state.reading || mounted.state.terminal == nil {
		return ui.Row(view).Fill().Center().Children(func() { kit.Loader(view, 20) }), nil
	}
	terminal := myterminal.View(view, mounted.state.terminal).Grow(1).MinHeight(0).FillWidth().Disabled(suspended)
	if !suspended && state.terminalFocus == id.String() {
		terminal.Focus()
		state.terminalFocus = ""
	}
	return terminal, nil
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

func (state *pluginNativeState) mountTerminal(identity string, id protocol.TerminalID, local, remote bool) *pluginNativeTerminal {
	ctx, cancel := context.WithCancel(state.native.Context)
	mounted := &pluginNativeTerminal{id: id, cancel: cancel, state: &liveTerminal{owner: state.runtime, reading: true}}
	state.terminals[identity] = mounted
	if !state.runtime.async(func() {
		output, err := state.execute(ctx, protocol.Command{Kind: "inspect_terminal", Data: protocol.CommandInspectTerminalPayload{Terminal: id}})
		snapshot, valid := detail[protocol.TerminalSnapshot](output.Data)
		if err == nil && (!valid || output.Kind != "terminal_snapshot" || snapshot.Node != state.runtime.controller.Target() || snapshot.Info.ID != id ||
			snapshot.Info.Status.Kind == "closed" || !pluginTerminalInScope(state.scope, snapshot.Info, local, remote)) {
			err = pluginNativeFault(protocol.ErrorCodePermissionDenied, "terminal does not match its captured scope")
		}
		var connection *terminalConn
		var terminal *myterminal.Terminal
		if err == nil {
			stream, streamErr := state.runtime.controller.Subscribe(ctx, protocol.Topic{Kind: "terminal", Data: id})
			err = streamErr
			if err == nil {
				connection = &terminalConn{
					owner: state.runtime, ctx: ctx, cancel: cancel, stream: stream,
					ready: make(chan struct{}), info: snapshot.Info, self: state.runtime.localRuntime().controller.Target(),
				}
				scoped := &pluginNativeTerminalConn{terminalConn: connection, native: state}
				terminal, err = myterminal.New(myterminal.Options{
					Conn: scoped, ResizeByConn: true, ClipboardRead: myterminal.ClipboardAsk, Transparent: true,
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
		state.runtime.enqueue(func(view *ui.Context) {
			if state.native.Context.Err() != nil || state.terminals[identity] != mounted {
				if !state.native.Suspended() {
					mounted.release()
				}
				return
			}
			mounted.state.reading = false
			if err != nil {
				mounted.state.failed = true
				state.runtime.packageReport(view, err)
				return
			}
			mounted.state.conn, mounted.state.terminal = connection, terminal
		})
	}) {
		mounted.state.reading, mounted.state.failed = false, true
		cancel()
	}
	return mounted
}

func (connection *pluginNativeTerminalConn) Resize(columns, rows int) error {
	return connection.ResizePixels(columns, rows, 0, 0)
}

func (connection *pluginNativeTerminalConn) ResizePixels(columns, rows, cellWidth, cellHeight int) error {
	info := connection.current()
	columns, rows = min(max(columns, 1), 500), min(max(rows, 1), 200)
	viewport := protocol.TerminalViewport{
		Columns: uint16(columns), Rows: uint16(rows),
		PixelWidth: uint32(columns * max(cellWidth, 0)), PixelHeight: uint32(rows * max(cellHeight, 0)),
	}
	_, err := connection.native.execute(connection.ctx, protocol.Command{Kind: "resize_terminal", Data: protocol.CommandResizeTerminalPayload{Terminal: info.ID, Revision: info.Revision, Viewport: viewport}})
	return err
}

func (connection *pluginNativeTerminalConn) UseTheme(palette *myterminal.Theme) error {
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
	_, err := connection.native.execute(connection.ctx, protocol.Command{Kind: "set_terminal_appearance", Data: protocol.CommandSetTerminalAppearancePayload{Terminal: info.ID, Revision: info.Revision, Appearance: appearance}})
	return err
}
