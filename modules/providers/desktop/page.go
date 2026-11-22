package desktop

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func Page() settings.NativePage {
	return settings.NativePage{
		Section: settings.Providers,
		Group:   "settings_group_ai",
		Icon:    icons.Cpu,
		Order:   100,
		Render:  func(c *ui.Context, s *settings.State) { For(s).render(c) },
	}
}

// Controller owns channel-management drafts and dialogs for a settings window.
type Controller struct {
	*settings.Model
	query, family string
	expanded      string
	removing      string
	provider      providerEditor
	login         *settings.Authorization
	loginFor      string
	loginWarning  loginWarning
}

// For returns the same controller to the settings page, onboarding and sign-in.
func For(s *settings.State) *Controller {
	return s.NativePageState(settings.Providers, func() any { return &Controller{Model: s.Model} }).(*Controller)
}

var compactionMessages = settings.Messages{Failed: "compaction_settings_failed", Conflict: "compaction_settings_conflict", Uncertain: "compaction_settings_unknown"}

// chips is how many model IDs an expanded row shows.
const chips = 8

// ProviderStatus is what a channel lacks: its key or its login; "" when it
// lacks nothing.
func ProviderStatus(ch *preview.Channel) string {
	switch {
	case ch.Auth == preview.AuthKey && !ch.KeySet:
		return "provider_status_missing_key"
	case ch.Auth == preview.AuthOAuth && !ch.SignedIn && !ch.AccountPending:
		return "provider_status_signed_out"
	}
	return ""
}

// matches is the query in the name, the API or a model's ID or name.
func matches(ch *preview.Channel, q string) bool {
	q = strings.ToLower(strings.TrimSpace(q))
	if q == "" || strings.Contains(strings.ToLower(ch.Name+" "+ch.API), q) {
		return true
	}
	return slices.ContainsFunc(ch.Models, func(m preview.ChannelModel) bool {
		return strings.Contains(strings.ToLower(m.ID+" "+m.Name), q)
	})
}

// tokens writes a token count as provider_tokens_k/_m.
func tokens(c *ui.Context, n int) string {
	if n >= 1_000_000 {
		return i18n.T(c, "provider_tokens_m", "count", n/1_000_000)
	}
	return i18n.T(c, "provider_tokens_k", "count", n/1000)
}

func (m *Controller) render(c *ui.Context) {
	if m.Live && m.ProviderActions.Available != nil && !m.ProviderActions.Available() {
		kit.Result(c, kit.ResultProps{Icon: icons.Device, Title: i18n.T(c, "unavailable"), Card: true})
		return
	}
	channels := m.providerSettingsChannels()
	ui.Column(c).Gap(24).Children(func() {
		catalog(c, m)
		ui.Column(c.Key("channels")).Gap(12).Children(func() {
			var shown []int
			for i := range channels {
				ch := &channels[i]
				if (m.family == "" || ch.Brand == m.family) && matches(ch, m.query) {
					shown = append(shown, i)
				}
			}
			kit.Toolbar(c, i18n.T(c, "provider_channels"), len(channels), &m.query, i18n.T(c, "provider_list_search"), func() {
				families(c, m)
				settings.IconButton(c, "add", icons.Plus, i18n.T(c, "settings_add"), true, false).OnClick(func() {
					if requireProviderCatalog(c, m, m.ManagedNodeID()) {
						m.provider.add(m)
					}
				})
			})
			if len(shown) == 0 {
				kit.Result(c, kit.ResultProps{Icon: icons.Cpu, Title: i18n.T(c, "provider_empty_live"), Card: true})
				return
			}
			kit.Card(c, func() {
				for _, i := range shown {
					channelRow(c, m, &channels[i])
				}
			})
		})
	})
	providerEditorDialog(c, m)
	m.LoginDialog(c)
	if ch := findChannel(channels, m.removing); ch != nil {
		open := true
		name := settings.ProviderHostLabel(c, m.Model, ch.Node, ch.Name)
		if settings.Confirm(c, "delete", &open, i18n.T(c, "provider_delete_title"), name+"\n"+i18n.T(c, "provider_remove_effect"), i18n.T(c, "settings_delete")) {
			if m.Live && m.ProviderActions.Remove != nil {
				m.ProviderActions.Remove(ch.Node, ch.ID, ch.Revision)
			} else {
				key := channelKey(ch.Node, ch.ID)
				m.Channels = slices.DeleteFunc(m.Channels, func(x preview.Channel) bool { return channelKey(x.Node, x.ID) == key })
			}
		}
		if !open {
			m.removing = ""
		}
	}
}

