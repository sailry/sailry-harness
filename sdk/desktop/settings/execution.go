package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

type ExecutionSetting = settings.ExecutionSetting
type ExecutionSettingFactory = settings.ExecutionSettingFactory
type ExecutionSettingHost = settings.ExecutionSettingHost
type Stored[T any] = settings.Stored[T]
type Messages = settings.Messages

const Conversation = settings.Conversation

func Report(c *ui.Context, outcome kit.Outcome, messages Messages) {
	settings.Report(c, outcome, messages)
}

func Switch(c *ui.Context, key any, title, description string, on *bool, disabled bool) bool {
	return settings.Switch(c, key, title, description, on, disabled)
}
