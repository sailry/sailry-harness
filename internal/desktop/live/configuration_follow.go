package live

import (
	"encoding/hex"
	"slices"

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
	change     *protocol.ConfigurationFollowCommand
	mutate     func(protocol.ConfigurationFollowCommand)
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
		editor.model.Loading = true
		r.async(func() {
			status, err := editor.controller.ReadConfigurationFollow(r.ctx)
			var identity protocol.IdentityState
			var packages []protocol.PluginInfo
			if err == nil {
				controller := editor.controller
				out, readErr := controller.Do(r.ctx, protocol.Command{Kind: "identity", Data: protocol.IdentityCommand{Kind: "read_identity"}})
				if readErr == nil {
					identity, _ = detail[protocol.IdentityState](out.Data)
				}
				if editor.owner != nil {
					for _, summary := range editor.owner.snapshot().Plugins {
						out, readErr := controller.Do(r.ctx, protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: summary.Name}})
						if info, ok := detail[protocol.PluginInfo](out.Data); readErr == nil && ok {
							packages = append(packages, info)
						}
					}
				}
			}
			r.enqueue(func(c *ui.Context) {
				if current != editor || model.FollowEditor != editor.model {
					return
				}
				editor.model.Loading = false
				if err != nil {
					r.reportKeys(c, err, "configuration_follow_failed", "configuration_follow_conflict", "configuration_follow_unknown")
					return
				}
				r.projectConfigurationFollow(editor.model, status, identity, packages)
			})
		})
	}
	model.FollowActions = &settings.ConfigurationFollowActions{
		Open: func(device viewmodel.Device) {
			controller, err := r.connectionClient(device)
			if err != nil {
				r.Report(err)
				return
			}
			editor := &configurationFollowEditor{model: &settings.ConfigurationFollowEditor{Device: device}, controller: controller, owner: r.runtimeFor(device.ID)}
			editor.model.Unavailable = editor.owner == nil
			current = editor
			model.FollowEditor = editor.model
			// Connected host runtimes own uncertain requests and route saves to their
			// own authenticated Client. A disappearing host keeps this editor's draft.
			editor.mutate = func(command protocol.ConfigurationFollowCommand) {
				if editor.owner == nil || editor.model.Pending {
					return
				}
				editor.change = &command
				editor.model.Pending = true
				editor.owner.execute(protocol.Command{Kind: "configuration_follow", Data: &command}, func(c *ui.Context, output protocol.Output, err error) {
					editor.model.Pending = false
					editor.model.Locked = faultCode(err) == protocol.ErrorCodeOutcomeUnknown || client.Retained(err)
					if err != nil {
						if !editor.model.Locked {
							editor.change = nil
						}
						r.reportKeys(c, err, "configuration_follow_failed", "configuration_follow_conflict", "configuration_follow_unknown")
						return
					}
					editor.change = nil
					if current != editor || model.FollowEditor != editor.model {
						return
					}
					kit.Toast(c, kit.Done, i18n.T(c, "configuration_follow_saved"))
					refresh(editor)
				})
			}
			refresh(editor)
		},
		Refresh: func(view *settings.ConfigurationFollowEditor) {
			if current != nil && current.model == view {
				if current.model.Locked && current.change != nil {
					current.mutate(*current.change)
				} else {
					refresh(current)
				}
			}
		},
		Source: func(view *settings.ConfigurationFollowEditor) {
			if current == nil || current.model != view || view.Source < 0 || view.Source >= len(view.Sources) {
				return
			}
			var source *protocol.NodeID
			if raw := view.Sources[view.Source].ID; raw != "" {
				id, ok := configurationNodeID(raw)
				if !ok {
					return
				}
				source = &id
			}
			current.mutate(protocol.ConfigurationFollowCommand{Action: "set_source", Source: source, ExpectedRevision: view.Revision})
		},
		Target: func(view *settings.ConfigurationFollowEditor, index int) {
			if current == nil || current.model != view || index < 0 || index >= len(view.Targets) {
				return
			}
			choice := view.Targets[index]
			id, ok := configurationNodeID(choice.ID)
			if !ok {
				return
			}
			current.mutate(protocol.ConfigurationFollowCommand{Action: "set_target", ExpectedRevision: choice.Revision, Target: &protocol.ConfigurationFollowTarget{Host: id, Configuration: choice.Configuration, Secrets: choice.Secrets}})
		},
		MCPLocal: func(view *settings.ConfigurationFollowEditor, index int) {
			if current == nil || current.model != view || index < 0 || index >= len(view.MCP) {
				return
			}
			choice := view.MCP[index]
			current.mutate(protocol.ConfigurationFollowCommand{Action: "mcp_local", Plugin: choice.Plugin, Server: choice.Server, Local: choice.Local})
		},
		Detach: func(view *settings.ConfigurationFollowEditor) {
			if current != nil && current.model == view {
				current.mutate(protocol.ConfigurationFollowCommand{Action: "detach"})
			}
		},
		Authorize: func(view *settings.ConfigurationFollowEditor, issue settings.ConfigurationFollowIssue) {
			if current == nil || current.model != view || current.owner == nil {
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

func (r *Runtime) projectConfigurationFollow(view *settings.ConfigurationFollowEditor, status protocol.ConfigurationFollowStatus, identity protocol.IdentityState, packages []protocol.PluginInfo) {
	view.Revision = status.Revision
	view.Disabled = status.Source == nil && status.Revision != 0
	view.Sources = []settings.ConfigurationSourceChoice{{LabelKey: "configuration_follow_disabled"}}
	if status.User != nil && status.User.Local {
		view.Sources = append(view.Sources, settings.ConfigurationSourceChoice{ID: view.Device.ID, LabelKey: "configuration_follow_this_host"})
	}
	for _, member := range identity.Devices {
		if member.User != identity.User.ID || member.Device.String() == view.Device.ID || member.Direction == "host" {
			continue
		}
		id := member.Device.String()
		if slices.ContainsFunc(view.Sources, func(choice settings.ConfigurationSourceChoice) bool { return choice.ID == id }) {
			continue
		}
		view.Sources = append(view.Sources, settings.ConfigurationSourceChoice{ID: id, Name: r.configurationHostName(id)})
	}
	view.Source = 0
	chosen := ""
	if status.Source != nil {
		chosen = status.Source.String()
	} else if status.Revision == 0 && status.User != nil && status.User.Local {
		chosen = view.Device.ID
	}
	for i, choice := range view.Sources {
		if choice.ID == chosen {
			view.Source = i
		}
	}
	if chosen != "" && !slices.ContainsFunc(view.Sources, func(choice settings.ConfigurationSourceChoice) bool { return choice.ID == chosen }) {
		view.Sources = append(view.Sources, settings.ConfigurationSourceChoice{ID: chosen, Name: r.configurationHostName(chosen)})
		view.Source = len(view.Sources) - 1
	}
	view.Targets = nil
	for _, target := range status.Targets {
		value := settings.ConfigurationTargetChoice{ID: target.Host.String(), Name: r.configurationHostName(target.Host.String()), Revision: target.Revision, Configuration: target.Configuration, Secrets: target.Secrets}
		if target.Error != nil {
			value.Failure = target.Error.Message
		}
		view.Targets = append(view.Targets, value)
	}
	view.Issues = nil
	for _, issue := range status.Issues {
		view.Issues = append(view.Issues, settings.ConfigurationFollowIssue{Category: issue.Category, ID: issue.ID, State: issue.State, Message: issue.Message, Authorize: issue.State == "authorization" && (issue.Category == "providers" || issue.Category == "connections" || issue.Category == "plugin_settings")})
	}
	view.MCP = nil
	for _, info := range packages {
		if info.Summary.Source == nil {
			continue
		}
		for _, server := range info.Mcp {
			local := slices.ContainsFunc(status.MCPLocal, func(choice protocol.ConfigurationMCPLocal) bool {
				return choice.Plugin == info.Summary.Name && choice.Server == server.Name
			})
			view.MCP = append(view.MCP, settings.ConfigurationMCPLocal{Plugin: info.Summary.Name, Server: server.Name, Local: local})
		}
	}
	slices.SortFunc(view.MCP, func(a, b settings.ConfigurationMCPLocal) int {
		if a.Plugin < b.Plugin {
			return -1
		}
		if a.Plugin > b.Plugin {
			return 1
		}
		if a.Server < b.Server {
			return -1
		}
		if a.Server > b.Server {
			return 1
		}
		return 0
	})
}
func (r *Runtime) configurationHostName(id string) string {
	for _, device := range r.projectDevices() {
		if device.ID == id && device.Name != "" {
			return device.Name + " · " + id[:min(8, len(id))]
		}
	}
	return id[:min(12, len(id))]
}