// catalog is the models.dev catalog's and the scores' state, the last update,
// Update, and the compaction threshold.
func catalog(c *ui.Context, m *Controller) {
	p := theme.Of(c)
	state := func(updated, bundled bool) func() {
		return func() {
			key, col := "provider_catalog_never", p.MutedForeground
			if updated {
				key, col = "provider_catalog_updated", p.Success
			} else if bundled {
				key = "provider_catalog_bundled"
			}
			ui.Text(c, i18n.T(c, key)).FontSize(14).TextColor(col)
		}
	}
	kit.Group(c, "catalog", i18n.T(c, "provider_catalog"), "", func() {
		kit.PrimaryBusy(c.Key("update"), i18n.T(c, "provider_catalog_update"), false, m.Busy[settings.BusyCatalog]).OnClick(func() {
			if m.Live && m.ProviderActions.RefreshCatalog != nil {
				m.ProviderActions.RefreshCatalog()
			} else {
				settings.PreviewNotice(c)
			}
		})
	}, func() {
		kit.Card(c, func() {
			kit.Row(c, "status", i18n.T(c, "provider_catalog_status"), "", state(m.Catalog.Updated, m.Catalog.Bundled))
			kit.Row(c, "scores", i18n.T(c, "provider_scores"), "", state(m.Catalog.Scores, false))
			last := m.Catalog.Last
			if last == "" {
				last = i18n.T(c, "provider_catalog_never")
			}
			kit.Row(c, "last", i18n.T(c, "provider_catalog_last_update"), "", func() {
				ui.Text(c, last).FontSize(14).TextColor(p.MutedForeground)
			})
			t := i18n.T(c, "provider_compaction_threshold")
			kit.Row(c, "compaction", t, "", func() {
				if m.Live && m.Compaction.Saved == 0 {
					ui.Text(c, i18n.T(c, "unavailable")).FontSize(14).TextColor(p.MutedForeground)
					return
				}
				ui.Row(c).Width(240).Gap(12).Children(func() {
					if ui.StepSlider(c.Key("slider"), &m.Compaction.Draft, 50, 95, 5).Label(t).Disabled(m.Compaction.Pending).Grow(1).Changed() {
						if m.Live && m.ProviderActions.SaveCompaction != nil {
							m.ProviderActions.SaveCompaction()
						} else {
							settings.Report(c, m.Compaction.Commit(), compactionMessages)
						}
					}
					if m.Live && m.Compaction.Pending {
						kit.Busy(c) // the threshold is being saved
					}
					ui.Textf(c, "%.0f%%", m.Compaction.Draft).FontSize(14).Width(40).TextAlign(ui.End)
				})
			})
		})
	})
}

func requireProviderCatalog(c *ui.Context, m *Controller, node string) bool {
	catalog := m.Catalog
	if m.Live && node != "" && m.ProviderActions.CatalogAt != nil {
		catalog = m.ProviderActions.CatalogAt(node)
	}
	if !m.Live || catalog.Updated || catalog.Bundled {
		return true
	}
	kit.Toast(c, kit.Failed, i18n.T(c, "provider_catalog_required"))
	return false
}

// capabilityMarks are the small icons of what a model understands and
// generates, each named by its tooltip.
func capabilityMarks(c *ui.Context, x *preview.ChannelModel) {
	p := theme.Of(c)
	for _, kind := range viewmodel.Capabilities(x.Vision, x.Generates) {
		label := i18n.T(c, viewmodel.CapabilityLabels[kind])
		kit.Tooltip(c, ui.Icon(c.Key(kind), icons.Capabilities[kind]).FontSize(12).Shrink(0).TextColor(p.MutedForeground).Label(label), label)
	}
}

