package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

type ConfigurationSourceChoice struct {
	ID, Name string
	LabelKey string
}
type ConfigurationTargetChoice struct {
	ID, Name               string
	Revision               uint64
	Configuration, Secrets bool
	Failure                string
}
type ConfigurationFollowIssue struct {
	Category, ID, State, Message string
	Authorize                    bool
}
type ConfigurationMCPLocal struct {
	Plugin, Server string
	Local          bool
}
type ConfigurationFollowEditor struct {
	Device                                          viewmodel.Device
	Revision                                        uint64
	Source                                          int
	Sources                                         []ConfigurationSourceChoice
	Targets                                         []ConfigurationTargetChoice
	Issues                                          []ConfigurationFollowIssue
	MCP                                             []ConfigurationMCPLocal
	Loading, Pending, Locked, Disabled, Unavailable bool
}
type ConfigurationFollowActions struct {
	Open      func(viewmodel.Device)
	Refresh   func(*ConfigurationFollowEditor)
	Source    func(*ConfigurationFollowEditor)
	Target    func(*ConfigurationFollowEditor, int)
	MCPLocal  func(*ConfigurationFollowEditor, int)
	Authorize func(*ConfigurationFollowEditor, ConfigurationFollowIssue)
	Detach    func(*ConfigurationFollowEditor)
}

func configurationFollowDialog(c *ui.Context, m *Model) {
	e := m.FollowEditor
	if e == nil {
		return
	}
	open := true
	t := func(key string) string { return i18n.T(c, key) }
	title := t("configuration_follow_title")
	kit.Dialog(c.Key("configuration-follow"), &open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		ui.Text(c, deviceName(c, e.Device)).FontSize(14).TextColor(theme.Of(c).MutedForeground)
		ui.Scroll(c.Key("body")).MaxHeight(500).Gap(16).Children(func() {
			if e.Loading {
				kit.Shimmer(c, t("host_runtime_loading"), 14, theme.Of(c).MutedForeground)
				return
			}
			names := make([]string, len(e.Sources))
			for i, choice := range e.Sources {
				names[i] = choice.Name
				if choice.LabelKey != "" {
					names[i] = t(choice.LabelKey)
				}
			}
			kit.Card(c, func() {
				kit.Row(c, "source", t("configuration_follow_source"), t("configuration_follow_source_description"), func() {
					if pick(c, "source", t("configuration_follow_source"), &e.Source, names, e.Pending || e.Locked || e.Unavailable).Changed() && m.FollowActions != nil && m.FollowActions.Source != nil {
						m.FollowActions.Source(e)
					}
				})
			})
			if len(e.Targets) > 0 {
				kit.Group(c, "targets", t("configuration_follow_targets"), "", nil, func() {
					for i := range e.Targets {
						target := &e.Targets[i]
						kit.Card(c.Key(target.ID), func() {
							ui.Text(c, target.Name).FontSize(14).FontWeight(600)
							if settingSwitch(c, "configuration", t("configuration_follow_configuration"), "", &target.Configuration, e.Pending || e.Locked || e.Unavailable) && m.FollowActions != nil && m.FollowActions.Target != nil {
								m.FollowActions.Target(e, i)
							}
							if settingSwitch(c, "secrets", t("configuration_follow_secrets"), t("configuration_follow_secrets_description"), &target.Secrets, e.Pending || e.Locked || e.Unavailable) && m.FollowActions != nil && m.FollowActions.Target != nil {
								m.FollowActions.Target(e, i)
							}
							if target.Failure != "" {
								ui.Text(c, target.Failure).FontSize(14).TextColor(theme.Of(c).MutedForeground)
							}
						})
					}
				})
			}
			if len(e.MCP) > 0 {
				kit.Group(c, "mcp", t("configuration_follow_mcp"), t("configuration_follow_mcp_description"), nil, func() {
					kit.Card(c, func() {
						for i := range e.MCP {
							server := &e.MCP[i]
							if settingSwitch(c, server.Plugin+"/"+server.Server, server.Plugin+" / "+server.Server, "", &server.Local, e.Pending || e.Locked || e.Unavailable) && m.FollowActions != nil && m.FollowActions.MCPLocal != nil {
								m.FollowActions.MCPLocal(e, i)
							}
						}
					})
				})
			}
			if len(e.Issues) > 0 {
				kit.Group(c, "issues", t("configuration_follow_issues"), "", nil, func() {
					kit.Card(c, func() {
						for _, issue := range e.Issues {
							kit.Row(c, issue.Category+"/"+issue.ID, issue.Category+" / "+issue.ID, issue.Message, func() {
								if issue.Authorize {
									kit.Secondary(c, t("configuration_follow_authorize")).Disabled(e.Pending).OnClick(func() {
										if m.FollowActions != nil && m.FollowActions.Authorize != nil {
											m.FollowActions.Authorize(e, issue)
										}
									})
								}
							})
						}
					})
				})
			}
		})
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			if e.Disabled {
				kit.Secondary(c.Key("detach"), t("configuration_follow_detach")).Disabled(e.Pending || e.Locked || e.Unavailable).OnClick(func() {
					if m.FollowActions != nil && m.FollowActions.Detach != nil {
						m.FollowActions.Detach(e)
					}
				})
			}
			kit.Secondary(c.Key("refresh"), t("settings_retry")).Disabled(e.Pending || e.Loading).OnClick(func() {
				if m.FollowActions != nil && m.FollowActions.Refresh != nil {
					m.FollowActions.Refresh(e)
				}
			})
			kit.Secondary(c.Key("close"), t("settings_cancel")).Disabled(e.Pending).OnClick(func() { m.FollowEditor = nil })
		})
	}).Width(min(560, width(c)-48)).Gap(16).Label(title)
	if !open && !e.Pending {
		m.FollowEditor = nil
	}
}

// AuthorizeFollowedConnection opens the existing connection form on this host.
func (m *Model) AuthorizeFollowedConnection(id string) {
	for i := range m.Services {
		if m.Services[i].ID == id {
			m.editService(&m.Services[i])
			return
		}
	}
}
