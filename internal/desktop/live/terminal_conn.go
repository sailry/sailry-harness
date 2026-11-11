package live

import (
	"errors"
	"io"

	myterminal "github.com/egoist/mygo/plugins/terminal"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (connection *terminalConn) current() protocol.TerminalInfo {
	connection.mu.Lock()
	defer connection.mu.Unlock()
	return connection.info
}

func (connection *terminalConn) Read(destination []byte) (int, error) {
	if len(destination) == 0 {
		return 0, nil
	}
	connection.read.Lock()
	defer connection.read.Unlock()
	select {
	case <-connection.ctx.Done():
		return 0, connection.ctx.Err()
	case <-connection.ready:
	}
	for len(connection.pending) == 0 {
		update, err := connection.stream.Next(connection.ctx)
		if err != nil {
			return 0, err
		}
		var screen protocol.TerminalScreen
		var info protocol.TerminalInfo
		switch update.Kind {
		case "terminal_snapshot":
			value, valid := detail[protocol.TerminalSnapshot](update.Data)
			if !valid || value.Node != connection.owner.controller.Target() {
				return 0, errors.New("unexpected terminal snapshot response")
			}
			info, screen = value.Info, value.Screen
		case "terminal_frame":
			value, valid := detail[protocol.TerminalFrame](update.Data)
			replacement, replace := detail[protocol.TerminalScreenScreenUpdateReplacePayload](value.Screen.Data)
			if !valid || value.Node != connection.owner.controller.Target() || value.Screen.Kind != "replace" || !replace {
				return 0, errors.New("unexpected terminal frame response")
			}
			info, screen = value.Info, replacement.Screen
		default:
			continue
		}
		if info.ID != connection.current().ID || screen.VT == nil {
			return 0, errors.New("terminal stream does not match its captured terminal")
		}
		connection.mu.Lock()
		previous := connection.info
		connection.info = info
		connection.mu.Unlock()
		if previous.Revision != info.Revision {
			// Ownership or status changed: the view's control state follows.
			connection.owner.localRuntime().wake()
		}
		viewport := screen.VT.Viewport
		connection.terminal.Resize(int(viewport.Columns), int(viewport.Rows))
		connection.pending = screen.VT.Bytes
		if len(connection.pending) == 0 && (info.Status.Kind == "closed" || info.Status.Kind == "stopped") {
			return 0, io.EOF
		}
	}
	count := copy(destination, connection.pending)
	connection.pending = connection.pending[count:]
	return count, nil
}

// terminalInputLimit is the Node's bound on one input_terminal command.
const terminalInputLimit = 64 * 1024

// controlling reports whether this desktop owns the terminal's input. Only
// the owner's input, size and appearance are accepted by the Node.
func (connection *terminalConn) controlling() bool {
	info := connection.current()
	return info.Owner != nil && *info.Owner == connection.self && info.Status.Kind == "running"
}

// Write sends typed and pasted bytes in order, in commands within the Node's
// input bound. MyGo closes a terminal's input for good on a write error, so
// only a closed connection returns one: input typed while another controller
// owns the terminal is dropped, and a rejected command drops the rest of its
// write and is reported once until input is accepted again.
func (connection *terminalConn) Write(source []byte) (int, error) {
	if len(source) == 0 {
		return 0, nil
	}
	if err := connection.ctx.Err(); err != nil {
		return 0, err
	}
	if !connection.controlling() {
		return len(source), nil
	}
	for rest := source; len(rest) > 0; {
		chunk := rest[:min(len(rest), terminalInputLimit)]
		rest = rest[len(chunk):]
		info := connection.current()
		_, err := connection.owner.controller.Do(connection.ctx, protocol.Command{Kind: "input_terminal", Data: protocol.CommandInputTerminalPayload{Terminal: info.ID, Revision: info.Revision, Bytes: append(protocol.Bytes(nil), chunk...), Inputs: []protocol.TerminalInput{}}})
		if err != nil {
			if closed := connection.ctx.Err(); closed != nil {
				return 0, closed
			}
			connection.mu.Lock()
			reported := connection.rejected
			connection.rejected = true
			connection.mu.Unlock()
			if !reported {
				connection.owner.Report(err)
			}
			return len(source), nil
		}
	}
	connection.mu.Lock()
	connection.rejected = false
	connection.mu.Unlock()
	return len(source), nil
}

func (connection *terminalConn) Resize(cols, rows int) error {
	return connection.ResizePixels(cols, rows, 0, 0)
}

func (connection *terminalConn) ResizePixels(cols, rows, cellWidth, cellHeight int) error {
	cols, rows = min(max(cols, 1), 500), min(max(rows, 1), 200)
	viewport := protocol.TerminalViewport{Columns: uint16(cols), Rows: uint16(rows), PixelWidth: uint32(cols * max(cellWidth, 0)), PixelHeight: uint32(rows * max(cellHeight, 0))}
	connection.mu.Lock()
	connection.viewport = &viewport
	connection.mu.Unlock()
	return connection.sendViewport(viewport)
}

func (connection *terminalConn) sendViewport(viewport protocol.TerminalViewport) error {
	if !connection.controlling() {
		return nil
	}
	info := connection.current()
	_, err := connection.owner.controller.Do(connection.ctx, protocol.Command{Kind: "resize_terminal", Data: protocol.CommandResizeTerminalPayload{Terminal: info.ID, Revision: info.Revision, Viewport: viewport}})
	if err != nil && connection.ctx.Err() == nil {
		connection.owner.Report(err)
	}
	return err
}

func (connection *terminalConn) UseTheme(theme *myterminal.Theme) error {
	color := func(value ui.Color) protocol.TerminalRgb {
		return protocol.TerminalRgb{Red: value.R, Green: value.G, Blue: value.B}
	}
	appearance := protocol.TerminalAppearance{Foreground: color(theme.Foreground), Background: color(theme.Background), ColorScheme: protocol.TerminalColorSchemeLight}
	if 299*int(theme.Background.R)+587*int(theme.Background.G)+114*int(theme.Background.B) < 128000 {
		appearance.ColorScheme = protocol.TerminalColorSchemeDark
	}
	for index, entry := range theme.Palette {
		appearance.Palette[index] = color(entry)
	}
	connection.mu.Lock()
	connection.appearance = &appearance
	connection.mu.Unlock()
	return connection.sendAppearance(appearance)
}

func (connection *terminalConn) sendAppearance(appearance protocol.TerminalAppearance) error {
	if !connection.controlling() {
		return nil
	}
	info := connection.current()
	_, err := connection.owner.controller.Do(connection.ctx, protocol.Command{Kind: "set_terminal_appearance", Data: protocol.CommandSetTerminalAppearancePayload{Terminal: info.ID, Revision: info.Revision, Appearance: appearance}})
	if err != nil && connection.ctx.Err() == nil {
		connection.owner.Report(err)
	}
	return err
}

// claimed records the ownership the Node granted and sends the size and
// appearance this view asked for while it was not the owner.
func (connection *terminalConn) claimed(info protocol.TerminalInfo) {
	connection.mu.Lock()
	if info.Revision > connection.info.Revision {
		connection.info = info
	}
	viewport, appearance := connection.viewport, connection.appearance
	connection.rejected = false
	connection.mu.Unlock()
	if viewport != nil {
		_ = connection.sendViewport(*viewport)
	}
	if appearance != nil {
		_ = connection.sendAppearance(*appearance)
	}
}

func (connection *terminalConn) clearScreen() {
	if !connection.controlling() {
		return
	}
	info := connection.current()
	connection.owner.async(func() {
		_, err := connection.owner.controller.Do(connection.ctx, protocol.Command{Kind: "input_terminal", Data: protocol.CommandInputTerminalPayload{Terminal: info.ID, Revision: info.Revision, Inputs: []protocol.TerminalInput{}, Clear: true}})
		if err != nil && connection.ctx.Err() == nil {
			connection.owner.Report(err)
		}
	})
}

func (connection *terminalConn) Close() error {
	var err error
	connection.close.Do(func() {
		connection.cancel()
		err = connection.stream.Close()
	})
	return err
}
