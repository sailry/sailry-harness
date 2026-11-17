package live

import (
	"encoding/hex"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type configurationFollowEditor struct {
	model      *settings.ConfigurationFollowEditor
	owner      *Runtime
	controller *client.Client
	retry      func()
}

func (r *Runtime) initConfigurationFollowActions() {
	if r.parent != nil {
		return
	}
	var current *configurationFollowEditor
	model := r.App.Settings.Model
	var refresh func(*configurationFollowEditor)
	refresh = func(editor *configurationFollowEditor) {
		if editor.model.Pending || editor.model.Loading {
			return
		}
		editor.owner = r.runtimeFor(editor.model.Device.ID)
		if editor.owner != nil {
			editor.controller = editor.owner.controller
		}
		editor.model.Loading = true
		r.async(func() {
			status, err := editor.controller.ReadConfigurationFollow(r.ctx)
			r.enqueue(func(c *ui.Context) {
				if current != editor || model.FollowEditor != editor.model {
					return
				}
				editor.model.Loading = false
				editor.model.ReadFailed = err != nil
				if err != nil {
					r.reportKeys(c, err, "configuration_follow_failed", "configuration_follow_conflict", "configuration_follow_unknown")
					return
				}
				r.projectConfigurationFollow(editor.model, status)
			})
		})
	}
	model.FollowActions = &settings.ConfigurationFollowActions{
		Open: func(device viewmodel.Device) {
			var controller *client.Client
			if owner := r.runtimeFor(device.ID); owner != nil {
				controller = owner.controller
			} else {
				var err error
				controller, err = r.connectionClient(device)
				if err != nil {
					r.Report(err)
					return
				}
			}
			editor := &configurationFollowEditor{model: &settings.ConfigurationFollowEditor{Device: device}, controller: controller}
			current = editor
			model.FollowEditor = editor.model
			// The selected Host retains an uncertain sync request, so retrying
			// through its Runtime preserves the original request identity.
			editor.retry = func() {
				if editor.model.Pending || editor.model.Loading {
					return
				}
				editor.owner = r.runtimeFor(device.ID)
				if editor.owner == nil {
					refresh(editor)
					return
				}
				editor.model.Pending = true
				editor.owner.execute(protocol.Command{Kind: "configuration_follow", Data: &protocol.ConfigurationFollowCommand{Action: "sync"}}, func(c *ui.Context, output protocol.Output, err error) {
					editor.model.Pending = false
					if current != editor || model.FollowEditor != editor.model {
						return
					}
					if err != nil {
						r.reportKeys(c, err, "configuration_follow_failed", "configuration_follow_conflict", "configuration_follow_unknown")
						return
					}
					kit.Toast(c, kit.Done, i18n.T(c, "configuration_follow_requested"))
					refresh(editor)
				})
			}
			refresh(editor)
		},
		Refresh: func(view *settings.ConfigurationFollowEditor) {
			if current != nil && current.model == view {
				current.retry()
			}
		},
	}
}

func configurationNodeID(value string) (protocol.NodeID, bool) {
	raw, err := hex.DecodeString(value)
	if err != nil || len(raw) != 32 {
		return protocol.NodeID{}, false
	}
	return protocol.NodeID(raw), true
}

func (r *Runtime) projectConfigurationFollow(view *settings.ConfigurationFollowEditor, status protocol.ConfigurationFollowStatus) {
	view.Local = status.User != nil && status.User.Local
	view.Source = ""
	source := status.Source
	if status.User != nil && status.User.Controller != nil {
		source = status.User.Controller
	}
	if source != nil && !view.Local {
		view.Source = r.configurationHostName(source.String())
	}
	view.Targets = nil
	for _, target := range status.Targets {
		value := settings.ConfigurationTargetStatus{ID: target.Host.String(), Name: r.configurationHostName(target.Host.String())}
		if target.Error != nil {
			value.Failure = target.Error.Message
		}
		view.Targets = append(view.Targets, value)
	}
}
func (r *Runtime) configurationHostName(id string) string {
	for _, device := range r.App.Settings.Devices {
		if device.ID == id && device.Name != "" {
			return device.Name + " · " + id[:min(8, len(id))]
		}
	}
	return id[:min(12, len(id))]
}
