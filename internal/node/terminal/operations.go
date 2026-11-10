package terminal

import (
	"unicode"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func validateViewport(viewport protocol.TerminalViewport) error {
	if viewport.Columns < 1 || viewport.Columns > 500 || viewport.Rows < 1 || viewport.Rows > 200 {
		return fault(protocol.ErrorCodeInvalidRequest, "terminal viewport is outside the supported range")
	}
	return nil
}

func (value *handle) owns(caller protocol.NodeID, revision uint64) error {
	if value.info.Revision != revision {
		return fault(protocol.ErrorCodeRevisionConflict, "terminal ownership changed")
	}
	if value.info.Owner == nil || *value.info.Owner != caller {
		return fault(protocol.ErrorCodePermissionDenied, "terminal input belongs to another controller")
	}
	if value.info.Status.Kind != "running" {
		return fault(protocol.ErrorCodeConflict, "terminal process is not running")
	}
	return nil
}

func (service *Service) Claim(caller protocol.NodeID, id protocol.TerminalID, expected uint64) (protocol.TerminalInfo, error) {
	value, err := service.get(id)
	if err != nil {
		return protocol.TerminalInfo{}, err
	}
	value.mu.Lock()
	if value.info.Revision != expected {
		value.mu.Unlock()
		return protocol.TerminalInfo{}, fault(protocol.ErrorCodeRevisionConflict, "terminal ownership changed")
	}
	if value.info.Status.Kind != "running" {
		value.mu.Unlock()
		return protocol.TerminalInfo{}, fault(protocol.ErrorCodeConflict, "terminal process is not running")
	}
	if value.info.Owner == nil || *value.info.Owner != caller {
		value.info.Owner = &caller
		value.info.Revision++
	}
	info := value.info
	value.mu.Unlock()
	service.publish(info)
	return info, nil
}

func (service *Service) Input(caller protocol.NodeID, input protocol.CommandInputTerminalPayload) (protocol.TerminalInfo, error) {
	if len(input.Inputs) > 256 || len(input.Inputs) == 0 && len(input.Bytes) == 0 && !input.Clear {
		return protocol.TerminalInfo{}, fault(protocol.ErrorCodeInvalidRequest, "terminal input batch is empty or exceeds its limit")
	}
	size := len(input.Bytes)
	for _, entry := range input.Inputs {
		switch entry.Kind {
		case "text":
			text, valid := payload[protocol.TerminalInputTextPayload](entry.Data)
			if !valid || text.Text == "" || !utf8.ValidString(text.Text) {
				return protocol.TerminalInfo{}, fault(protocol.ErrorCodeInvalidRequest, "terminal input text is invalid")
			}
			for _, character := range text.Text {
				if unicode.IsControl(character) {
					return protocol.TerminalInfo{}, fault(protocol.ErrorCodeInvalidRequest, "terminal input text contains control characters")
				}
			}
			size += len(text.Text)
		case "paste":
			text, valid := payload[protocol.TerminalInputPastePayload](entry.Data)
			if !valid || text.Text == "" || !utf8.ValidString(text.Text) {
				return protocol.TerminalInfo{}, fault(protocol.ErrorCodeInvalidRequest, "terminal paste is invalid")
			}
			size += len(text.Text)
		default:
			return protocol.TerminalInfo{}, fault(protocol.ErrorCodeNotConfigured, "structured terminal input requires the terminal controller adapter")
		}
	}
	if size > 64*1024 {
		return protocol.TerminalInfo{}, fault(protocol.ErrorCodeInvalidRequest, "terminal input exceeds 65536 bytes")
	}
	value, err := service.get(input.Terminal)
	if err != nil {
		return protocol.TerminalInfo{}, err
	}
	value.mu.Lock()
	defer value.mu.Unlock()
	if err := value.owns(caller, input.Revision); err != nil {
		return protocol.TerminalInfo{}, err
	}
	for _, entry := range input.Inputs {
		if entry.Kind == "text" {
			text, _ := payload[protocol.TerminalInputTextPayload](entry.Data)
			value.terminal.Send([]byte(text.Text))
		} else {
			text, _ := payload[protocol.TerminalInputPastePayload](entry.Data)
			value.terminal.Paste(text.Text)
		}
	}
	value.terminal.Send(input.Bytes)
	if input.Clear {
		value.terminal.ClearScreen()
	}
	return value.info, nil
}

func (service *Service) Resize(caller protocol.NodeID, input protocol.CommandResizeTerminalPayload) (protocol.TerminalInfo, error) {
	if err := validateViewport(input.Viewport); err != nil {
		return protocol.TerminalInfo{}, err
	}
	value, err := service.get(input.Terminal)
	if err != nil {
		return protocol.TerminalInfo{}, err
	}
	value.mu.Lock()
	defer value.mu.Unlock()
	if err := value.owns(caller, input.Revision); err != nil {
		return protocol.TerminalInfo{}, err
	}
	value.terminal.ResizePixels(int(input.Viewport.Columns), int(input.Viewport.Rows), cellPixels(input.Viewport.PixelWidth, input.Viewport.Columns), cellPixels(input.Viewport.PixelHeight, input.Viewport.Rows))
	value.viewport = input.Viewport
	return value.info, nil
}

func (service *Service) Appearance(caller protocol.NodeID, input protocol.CommandSetTerminalAppearancePayload) (protocol.TerminalInfo, error) {
	value, err := service.get(input.Terminal)
	if err != nil {
		return protocol.TerminalInfo{}, err
	}
	value.mu.Lock()
	defer value.mu.Unlock()
	if err := value.owns(caller, input.Revision); err != nil {
		return protocol.TerminalInfo{}, err
	}
	value.terminal.UseTheme(terminalTheme(input.Appearance))
	return value.info, nil
}

func (service *Service) CloseTerminal(worktree protocol.WorktreeID, id protocol.TerminalID) (protocol.TerminalInfo, error) {
	value, err := service.get(id)
	if err != nil {
		return protocol.TerminalInfo{}, err
	}
	value.mu.Lock()
	if value.info.Worktree == nil || *value.info.Worktree != worktree {
		value.mu.Unlock()
		return protocol.TerminalInfo{}, fault(protocol.ErrorCodeWrongTarget, "terminal belongs to another worktree")
	}
	value.info.Status = protocol.TerminalStatus{Kind: "closed"}
	value.info.Owner = nil
	value.info.Revision++
	info := value.info
	value.mu.Unlock()
	err = value.terminal.Close()
	service.mu.Lock()
	delete(service.handles, id)
	service.mu.Unlock()
	service.publish(info)
	return info, err
}

func payload[Value any](data any) (Value, bool) {
	if value, valid := data.(Value); valid {
		return value, true
	}
	if value, valid := data.(*Value); valid && value != nil {
		return *value, true
	}
	var empty Value
	return empty, false
}
