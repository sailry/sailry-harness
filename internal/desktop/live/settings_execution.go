package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) bindExecutionSettings() {
	factories := r.localRuntime().executionSettingFactories
	if len(factories) == 0 {
		return
	}
	r.executionSettings = settings.BindExecutionSettings(factories, settings.ExecutionSettingHost{
		Execute: r.execute,
		Save: func(command protocol.Command, messages settings.Messages, done func(protocol.Output, bool)) {
			if !r.showsSettings() || r.machineSettingsStatus() != "" {
				done(protocol.Output{}, false)
				return
			}
			if !r.controller.Connected() {
				r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the settings host is unavailable"))
				done(protocol.Output{}, false)
				return
			}
			r.saveSetting(command, messages, done)
		},
		Report: func(c *ui.Context, err error, messages settings.Messages) {
			r.reportKeys(c, err, messages.Failed, messages.Conflict, messages.Uncertain)
		},
	})
}

func (r *Runtime) readExecutionSettings() {
	for _, setting := range r.executionSettings {
		setting.Read()
	}
}
