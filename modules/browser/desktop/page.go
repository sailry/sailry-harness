package desktop

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

func Page() settings.NativePage {
	return settings.NativePage{
		Section: settings.Browser,
		Group:   "settings_group_tools",
		Icon:    icons.Web,
		Order:   800,
		Render:  render,
	}
}

func render(c *ui.Context, s *settings.State) {
	b := &s.Browser
	act := s.BrowserActions
	t := func(key string) string { return i18n.T(c, key) }

	kit.Group(c, "browser_data", t("browser_data"), "", nil, func() {
		if !b.Supported {
			kit.Result(c, kit.ResultProps{Title: t("browser_import_unavailable"), Card: true})
			return
		}
		kit.Card(c, func() {
			scanning := b.Importing && len(b.Profiles) == 0
			kit.Row(c, "chrome_import", t("browser_chrome_import"), t("browser_chrome_description"), func() {
				kit.SecondaryBusy(c.Key("scan"), t("browser_import"), scanning).
					Disabled(!b.Enabled || b.Importing).
					OnClick(func() {
						if !s.Live || act == nil || act.ScanChrome == nil {
							kit.Message(c, t("preview_notice"))
							return
						}
						act.ScanChrome()
					})
			})
			persistent := b.Persistent
			kit.Row(c, "persistent", t("browser_data_local"), t("browser_data_description"), func() {
				if ui.Switch(c.Key("switch"), &persistent).
					Label(t("browser_data_local")).
					Disabled(!b.Enabled || b.Importing).
					Changed() {
					b.Persistent = persistent
					if s.Live && act != nil && act.SetPersistent != nil {
						act.SetPersistent(persistent)
					}
				}
			})
		})
	})

	kit.Group(c, "browser_external", t("browser_external_section"), "", nil, func() {
		if b.ExternalStatus != "" {
			kit.Result(c, kit.ResultProps{Title: t(b.ExternalStatus), Card: true})
			return
		}
		kit.Card(c, func() {
			desc := t("browser_external_description")
			if b.ExternalLoaded && !b.ExternalAvailable {
				desc = t("browser_external_unavailable")
			}
			disabled := !b.ExternalLoaded || b.ExternalEnabled.Pending || (!b.ExternalAvailable && !b.ExternalEnabled.Draft)
			kit.Row(c, "external_enabled", t("browser_external_enabled"), desc, func() {
				if ui.Switch(c.Key("switch"), &b.ExternalEnabled.Draft).
					Label(t("browser_external_enabled")).
					Disabled(disabled).
					Changed() {
					if s.Live && act != nil && act.SaveExternal != nil {
						act.SaveExternal()
					} else {
						b.ExternalEnabled.Commit()
					}
				}
			})
		})
	})

	open := len(b.Profiles) > 0
	title := t("browser_choose_profile")
	kit.Dialog(c.Key("browser-profiles-dialog"), &open, func() {
		kit.DialogHeader(c, title)
		ui.Scroll(c.Key("profiles")).FillWidth().MaxHeight(320).Gap(4).Children(func() {
			for _, profile := range b.Profiles {
				profile := profile
				busy := b.Importing && b.ImportingProfile == profile.ID
				disabled := !b.Enabled || (b.Importing && !busy)
				kit.TextActionBusy(c.Key(profile.ID), profile.Name, disabled, busy).
					FillWidth().Justify(ui.Start).
					OnClick(func() {
						if s.Live && act != nil && act.ImportChrome != nil {
							act.ImportChrome(profile.ID)
						}
					})
			}
		})
	}).Width(settings.DialogWidth(c, 388+2*kit.DialogInset)).Label(title)
	if !open && len(b.Profiles) > 0 && !b.Importing {
		if act != nil && act.CloseProfiles != nil {
			act.CloseProfiles()
		} else {
			b.Profiles = nil
		}
	}
}
