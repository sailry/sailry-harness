package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

type ConfigurationTargetStatus struct {
	ID, Name, Failure string
}
type ConfigurationFollowIssue struct {
	Category, ID, State, Message, MessageKey string
	Authorize                                bool
}
type ConfigurationFollowEditor struct {
	Device                                           viewmodel.Device
	Source                                           string
	Targets                                          []ConfigurationTargetStatus
	Issues                                           []ConfigurationFollowIssue
	Local, Loading, Pending, ReadFailed, Unavailable bool
}
type ConfigurationFollowActions struct {
	Open      func(viewmodel.Device)
	Refresh   func(*ConfigurationFollowEditor)
	Authorize func(*ConfigurationFollowEditor, ConfigurationFollowIssue)
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
				kit.Shimmer(c, t("configuration_follow_loading"), 14, theme.Of(c).MutedForeground)
				return
			}
			kit.Card(c, func() {
				if e.ReadFailed {
					ui.Text(c, t("configuration_follow_unavailable")).FontSize(14).TextColor(theme.Of(c).MutedForeground)
				} else if e.Local {
					kit.Row(c, "local", t("configuration_follow_this_device"), t("configuration_follow_local_description"), nil)
				} else if e.Source != "" {
					kit.Row(c, "controller", t("configuration_follow_controller"), e.Source, nil)
					ui.Text(c, t("configuration_follow_description")).FontSize(14).TextColor(theme.Of(c).MutedForeground)
				} else {
					kit.Row(c, "waiting", t("configuration_follow_waiting"), t("configuration_follow_description"), nil)
				}
			})
			if len(e.Targets) > 0 {
				kit.Group(c, "targets", t("configuration_follow_targets"), "", nil, func() {
					kit.Card(c, func() {
						for _, target := range e.Targets {
							status := target.Failure
							if status == "" {
								status = t("configuration_follow_automatic")
							}
							kit.Row(c, target.ID, target.Name, status, nil)
						}
					})
				})
			}
			if len(e.Issues) > 0 {
				kit.Group(c, "issues", t("configuration_follow_issues"), "", nil, func() {
					kit.Card(c, func() {
						for _, issue := range e.Issues {
							message := issue.Message
							if issue.MessageKey != "" {
								message = t(issue.MessageKey)
							}
							kit.Row(c, issue.Category+"/"+issue.ID, issue.Category+" / "+issue.ID, message, func() {
								if issue.Authorize {
									kit.Secondary(c, t("configuration_follow_authorize")).Disabled(e.Pending || e.Unavailable).OnClick(func() {
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
			kit.Secondary(c.Key("refresh"), t("settings_retry")).Disabled(e.Pending || e.Loading).OnClick(func() {
				if m.FollowActions != nil && m.FollowActions.Refresh != nil {
					m.FollowActions.Refresh(e)
				}
			})
			kit.Secondary(c.Key("close"), t("close")).Disabled(e.Pending).OnClick(func() { m.FollowEditor = nil })
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
