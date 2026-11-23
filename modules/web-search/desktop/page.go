package desktop

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func Page() settings.NativePage {
	return settings.NativePage{
		Section: settings.Search,
		Group:   "settings_group_tools",
		Icon:    icons.Search,
		Order:   400,
		Render:  render,
	}
}

type pageState struct {
	editorOpen     bool
	driver         string
	openedRevision uint64
	endpoint       string
	apiKey         string
	apiKeyShown    bool
	clearKey       bool
	username       string
	usernameShown  bool
	password       string
	passwordShown  bool
	clearBasic     bool
	modelKey       string
	saving         bool
	retry          bool
}

func (st *pageState) CloseDialogs() {
	st.editorOpen = false
	st.saving = false
	st.retry = false
	st.apiKey = ""
	st.username = ""
	st.password = ""
	st.apiKeyShown = false
	st.usernameShown = false
	st.passwordShown = false
}

func render(c *ui.Context, s *settings.State) {
	st := s.NativePageState(settings.Search, func() any { return &pageState{} }).(*pageState)
	s.SearchPageState = st
	m := &s.Search
	act := s.SearchActions
	t := func(key string) string { return i18n.T(c, key) }

	if m.Status != "" {
		kit.Result(c, kit.ResultProps{Title: t(m.Status), Card: true})
		return
	}

	if !m.Loaded && m.Loading {
		kit.Card(c, func() {
			ui.Row(c).Height(96).Center().Children(func() { kit.Busy(c) })
		})
		return
	}

	locked := !m.Loaded || !m.Connected || m.Loading || m.Pending || m.Retryable

	if (!m.Loaded && !m.Loading) || (m.Retryable && !st.editorOpen) {
		ui.Row(c.Key("recovery")).Gap(8).Children(func() {
			if !m.Loaded && !m.Loading {
				kit.Secondary(c.Key("reload"), t("search_reload")).OnClick(func() {
					if act != nil && act.Reload != nil {
						act.Reload()
					}
				})
			}
			if m.Retryable && !st.editorOpen {
				kit.SecondaryBusy(c.Key("retry"), t("search_retry"), m.Pending).OnClick(func() {
					if act != nil && act.Retry != nil {
						act.Retry()
					}
				})
			}
		})
	}

	kit.Group(c, "search_settings", t("search_settings"), "", nil, func() {
		kit.Card(c, func() {
			options := make([]kit.SelectOption, 0, len(m.Drivers)+2)
			options = append(options, kit.SelectOption{Value: "auto", Label: t("search_driver_auto")})
			for _, driver := range m.Drivers {
				options = append(options, kit.SelectOption{Value: driver.ID, Label: t("search_driver_" + driver.ID)})
			}
			options = append(options, kit.SelectOption{Value: "none", Label: t("search_driver_none")})
			chosen := m.DefaultDriver
			if chosen == "" {
				chosen = "none"
			}
			kit.WideRow(c, "default-driver", t("search_default_driver"), "", func() {
				el, changed := kit.SelectField(c, "search-default-driver", &chosen, kit.SelectProps{
					Label:    t("search_default_driver"),
					Options:  options,
					Disabled: locked,
				})
				el.Grow(1).Shrink(1).MinWidth(0)
				if changed {
					m.DefaultDriver = chosen
					if s.Live && act != nil && act.SetDefault != nil {
						act.SetDefault(chosen)
					} else if chosen != "auto" && chosen != "none" {
						for i := range m.Drivers {
							if m.Drivers[i].ID == chosen {
								m.Drivers[i].Enabled = true
							}
						}
					}
				}
			})
			fallback := m.Fallback
			kit.Row(c, "fallback", t("search_fallback"), "", func() {
				if ui.Switch(c.Key("switch"), &fallback).
					Label(t("search_fallback")).
					Disabled(locked || chosen == "model").
					Changed() {
					m.Fallback = fallback
					if s.Live && act != nil && act.SetFallback != nil {
						act.SetFallback(fallback)
					}
				}
			})
		})
	})

	kit.Group(c, "search_providers", t("search_providers"), "", nil, func() {
		kit.Card(c, func() {
			kit.SortableList(c.Key("drivers"), &m.Drivers, func(d settings.SearchDriver) string { return d.ID }, kit.SortableListProps{
				Divided:  true,
				Disabled: locked,
				Handle:   t("search_reorder"),
				OnMove: func(_, _ int) {
					order := make([]string, 0, len(m.Drivers))
					for _, d := range m.Drivers {
						order = append(order, d.ID)
					}
					if s.Live && act != nil && act.Reorder != nil {
						act.Reorder(order)
					}
				},
			}, func(driver settings.SearchDriver, handle func()) {
				ui.Row(c).FillWidth().AlignItems(ui.Center).Gap(8).Children(func() {
					handle()
					kit.LeadRow(c, driver.ID, func() { driverLeadIcon(c, driver.ID) },
						t("search_driver_"+driver.ID), driverDetail(c, m, driver),
						func() {
							enabled := driver.Enabled
							if ui.Switch(c.Key("enabled"), &enabled).
								Label(t("search_driver_" + driver.ID)).
								Disabled(locked).
								Changed() {
								for i := range m.Drivers {
									if m.Drivers[i].ID == driver.ID {
										m.Drivers[i].Enabled = enabled
										break
									}
								}
								if s.Live && act != nil && act.SetEnabled != nil {
									act.SetEnabled(driver.ID, enabled)
								}
							}
							kit.MenuButton(c, "search-more-"+driver.ID, func(b ui.Element) {
								kit.IconLook(c, b.Disabled(locked), icons.Ellipsis, t("search_more"), "", 28, kit.MenuOpen(c, "search-more-"+driver.ID))
							}, func(menu *kit.Menu) {
								if menu.Item(t("settings_edit")).Icon(icons.Settings2).Chosen() {
									openEditor(st, m, driver)
								}
								if menu.Item(t("search_test")).Icon(icons.Play).Chosen() {
									if s.Live && act != nil && act.Test != nil {
										act.Test(driver.ID)
									} else {
										kit.Message(c, t("preview_notice"))
									}
								}
							}, kit.MenuOptions{At: ui.AnchorBottomEnd, Self: ui.AnchorTopEnd})
						}).Grow(1).Shrink(1).MinWidth(0)
				})
			})
		})
	})

	renderEditor(c, s, st)
}

