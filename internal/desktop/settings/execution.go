package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// ExecutionSetting is one Node-owned contribution to Conversation settings.
type ExecutionSetting interface {
	Read()
	Render(*ui.Context)
}

// ExecutionSettingHost routes reads and saves to one settings Node. A fixture
// or a shell without a live binding has no command callbacks.
type ExecutionSettingHost struct {
	Preview bool
	Execute func(protocol.Command, func(*ui.Context, protocol.Output, error))
	Save    func(protocol.Command, Messages, func(protocol.Output, bool))
	Report  func(*ui.Context, error, Messages)
}

// ExecutionSettingFactory builds a fresh setting for a window and settings Node.
type ExecutionSettingFactory func(ExecutionSettingHost) ExecutionSetting

func BindExecutionSettings(factories []ExecutionSettingFactory, host ExecutionSettingHost) []ExecutionSetting {
	if len(factories) == 0 {
		return nil
	}
	bindings := make([]ExecutionSetting, 0, len(factories))
	for _, bind := range factories {
		bindings = append(bindings, bind(host))
	}
	return bindings
}
