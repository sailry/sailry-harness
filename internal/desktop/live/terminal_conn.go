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
		connection.info = info
		connection.mu.Unlock()
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

func (connection *terminalConn) Write(source []byte) (int, error) {
	if len(source) == 0 {
		return 0, nil
	}
	info := connection.current()
	_, err := connection.owner.controller.Do(connection.ctx, protocol.Command{Kind: "input_terminal", Data: protocol.CommandInputTerminalPayload{Terminal: info.ID, Revision: info.Revision, Bytes: append(protocol.Bytes(nil), source...), Inputs: []protocol.TerminalInput{}}})
	if err != nil {
		if connection.ctx.Err() == nil {
			connection.owner.Report(err)
		}
		return 0, err
	}
	return len(source), nil
}

func (connection *terminalConn) Resize(cols, rows int) error {
	return connection.ResizePixels(cols, rows, 0, 0)
}

func (connection *terminalConn) ResizePixels(cols, rows, cellWidth, cellHeight int) error {
	info := connection.current()
	cols, rows = min(max(cols, 1), 500), min(max(rows, 1), 200)
	viewport := protocol.TerminalViewport{Columns: uint16(cols), Rows: uint16(rows), PixelWidth: uint32(cols * max(cellWidth, 0)), PixelHeight: uint32(rows * max(cellHeight, 0))}
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
	info := connection.current()
	_, err := connection.owner.controller.Do(connection.ctx, protocol.Command{Kind: "set_terminal_appearance", Data: protocol.CommandSetTerminalAppearancePayload{Terminal: info.ID, Revision: info.Revision, Appearance: appearance}})
	if err != nil && connection.ctx.Err() == nil {
		connection.owner.Report(err)
	}
	return err
}

func (connection *terminalConn) clearScreen() {
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
