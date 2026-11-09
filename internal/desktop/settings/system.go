package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// permissions is each permission with its status, its system settings and the
// request, the check of them all, and the full disk access help while it is
// not granted. Asking the system and opening its settings are the system's,
// which the preview does not do.
func permissions(c *ui.Context, m *Model) {
	kit.Group(c, "permissions", i18n.T(c, "permission_title"), "", func() {
		kit.Action(c.Key("check"), icons.Reload, i18n.T(c, "permission_check"), false).OnClick(func() { notice(c) })
	}, func() {
		kit.Card(c, func() {
			for _, x := range m.Permissions {
				granted := x.Status == "permission_granted" || x.Status == "permission_not_needed"
				kit.Row(c, x.Resource, i18n.T(c, x.Resource), i18n.T(c, x.Status), func() {
					if x.Settings && (x.Resource != "permission_full_disk" || granted) {
						kit.Action(c.Key("settings"), icons.Settings2, i18n.T(c, "permission_settings"), false).OnClick(func() { notice(c) })
					}
					if !granted {
						action := "permission_request"
						if x.Settings {
							action = "permission_settings"
						}
						small(c, "request", i18n.T(c, action)).OnClick(func() { notice(c) })
					}
				})
			}
		})
	})
	for _, x := range m.Permissions {
		if x.Resource != "permission_full_disk" || x.Status == "permission_granted" {
			continue
		}
		kit.Alert(c, "disk-help", kit.Info, i18n.T(c, "permission_disk_help"), func() {
			label := i18n.T(c, "permission_show_app")
			ghostIcon(c, icons.FolderOpen, label).OnClick(func() { notice(c) })
		})
	}
}

// ghostIcon is a small ghost button with an icon and a label.
func ghostIcon(c *ui.Context, icon *ui.SVG, label string) ui.Element {
	return kit.GhostLook(c, ui.ButtonBase(c.Key(label)).Label(label), false).Height(28).PaddingX(8).Gap(6).Shrink(0).Children(func() {
		ui.Icon(c, icon).FontSize(14)
		ui.Text(c, label).FontSize(14).SingleLine()
	})
}

// about is the version, the build and the license, then the updater, whose
// check reaches the network.
func about(c *ui.Context, m *Model) {
	p := theme.Of(c)
	text := func(s string) func() {
		return func() { ui.Text(c, s).FontSize(14).Selectable().TextColor(p.MutedForeground) }
	}
	kit.Group(c, "app", i18n.T(c, "app"), "", nil, func() {
		kit.Card(c, func() {
			kit.Row(c, "version", i18n.T(c, "settings_version"), "", text(m.Version))
			kit.Row(c, "build", i18n.T(c, "settings_build"), "", text(i18n.T(c, "preview")))
			kit.Row(c, "license", i18n.T(c, "settings_license"), "", text("Apache-2.0"))
		})
	})
	updates(c, m)
}
