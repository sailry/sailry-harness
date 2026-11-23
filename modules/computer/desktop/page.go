package desktop

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

func Page() settings.NativePage {
	return settings.NativePage{
		Section: settings.Computer,
		Group:   "settings_group_tools",
		Icon:    icons.Monitor,
		Order:   900,
		Render:  render,
	}
}

func render(c *ui.Context, s *settings.State) {
	m := &s.Computer
	act := s.ComputerActions
	t := func(key string) string { return i18n.T(c, key) }

	if m.Status != "" {
		kit.Result(c, kit.ResultProps{Title: t(m.Status), Card: true})
		return
	}

	kit.Group(c, "computer_settings", t("computer_settings"), "", nil, func() {
		kit.Card(c, func() {
			desc := t("computer_enabled_description")
			if m.Loaded && !m.Available {
				desc = t("computer_helper_unavailable")
			}
			disabled := !m.Loaded || m.Enabled.Pending || (!m.Available && !m.Enabled.Draft)
			kit.Row(c, "enabled", t("computer_enabled"), desc, func() {
				if ui.Switch(c.Key("switch"), &m.Enabled.Draft).
					Label(t("computer_enabled")).
					Disabled(disabled).
					Changed() {
					if s.Live && act != nil && act.Save != nil {
						act.Save()
					} else {
						m.Enabled.Commit()
					}
				}
			})
		})
	})

	if !m.Enabled.Saved {
		return
	}

	kit.Group(c, "computer_permissions", t("computer_permissions_section"), "", func() {
		if m.Checking {
			kit.BusyAction(c.Key("check"), t("computer_permissions_refresh"))
			return
		}
		kit.Action(c.Key("check"), icons.Reload, t("computer_permissions_refresh"), !m.Loaded).OnClick(func() {
			if s.Live && act != nil && act.CheckPermissions != nil {
				act.CheckPermissions()
			} else {
				kit.Message(c, t("preview_notice"))
			}
		})
	}, func() {
		if len(m.Permissions) == 0 {
			kit.Result(c, kit.ResultProps{Icon: icons.Inbox, Title: t("settings_empty"), Card: true})
			return
		}
		kit.Card(c, func() {
			for _, perm := range m.Permissions {
				perm := perm
				granted := perm.Status == "permission_granted" || perm.Status == "permission_not_needed"
				absent := perm.Status == "permission_unavailable" || perm.Status == "permission_remote"
				kit.Row(c, perm.Resource, t(perm.Resource), t(perm.Status), func() {
					if m.HostReadOnly || act == nil {
						return
					}
					if granted && perm.Settings && act.OpenSettings != nil {
						kit.Action(c.Key("settings"), icons.Settings2, t("permission_settings"), false).OnClick(func() {
							act.OpenSettings(perm.Resource)
						})
					}
					if !granted && act.RequestPermission != nil {
						actionKey := "permission_request"
						if perm.Settings {
							actionKey = "permission_settings"
						}
						kit.Secondary(c.Key("request"), t(actionKey)).Height(28).Disabled(absent).OnClick(func() {
							if perm.Settings && act.OpenSettings != nil {
								act.OpenSettings(perm.Resource)
							} else {
								act.RequestPermission(perm.Resource)
							}
						})
					}
				})
			}
		})
		if m.HostReadOnly {
			kit.Alert(c, "remote-help", kit.Info, t("computer_permissions_remote"), nil)
		} else if slices.ContainsFunc(m.Permissions, func(x preview.Permission) bool {
			return x.Status != "permission_granted" && x.Status != "permission_not_needed" && x.Status != "permission_remote" && x.Status != "permission_unavailable"
		}) {
			kit.Alert(c, "disk-help", kit.Info, t("permission_disk_help"), func() {
				kit.TextAction(c.Key("show-app"), t("permission_show_app"), false).OnClick(func() {
					if act != nil && act.ShowApp != nil {
						act.ShowApp()
					}
				})
			})
		}
	})
}