func openEditor(st *pageState, m *settings.SearchSettingsState, driver settings.SearchDriver) {
	st.CloseDialogs()
	st.editorOpen = true
	st.driver = driver.ID
	st.openedRevision = m.Revision
	st.endpoint = driver.Endpoint
	st.clearKey = false
	st.clearBasic = false
	if driver.ModelProvider != "" && driver.ModelID != "" {
		st.modelKey = driver.ModelProvider + "/" + driver.ModelID
	} else {
		st.modelKey = ""
	}
}

func renderEditor(c *ui.Context, s *settings.State, st *pageState) {
	m := &s.Search
	act := s.SearchActions
	t := func(key string) string { return i18n.T(c, key) }

	var cur settings.SearchDriver
	for _, d := range m.Drivers {
		if d.ID == st.driver {
			cur = d
			break
		}
	}
	locked := m.Loading || m.Pending || st.saving || st.retry

	open := st.editorOpen
	kit.Dialog(c.Key("search-editor-"+st.driver), &open, func() {
		p := theme.Of(c)
		ui.Column(c).Width(480).Gap(16).Children(func() {
			ui.Text(c, t("search_driver_"+st.driver)).FontSize(16).FontWeight(600).TextColor(p.PopoverForeground)
			if st.driver == "model" {
				kit.Field(c, "model", t("search_model_name"), "", func() {
					kit.ModelSelect(c.Key("select"), kit.ModelSelectProps{
						Label:       t("search_model_name"),
						None:        t("search_model_follow"),
						Models:      m.ModelOptions,
						Model:       st.modelKey,
						Unavailable: t("search_model_unavailable"),
						Disabled:    locked,
						OnChange: func(id, _ string) {
							st.modelKey = id
						},
					}).FillWidth()
				})
			} else {
				if st.driver == "searxng" {
					kit.Field(c, "endpoint", t("search_endpoint"), "", func() {
						ui.TextInput(c.Key("input"), &st.endpoint).Label(t("search_endpoint")).Disabled(locked).FillWidth()
					})
				}
				keyLabel := t("search_api_key")
				if !cur.KeyRequired {
					keyLabel = t("search_optional_key")
				}
				keyPlaceholder := keyLabel
				if cur.KeyConfigured && !st.clearKey {
					keyPlaceholder = t("search_configured")
				}
				kit.Field(c, "api-key", keyLabel, "", func() {
					kit.SecretInput(c.Key("secret"), &st.apiKey, &st.apiKeyShown, kit.SecretInputProps{
						Label:       keyLabel,
						Placeholder: keyPlaceholder,
						Disabled:    locked || st.clearKey,
					})
					if cur.KeyConfigured && st.apiKey == "" {
						ui.Row(c.Key("clear-key")).AlignItems(ui.Center).Gap(8).Children(func() {
							ui.Switch(c.Key("switch"), &st.clearKey).Label(t("search_remove_key")).Disabled(locked)
							ui.Text(c, t("search_remove_key")).FontSize(13).TextColor(p.MutedForeground)
						})
					}
				})
				if st.driver == "searxng" {
					userPlaceholder := t("search_username")
					passPlaceholder := t("search_password")
					if cur.BasicConfigured && !st.clearBasic {
						userPlaceholder = t("search_configured")
						passPlaceholder = t("search_configured")
					}
					kit.Field(c, "username", t("search_username"), "", func() {
						kit.SecretInput(c.Key("secret"), &st.username, &st.usernameShown, kit.SecretInputProps{
							Label:       t("search_username"),
							Placeholder: userPlaceholder,
							Disabled:    locked || st.clearBasic,
						})
					})
					kit.Field(c, "password", t("search_password"), "", func() {
						kit.SecretInput(c.Key("secret"), &st.password, &st.passwordShown, kit.SecretInputProps{
							Label:       t("search_password"),
							Placeholder: passPlaceholder,
							Disabled:    locked || st.clearBasic,
						})
						if cur.BasicConfigured && st.username == "" && st.password == "" {
							ui.Row(c.Key("clear-basic")).AlignItems(ui.Center).Gap(8).Children(func() {
								ui.Switch(c.Key("switch"), &st.clearBasic).Label(t("search_remove_basic")).Disabled(locked)
								ui.Text(c, t("search_remove_basic")).FontSize(13).TextColor(p.MutedForeground)
							})
						}
					})
				}
			}

			ui.Row(c.Key("footer")).Justify(ui.End).Gap(8).Children(func() {
				if m.Revision != st.openedRevision {
					kit.Secondary(c.Key("reload"), t("search_reload")).Disabled(locked).OnClick(func() {
						openEditor(st, m, cur)
					})
				}
				kit.Secondary(c.Key("cancel"), t("settings_cancel")).Disabled(st.saving).OnClick(func() {
					if st.retry && act != nil && act.ForgetPending != nil {
						act.ForgetPending()
					}
					st.CloseDialogs()
				})
				saveLabel := t("settings_save")
				if st.retry {
					saveLabel = t("search_retry")
				}
				kit.PrimaryBusy(c.Key("save"), saveLabel, m.Loading || !m.Connected, st.saving).OnClick(func() {
					if st.retry {
						if act != nil && act.Retry != nil {
							st.saving = true
							act.Retry()
						}
						return
					}
					draft := settings.SearchDriverDraft{
						Driver:   st.driver,
						Endpoint: strings.TrimSpace(st.endpoint),
						Key:      settings.SearchCredentialUpdate{Action: "keep"},
						Basic:    settings.SearchCredentialUpdate{Action: "keep"},
					}
					if st.driver == "model" {
						if st.modelKey != "" {
							if !slices.ContainsFunc(m.ModelOptions, func(opt kit.ModelOption) bool { return opt.ID == st.modelKey }) {
								kit.Toast(c, kit.Failed, t("search_model_unavailable"))
								return
							}
							prov, mod, ok := strings.Cut(st.modelKey, "/")
							if ok {
								draft.ModelProvider, draft.ModelID = prov, mod
							}
						}
					} else {
						if st.apiKey != "" {
							draft.Key = settings.SearchCredentialUpdate{Action: "set", Value: st.apiKey}
						} else if st.clearKey {
							draft.Key = settings.SearchCredentialUpdate{Action: "clear"}
						}
						if st.driver == "searxng" {
							if (st.username != "") != (st.password != "") {
								kit.Toast(c, kit.Failed, t("search_basic_invalid"))
								return
							}
							if st.username != "" {
								draft.Basic = settings.SearchCredentialUpdate{Action: "set", Username: st.username, Password: st.password}
							} else if st.clearBasic {
								draft.Basic = settings.SearchCredentialUpdate{Action: "clear"}
							}
						}
					}
					if !s.Live || act == nil || act.SaveDriver == nil {
						for i := range m.Drivers {
							if m.Drivers[i].ID == st.driver {
								m.Drivers[i].Endpoint = draft.Endpoint
								if draft.Key.Action == "set" {
									m.Drivers[i].KeyConfigured = true
								} else if draft.Key.Action == "clear" {
									m.Drivers[i].KeyConfigured = false
								}
								m.Drivers[i].ModelProvider = draft.ModelProvider
								m.Drivers[i].ModelID = draft.ModelID
							}
						}
						st.CloseDialogs()
						kit.Toast(c, kit.Done, t("search_saved"))
						return
					}
					st.saving = true
					act.SaveDriver(draft, func(ok bool, retry bool) {
						st.saving = false
						st.retry = retry
						if ok {
							st.CloseDialogs()
						}
					})
				})
			})
		})
	})
	if !open && st.editorOpen && !st.saving {
		if st.retry && act != nil && act.ForgetPending != nil {
			act.ForgetPending()
		}
		st.CloseDialogs()
	}
}

