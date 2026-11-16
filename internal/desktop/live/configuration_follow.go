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
		editor.model.Unavailable = editor.owner == nil
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
				r.projectConfigurationFollow(editor.model, status, editor.controller.Snapshot().Providers)
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
				editor.model.Unavailable = false
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
		Authorize: func(view *settings.ConfigurationFollowEditor, issue settings.ConfigurationFollowIssue) {
			if current == nil || current.model != view {
				return
			}
			current.owner = r.runtimeFor(view.Device.ID)
			if current.owner == nil {
				return
			}
			r.App.Host = view.Device.ID
			r.showNodeSettings(current.owner)
			model.FollowEditor = nil
			switch issue.Category {
			case "providers":
				r.App.Settings.Open(settings.Providers)
				current.owner.beginProviderLogin(issue.ID)
			case "connections":
				r.App.Settings.Open(settings.Connectors)
				model.AuthorizeFollowedConnection(issue.ID)
			case "plugin_settings":
				r.App.Settings.Open(settings.Connectors)
				current.owner.configurePackage(issue.ID)
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

func (r *Runtime) projectConfigurationFollow(view *settings.ConfigurationFollowEditor, status protocol.ConfigurationFollowStatus, providers []protocol.ConversationProvider) {
	view.Local = status.User != nil && status.User.Local
	view.Source = ""
	if status.Source != nil {
		view.Source = r.configurationHostName(status.Source.String())
	}
	view.Targets = nil
	for _, target := range status.Targets {
		value := settings.ConfigurationTargetStatus{ID: target.Host.String(), Name: r.configurationHostName(target.Host.String())}
		if target.Error != nil {
			value.Failure = target.Error.Message
		}
		view.Targets = append(view.Targets, value)
	}
	view.Issues = nil
	for _, issue := range status.Issues {
		value := settings.ConfigurationFollowIssue{Category: issue.Category, ID: issue.ID, State: issue.State, Message: issue.Message}
		if issue.State == "authorization" {
			switch issue.Category {
			case "providers":
				for _, provider := range providers {
					if provider.ID.String() != issue.ID {
						continue
					}
					if provider.Authentication == protocol.AuthenticationApiKey {
						value.MessageKey = "configuration_follow_missing_key"
					} else {
						value.Authorize = provider.Authentication != ""
					}
					break
				}
			case "connections", "plugin_settings":
				value.Authorize = true
			}
		}
		view.Issues = append(view.Issues, value)
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