// family is a choice of the list's filter: a brand, by its ID.
type family struct{ id, name string }

// families are the offered brands, then the brands only a channel has (one
// no longer offered), named by the channel.
func (m *Controller) families() []family {
	var out []family
	node := m.ManagedNodeID()
	for _, b := range m.Brands {
		if node != "" && b.Node != node {
			continue
		}
		if !slices.ContainsFunc(out, func(f family) bool { return f.id == b.ID }) {
			out = append(out, family{b.ID, b.Name})
		}
	}
	for _, ch := range m.providerSettingsChannels() {
		if ch.Brand == "" || slices.ContainsFunc(out, func(f family) bool { return f.id == ch.Brand }) {
			continue
		}
		name := ch.BrandName
		if name == "" {
			name = ch.Brand[strings.LastIndex(ch.Brand, ":")+1:]
		}
		out = append(out, family{ch.Brand, name})
	}
	return out
}

// families is All, then each brand with its mark, the list's filter.
func families(c *ui.Context, m *Controller) {
	title := i18n.T(c, "provider_type")
	options := []kit.SelectOption{{Label: i18n.T(c, "provider_all"), Image: icons.Settings2}}
	for _, f := range m.families() {
		options = append(options, kit.SelectOption{Value: f.id, Label: f.name, Image: icons.BrandColor(f.id)})
	}
	kit.SelectField(c.Key("families"), "setting-"+title, &m.family, kit.SelectProps{Label: title, Options: options})
}

// channelMark is the channel's mark for its tile: its own, else its
// brand's; nil shows the name's initial.
func channelMark(ch *preview.Channel) *ui.SVG {
	if mark := icons.Brand(ch.Icon); mark != nil {
		return mark
	}
	return icons.Brand(ch.Brand)
}

func channelTile(c *ui.Context, ch *preview.Channel, size float32) {
	ui.Row(c).Size(size, size).Center().Children(func() { kit.Tile(c, ch.Name, channelMark(ch)) })
}

// channelRow is the channel, its quota, what it lacks, its switch and menu;
// clicking it shows its models.
func channelRow(c *ui.Context, m *Controller, ch *preview.Channel) {
	p := theme.Of(c)
	key := channelKey(ch.Node, ch.ID)
	ui.Column(c.Key(key)).Children(func() {
		ui.Row(c.Key("row")).Padding(12, 0).Gap(12).Children(func() {
			access := ch.API
			if ch.Auth == preview.AuthOAuth {
				access = "OAuth"
			}
			desc := access + " · " + i18n.T(c, "provider_model_count", "count", len(ch.Models))
			if !m.Live {
				desc = i18n.T(c, "preview") + " · " + desc
			}
			b := ui.ButtonBase(c.Key("details")).Label(ch.Name).Grow(1).Shrink(1).MinWidth(0).Justify(ui.Start).Cursor(ui.CursorPointer).
				Opacity(enabledOpacity(ch.Enabled)).OnClick(func() {
				if m.expanded == key {
					m.expanded = ""
				} else {
					m.expanded = key
				}
			})
			b.Children(func() {
				kit.MediaRow(c, ch.Name, desc, func() { kit.Tile(c, ch.Name, channelMark(ch)) }, nil)
			})
			var usage *settings.ChannelUsage
			if m.Live && m.ProviderActions.Usage != nil {
				usage = m.ProviderActions.Usage(c, ch.Node, ch.ID)
			}
			if usage != nil {
				channelUsage(c, ch, usage)
			} else if ch.Quota != nil && ch.Auth == preview.AuthOAuth {
				ui.Column(c.Key("usage")).Shrink(0).Gap(2).Opacity(enabledOpacity(ch.Enabled)).Children(func() {
					for _, w := range []struct {
						key string
						v   float64
					}{{"provider_window_5h", ch.Quota.FiveHour}, {"provider_window_7d", ch.Quota.SevenDay}} {
						if w.v < 0 {
							continue
						}
						ui.Row(c.Key(w.key)).Gap(6).Children(func() {
							ui.Text(c, i18n.T(c, w.key)).FontSize(12).Width(20).TextColor(p.MutedForeground)
							ui.Progress(c, w.v/100).Width(64).Height(4)
							ui.Textf(c, "%.0f%%", w.v).FontSize(12).Width(32).TextAlign(ui.End).TextColor(p.MutedForeground)
						})
					}
				})
			}
			if st := ProviderStatus(ch); st != "" {
				ui.Row(c.Key("status")).Shrink(0).PaddingX(8).Gap(6).Children(func() {
					ui.Box(c).Size(6, 6).Radius(3).Background(p.Warning)
					ui.Text(c, i18n.T(c, st)).FontSize(14).TextColor(p.MutedForeground)
				})
			}
			if ui.Switch(c.Key("enabled"), &ch.Enabled).Label(i18n.T(c, "provider_enabled")+" "+ch.Name).Disabled(ch.ConfigurationReadOnly).Changed() && m.Live && m.ProviderActions.Enable != nil {
				m.ProviderActions.Enable(ch.Node, ch.ID, ch.Revision, ch.Enabled)
			}
			channelMenu(c, m, ch)
		})
		if m.expanded == key {
			channelModels(c, ch)
		}
	})
}