func driverLeadIcon(c *ui.Context, driver string) {
	p := theme.Of(c)
	switch driver {
	case "tinyfish":
		ui.Icon(c, icons.Web).FontSize(18).TextColor(p.MutedForeground)
	case "model":
		ui.Icon(c, icons.Bot).FontSize(18).TextColor(p.MutedForeground)
	default:
		if mark := icons.BrandColor(driver); mark != nil {
			ui.Image(c, mark).Size(18, 18)
		} else {
			ui.Icon(c, icons.Search).FontSize(18).TextColor(p.MutedForeground)
		}
	}
}

func driverDetail(c *ui.Context, m *settings.SearchSettingsState, driver settings.SearchDriver) string {
	var parts []string
	switch {
	case driver.KeyRequired && !driver.KeyConfigured:
		parts = append(parts, i18n.T(c, "search_key_required"))
	case driver.ID == "searxng" && strings.TrimSpace(driver.Endpoint) == "":
		parts = append(parts, i18n.T(c, "search_endpoint_required"))
	case driver.ID == "model":
		if driver.ModelProvider == "" || driver.ModelID == "" {
			parts = append(parts, i18n.T(c, "search_model_follow"))
		} else {
			key := driver.ModelProvider + "/" + driver.ModelID
			if !slices.ContainsFunc(m.ModelOptions, func(opt kit.ModelOption) bool { return opt.ID == key }) {
				parts = append(parts, i18n.T(c, "search_model_unavailable"))
			}
		}
	}
	if driver.Quota != "" {
		key := "search_quota_" + driver.Quota
		if label := i18n.T(c, key); label != key {
			parts = append(parts, label)
		}
	}
	return strings.Join(parts, " · ")
}