// channelUsageValue is the least width of a meter's value, which holds a
// percentage with two decimals (list.rs PERCENT).
const channelUsageValue = 36

// channelUsage is the provider slot in the channel's row, where the login
// quota is and in its place (settings/providers/list.rs provider_usage): the
// account figure a package publishes for the channel, as up to two meters or
// one value after the figure's icon, with every figure in a card on hover.
// The meters are the login quota's. A meter under a fifth is not tinted as a
// warning, as the login quota's is not here, and a value without a declared
// icon has none, where the reference draws coins.
func channelUsage(c *ui.Context, ch *preview.Channel, usage *settings.ChannelUsage) {
	p := theme.Of(c)
	column := ui.Column(c.Key("usage")).Shrink(0).Gap(2).AlignItems(ui.End).Opacity(enabledOpacity(ch.Enabled))
	column.Children(func() {
		for i, line := range usage.Lines {
			ui.Row(c.Key(i)).Gap(6).AlignItems(ui.Center).Children(func() {
				if !line.Meter {
					if usage.Icon != nil {
						ui.Icon(c, usage.Icon).FontSize(14).TextColor(p.MutedForeground)
					}
					ui.Text(c, line.Value).FontSize(14).SingleLine().FontFeatures("tnum")
					return
				}
				ui.Text(c, line.Label).FontSize(12).SingleLine().TextColor(p.MutedForeground)
				ui.Progress(c, line.Progress/100).Width(64).Height(4).Label(line.Label)
				ui.Text(c, line.Value).FontSize(12).SingleLine().MinWidth(channelUsageValue).TextAlign(ui.End).
					FontFeatures("tnum").TextColor(p.MutedForeground)
			})
		}
	})
	if usage.Card != nil {
		usage.Card(c, column)
	}
}

// channelModels is the default model first, then the others, as chips, at
// most chips of them and the count of the rest.
func channelModels(c *ui.Context, ch *preview.Channel) {
	p := theme.Of(c)
	ids := make([]*preview.ChannelModel, 0, len(ch.Models))
	for i := range ch.Models {
		if i == ch.Default {
			ids = append([]*preview.ChannelModel{&ch.Models[i]}, ids...)
		} else {
			ids = append(ids, &ch.Models[i])
		}
	}
	hidden := max(0, len(ids)-chips)
	ids = ids[:min(len(ids), chips)]
	ui.Row(c.Key("models")).Wrap().Gap(6).Padding(0, 0, 12, kit.TileSize+12).Children(func() {
		if len(ids) == 0 {
			ui.Text(c, i18n.T(c, "provider_models_empty")).FontSize(14).TextColor(p.MutedForeground)
		}
		for _, x := range ids {
			// The model's ID, then the marks of what it understands and
			// generates.
			ui.Row(c.Key(x.ID)).Height(24).Padding(4, 8).Gap(4).AlignItems(ui.Center).Border(1, p.Border).Radius(p.Radius).Children(func() {
				ui.Text(c, x.ID).FontSize(12).SingleLine()
				capabilityMarks(c, x)
			})
		}
		if hidden > 0 {
			ui.Textf(c, "+%d", hidden).FontSize(12).Height(24).Padding(4, 8).TextColor(p.MutedForeground)
		}
	})
}

// channelMenu is edit, the account's sign-in, and delete.
func channelMenu(c *ui.Context, m *Controller, ch *preview.Channel) {
	key := channelKey(ch.Node, ch.ID)
	kit.MenuButton(c.Key("menu"), "provider-menu-"+key, func(b ui.Element) {
		kit.IconLook(c, b, icons.Ellipsis, i18n.T(c, "more"), "", 28, false)
	}, func(menu *kit.Menu) {
		if menu.Item(i18n.T(c, "settings_edit")).Icon(icons.Pencil).Disabled(ch.ConfigurationReadOnly).Chosen() {
			m.provider.edit(m, ch)
		}
		if ch.Auth == preview.AuthOAuth && (!ch.ConfigurationReadOnly || ch.Source != "") {
			key := "provider_connect"
			if ch.Account != "" || ch.SignedIn || ch.AccountPending {
				key = "provider_reconnect"
			}
			if menu.Item(i18n.T(c, key)).Icon(icons.CircleUser).Chosen() {
				m.signIn(ch.Node, ch.ID)
			}
		}
		menu.Separator()
		if menu.Item(i18n.T(c, "settings_delete")).Icon(icons.Trash).Danger().Chosen() {
			m.removing = key
		}
	}, kit.MenuOptions{At: ui.AnchorBottomEnd, Self: ui.AnchorTopEnd})
}

func channelKey(node, id string) string {
	if node == "" {
		return id
	}
	return node + "/" + id
}

func (m *Controller) channel(key string) *preview.Channel {
	if node, _, qualified := strings.Cut(key, "/"); qualified && m.Live && m.ChannelsByNode != nil {
		return findChannel(m.ChannelsByNode[node], key)
	}
	return findChannel(m.Channels, key)
}

func (m *Controller) providerSettingsChannels() []preview.Channel {
	if m.Live && (m.ChannelsByNode != nil || m.ManagedNode != "") {
		return m.ChannelsByNode[m.ManagedNodeID()]
	}
	return m.Channels
}

func findChannel(channels []preview.Channel, key string) *preview.Channel {
	if key == "" {
		return nil
	}
	for i := range channels {
		if channelKey(channels[i].Node, channels[i].ID) == key {
			return &channels[i]
		}
	}
	return nil
}

// signIn opens the authorization of a channel's account login. The
// preview has no provider: ChatGPT shows a device code, Claude its
// browser page with the pasted callback.
func (m *Controller) signIn(node, id string) {
	if m.Live {
		if m.ProviderActions.SignIn != nil {
			m.ProviderActions.SignIn(node, id)
		}
		return
	}
	key := channelKey(node, id)
	ch := m.channel(key)
	if ch == nil {
		return
	}
	view := &settings.Authorization{Title: ch.Name, Flow: settings.FlowBrowser, URL: "https://claude.ai/oauth/authorize", Callback: true}
	if ch.Method == "chatgpt-login" {
		view = &settings.Authorization{Title: ch.Name, Flow: settings.FlowDevice, Code: "PREV-IEW0", URL: "https://auth.openai.com/codex/device", Expires: "14:52"}
	}
	view.Retry = func() { m.signIn(node, id) }
	m.ShowProviderAuthorization(node, id, view)
}

func (m *Controller) providerHostAvailable(node string) bool {
	if node != "" && m.ProviderActions.Hosts != nil {
		return slices.ContainsFunc(m.ProviderActions.Hosts(), func(host settings.ProviderHost) bool {
			return host.Node == node && host.Connected
		})
	}
	return m.ProviderActions.Available == nil || m.ProviderActions.Available()
}

// enabledOpacity fades disabled rows as the providers list does.
func enabledOpacity(on bool) float32 {
	if on {
		return 1
	}
	return 0.45
}
