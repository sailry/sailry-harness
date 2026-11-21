package settings

import (
	"fmt"
	"maps"
	"math"
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The Providers page (settings/providers): the model catalog with the
// compaction threshold, the channels filtered by family and search, each
// expanding to its models, and the editor that adds or edits one: the
// brand step (grouped by access method), the method
// step where a brand has several, then the configuration and models. A
// method's wire protocol, product and region are switched in the form.
// Account logins sign in in the authorization dialog.

var compactionMessages = Messages{"", "compaction_settings_failed", "compaction_settings_conflict", "compaction_settings_unknown"}

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

func providers(c *ui.Context, s *State) {
	m := s.Model
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
				if (m.Family == "" || ch.Brand == m.Family) && matches(ch, s.Query) {
					shown = append(shown, i)
				}
			}
			kit.Toolbar(c, i18n.T(c, "provider_channels"), len(channels), &s.Query, i18n.T(c, "provider_list_search"), func() {
				families(c, m)
				iconButton(c, "add", icons.Plus, i18n.T(c, "settings_add"), true, false).OnClick(func() {
					if requireProviderCatalog(c, m, m.managedNodeID()) {
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
	providerAuthorizationDialog(c, m)
	if ch := findChannel(channels, m.removing); ch != nil {
		open := true
		name := providerHostLabel(c, m, ch.Node, ch.Name)
		if confirm(c, "delete", &open, i18n.T(c, "provider_delete_title"), name+"\n"+i18n.T(c, "provider_remove_effect"), i18n.T(c, "settings_delete")) {
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
func catalog(c *ui.Context, m *Model) {
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
		kit.PrimaryBusy(c.Key("update"), i18n.T(c, "provider_catalog_update"), false, m.Busy[BusyCatalog]).OnClick(func() {
			if m.Live && m.ProviderActions.RefreshCatalog != nil {
				m.ProviderActions.RefreshCatalog()
			} else {
				notice(c)
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
							report(c, m.Compaction.Commit(), compactionMessages)
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

func requireProviderCatalog(c *ui.Context, m *Model, node string) bool {
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
func (m *Model) families() []family {
	var out []family
	node := m.managedNodeID()
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
func families(c *ui.Context, m *Model) {
	title := i18n.T(c, "provider_type")
	options := []kit.SelectOption{{Label: i18n.T(c, "provider_all"), Image: icons.Settings2}}
	for _, f := range m.families() {
		options = append(options, kit.SelectOption{Value: f.id, Label: f.name, Image: icons.BrandColor(f.id)})
	}
	kit.SelectField(c.Key("families"), "setting-"+title, &m.Family, kit.SelectProps{Label: title, Options: options})
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
func channelRow(c *ui.Context, m *Model, ch *preview.Channel) {
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
			var usage *ChannelUsage
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
func channelUsage(c *ui.Context, ch *preview.Channel, usage *ChannelUsage) {
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
func channelMenu(c *ui.Context, m *Model, ch *preview.Channel) {
	key := channelKey(ch.Node, ch.ID)
	kit.MenuButton(c.Key("menu"), "provider-menu-"+key, func(b ui.Element) {
		kit.IconLook(c, b, icons.Ellipsis, i18n.T(c, "more"), "", 28, false)
	}, func(menu *Menu) {
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

// Menu is kit.Menu, named here for the builders' signatures.
type Menu = kit.Menu

func channelKey(node, id string) string {
	if node == "" {
		return id
	}
	return node + "/" + id
}

func (m *Model) channel(key string) *preview.Channel {
	if node, _, qualified := strings.Cut(key, "/"); qualified && m.Live && m.ChannelsByNode != nil {
		return findChannel(m.ChannelsByNode[node], key)
	}
	return findChannel(m.Channels, key)
}

func (m *Model) providerSettingsChannels() []preview.Channel {
	if m.Live && (m.ChannelsByNode != nil || m.ManagedNode != "") {
		return m.ChannelsByNode[m.managedNodeID()]
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
func (m *Model) signIn(node, id string) {
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
	view := &Authorization{Title: ch.Name, Flow: FlowBrowser, URL: "https://claude.ai/oauth/authorize", Callback: true}
	if ch.Method == "chatgpt-login" {
		view = &Authorization{Title: ch.Name, Flow: FlowDevice, Code: "PREV-IEW0", URL: "https://auth.openai.com/codex/device", Expires: "14:52"}
	}
	view.Retry = func() { m.signIn(node, id) }
	m.ShowProviderAuthorization(node, id, view)
}

// providerStep is the editor's page.
type providerStep int

const (
	stepBrand providerStep = iota
	stepMethod
	stepConfigure
)

// modelDraft is a model in the editor: its ID, context limit and reasoning
// efforts as typed, and whether its form shows.
type modelDraft struct {
	pending bool
	// seeded marks a model the route came with, which another route
	// replaces while its ID is as it came.
	seeded  bool
	model   preview.ChannelModel
	id      string
	context string
	efforts []string
	effort  string // the effort being added
	def     int    // the default effort
	// effortInput is the efforts field's input, which a click in the field
	// focuses; rejected asks the next frame to say an effort was refused.
	effortInput ui.Handle
	rejected    bool
	open        bool
	// hosted keeps the image tool the model came with: ticking Image again
	// restores it rather than the images endpoint.
	hosted bool
	// catalog is the model as it came when its tags were the catalog's,
	// which a reset of the tags shows again; nil when they were the user's.
	catalog *preview.ChannelModel
	lookup  *modelCapabilityLookup
	// relookup asks the catalog for the saved ID's tags, which a row that
	// follows the catalog again after the user's tags does not have.
	relookup bool
}

// providerEditor is the step, the brand, the methods its choice offered and
// the method and route chosen, the fields and the models. editing is the
// channel edited, "" for a new one.
type providerEditor struct {
	resumeSave          func(ProviderDraft, func(ProviderSaveResult))
	generation          uint64
	id                  string
	owner               string
	group               string
	revision            uint64
	targets             []string
	targetIDs           map[string]string
	targetRevisions     map[string]uint64
	targetSucceeded     []string
	targetFailed        []string
	targetUncertain     []string
	targetUnconfirmed   []string
	open                bool
	step                providerStep
	editing             string
	brand               *preview.Brand
	methods             []preview.Method
	method              preview.Method
	route               int
	name, endpoint, key string
	keySet              bool
	originalKey         string
	keyShown            bool
	apiVersion          string    // Azure OpenAI's, asked for where the route is that API
	oauth               [4]string // catalog, user agent, editor, editor plugin versions
	headers             []header  // an API-key channel's request headers, a row each
	headersRemoved      int       // header rows taken out, part of a row's identity
	// billingRate is an API-key channel's billing rate as the percent typed;
	// billingRateBase is the saved rate, nil for a channel without one.
	billingRate     string
	billingRateBase *uint32
	advanced        bool
	models          []modelDraft
	removed         int // models taken out, part of a model row's identity (mcpEditor.removed)
	def             int
	probing         bool
	saving          bool // probing is the save's, not a discovery's
	// returnTo reopens the welcome guide from the first step's Back, when
	// the guide opened the editor; nil otherwise.
	returnTo func()
}

func draftsOf(ms []preview.ChannelModel) []modelDraft {
	out := make([]modelDraft, len(ms))
	for i, x := range ms {
		ctx := ""
		if x.Context > 0 {
			ctx = strconv.Itoa(x.Context)
		}
		x.Inputs, x.Outputs, x.Generates = slices.Clone(x.Inputs), slices.Clone(x.Outputs), slices.Clone(x.Generates)
		out[i] = modelDraft{model: x, id: x.ID, context: ctx, efforts: slices.Clone(x.Efforts), def: max(0, slices.Index(x.Efforts, x.DefaultEffort)),
			hosted: slices.Contains(x.Generates, viewmodel.GenerateImageTool)}
		if !x.CustomCapabilities {
			came := ms[i]
			out[i].catalog = &came
		}
	}
	return out
}

func (e *providerEditor) add(m *Model) {
	owner := m.managedNodeID()
	if m.Live && m.ManagedNode == "" && m.ProviderActions.DefaultTarget != nil {
		if preferred := m.ProviderActions.DefaultTarget(); preferred != "" {
			owner = preferred
		}
	}
	e.addOn(m, owner)
}

func (e *providerEditor) addOn(m *Model, owner string) {
	*e = providerEditor{open: true, step: stepBrand, generation: e.generation + 1, owner: owner}
	if e.owner != "" {
		e.targets = []string{e.owner}
	}
	if m.Live && m.ProviderActions.NewID != nil {
		e.id = m.ProviderActions.NewID()
	}
}

// CloseProviderEditor drops the draft and actions captured for the previous Node.
func (m *Model) CloseProviderEditor(nextNode string) {
	m.removing, m.expanded, m.Family = "", "", ""
	if e := &m.provider; nextNode != "" && e.open && e.returnTo != nil && e.owner == nextNode {
		// The welcome guide opens its editor on the incoming local Node.
		return
	}
	m.provider = providerEditor{generation: m.provider.generation + 1}
}

func (e *providerEditor) edit(m *Model, ch *preview.Channel) {
	*e = providerEditor{open: true, step: stepConfigure, editing: ch.ID, owner: ch.Node, group: ch.Group, targetIDs: maps.Clone(ch.TargetIDs), targetRevisions: maps.Clone(ch.TargetRevisions), id: ch.ID, generation: e.generation + 1, name: ch.Name, endpoint: ch.Endpoint,
		keySet: ch.KeySet, models: draftsOf(ch.Models), def: ch.Default}
	if ch.Node != "" {
		e.targets = []string{ch.Node}
	}
	if e.targetRevisions == nil && ch.Node != "" {
		e.targetRevisions = map[string]uint64{ch.Node: ch.Revision}
	}
	e.revision = ch.Revision
	e.oauth = ch.OAuth
	e.apiVersion = ch.APIVersion
	e.headers = headerRows(ch.Headers)
	e.billingRate, e.billingRateBase = formatBillingRate(ch.BillingRateBps), cloneBillingRate(ch.BillingRateBps)
	if m.Live && !ch.ConfigurationReadOnly && (ch.Auth == preview.AuthKey || ch.Auth == viewmodel.AuthOptionalKey && ch.KeySet) && m.ProviderActions.ReadKey != nil {
		id := ch.ID
		generation := e.generation
		m.ProviderActions.ReadKey(ch.Node, id, func(value string) {
			if e.open && e.generation == generation && e.editing == id && e.owner == ch.Node {
				e.keySet, e.originalKey = value != "", value
				if e.key == "" {
					e.key = value
				}
			}
		})
	}
	for i := range m.Brands {
		b := &m.Brands[i]
		for _, x := range b.Methods {
			if ch.Node != "" && x.Node != ch.Node {
				continue
			}
			for j, route := range methodRoutes(x) {
				if route.ID == ch.Method {
					e.brand, e.method, e.route = b, x, j
				}
			}
		}
	}
	// A setting the channel saved without a value shows the one in use.
	for i, value := range e.method.OAuth {
		if e.oauth[i] == "" {
			e.oauth[i] = value
		}
	}
	if e.brand == nil {
		e.method = preview.Method{Node: ch.Node, ID: ch.Method, Name: ch.Name, API: ch.API, Auth: ch.Auth, Keyed: ch.Auth == preview.AuthKey || ch.Auth == viewmodel.AuthOptionalKey, Custom: ch.Endpoint != ""}
		if ch.Auth == preview.AuthOAuth {
			e.method.Login = "provider_access_login"
		}
	}
	if e.azure() && e.apiVersion == "" {
		e.apiVersion = azureDefaultVersion
	}
}

// azureDefaultVersion is the API version an Azure OpenAI form starts with,
// the reference's.
const azureDefaultVersion = "2024-10-21"

// azure reports an Azure OpenAI route: its form asks for the API version
// and names a model by its deployment.
func (e *providerEditor) azure() bool { return e.current().API == "azure_open_ai" }

// methodRoutes are a method's routes: its own, or the method as its only
// one.
func methodRoutes(x preview.Method) []preview.Route {
	if len(x.Routes) > 0 {
		return x.Routes
	}
	return []preview.Route{{ID: x.ID, API: x.API, Endpoint: x.Endpoint, Custom: x.Custom}}
}

// current is the route chosen.
func (e *providerEditor) current() preview.Route {
	routes := methodRoutes(e.method)
	return routes[min(max(e.route, 0), len(routes)-1)]
}

// routeAxis is what a form control switches a route by.
type routeAxis int

const (
	axisProtocol routeAxis = iota
	axisProduct
	axisRegion
)

// choices are the routes an axis's control offers, by index: those that
// differ from the current one in the axis alone (a product keeps the
// protocol, as its region follows in switchRoute), one for each value.
func (e *providerEditor) choices(axis routeAxis) []int {
	routes, cur := methodRoutes(e.method), e.current()
	var out []int
	for i, r := range routes {
		ok, same := false, func(o preview.Route) bool { return false }
		switch axis {
		case axisProtocol:
			ok, same = r.Region == cur.Region && r.Product == cur.Product, func(o preview.Route) bool { return o.API == r.API }
		case axisProduct:
			ok, same = r.API == cur.API, func(o preview.Route) bool { return o.Product == r.Product }
		case axisRegion:
			ok, same = r.API == cur.API && r.Product == cur.Product, func(o preview.Route) bool { return o.Region == r.Region }
		}
		if ok && !slices.ContainsFunc(out, func(j int) bool { return same(routes[j]) }) {
			out = append(out, i)
		}
	}
	return out
}

// switchRoute takes another route of the method. Another product keeps the
// region where it has one. The name, the key and what the user typed stay:
// the endpoint follows only while it is the route's own, and the models the
// route came with are replaced only while none was added or renamed.
func (e *providerEditor) switchRoute(i int) {
	routes, cur := methodRoutes(e.method), e.current()
	if i < 0 || i >= len(routes) {
		return
	}
	if want := routes[i]; e.editing != "" && (want.API == cur.API || want.Product != cur.Product || want.Region != cur.Region) {
		return
	}
	if want := routes[i]; want.Product != cur.Product {
		if j := slices.IndexFunc(routes, func(r preview.Route) bool {
			return r.API == want.API && r.Product == want.Product && r.Region == cur.Region
		}); j >= 0 {
			i = j
		}
	}
	if e.endpoint == cur.Endpoint {
		e.endpoint = routes[i].Endpoint
	}
	e.route = i
	e.refreshImageGeneration()
	if !slices.ContainsFunc(e.models, func(d modelDraft) bool { return !d.seeded || d.id != d.model.ID }) {
		e.models, e.def = nil, 0
		e.seed()
	}
}

// seed starts a new channel without models on the ones its route comes
// with, at the limits and tags a save gives a model typed in.
func (e *providerEditor) seed() {
	if len(e.models) > 0 || e.editing != "" {
		return
	}
	for _, id := range e.current().Models {
		d := draftsOf([]preview.ChannelModel{{ID: id, Name: id, Context: 128000, Vision: true, Tools: true}})[0]
		d.seeded = true
		e.models = append(e.models, d)
	}
}

// choose takes a choice of the brand step, a brand and the methods it
// offers: one method configures it at once, several ask which.
func (e *providerEditor) choose(b *preview.Brand, methods []preview.Method) {
	e.brand, e.methods = b, methods
	if len(methods) == 1 {
		e.configure(methods[0])
		return
	}
	e.step = stepMethod
}

// configure opens the form on a method's first route.
func (e *providerEditor) configure(x preview.Method) {
	e.method, e.route, e.step = x, 0, stepConfigure
	if e.owner == "" {
		e.owner = x.Node
	}
	if len(e.targets) == 0 && e.owner != "" {
		e.targets = []string{e.owner}
	}
	e.targets = e.normalizedTargets()
	e.name, e.endpoint = x.Name, e.current().Endpoint
	e.models, e.def = nil, 0
	e.oauth = x.OAuth
	e.apiVersion = ""
	// A rate typed for another method stays, as the headers do.
	if e.billingRate == "" {
		e.billingRate = formatBillingRate(nil)
	}
	if e.azure() {
		e.apiVersion = azureDefaultVersion
	}
	e.seed()
}

// back is Configure goes to Method when the choice offered several, else
// to Brand.
func (e *providerEditor) back() {
	if e.step == stepConfigure && len(e.methods) > 1 {
		e.step = stepMethod
		return
	}
	if back := e.returnTo; back != nil {
		// Opened from the welcome guide, whose picker was the first step: back
		// closes the editor and returns.
		e.open, e.returnTo = false, nil
		back()
		return
	}
	e.step = stepBrand
}

func (e *providerEditor) usesKey() bool {
	return e.method.Keyed || e.method.Access == "provider_access_key" || e.method.Access == "provider_access_cloud"
}

func (e *providerEditor) auth() preview.Auth {
	if e.method.Auth == preview.AuthOAuth || e.method.Auth == preview.AuthPlugin || e.method.Auth == viewmodel.AuthOptionalKey {
		return e.method.Auth
	}
	if e.login() {
		return preview.AuthOAuth
	}
	if e.method.Auth == preview.AuthKey || e.usesKey() {
		return preview.AuthKey
	}
	return preview.AuthNone
}

func providerAuthAllowsMultipleTargets(auth preview.Auth) bool {
	return auth == preview.AuthKey || auth == viewmodel.AuthOptionalKey || auth == preview.AuthNone
}

func providerTargetList(targets []string) []string {
	result := make([]string, 0, len(targets))
	for _, target := range targets {
		if target != "" && !slices.Contains(result, target) {
			result = append(result, target)
		}
	}
	slices.Sort(result)
	return result
}

func completeProviderSaveResult(targets []string, result ProviderSaveResult) ProviderSaveResult {
	wanted := providerTargetList(targets)
	result.Uncertain = providerTargetList(result.Uncertain)
	result.Unconfirmed = providerTargetList(result.Unconfirmed)
	uncertain := func(node string) bool {
		return slices.Contains(result.Uncertain, node) || slices.Contains(result.Unconfirmed, node)
	}
	failed := map[string]bool{}
	var extraFailed []string
	for _, node := range result.Failed {
		if uncertain(node) {
			continue
		}
		failed[node] = true
		if !slices.Contains(wanted, node) {
			extraFailed = append(extraFailed, node)
		}
	}
	succeeded := map[string]bool{}
	for _, node := range result.Succeeded {
		if !failed[node] {
			succeeded[node] = true
		}
	}
	result.Succeeded, result.Failed = nil, nil
	for _, node := range wanted {
		if uncertain(node) {
			continue
		}
		if failed[node] {
			result.Failed = append(result.Failed, node)
		} else if succeeded[node] {
			result.Succeeded = append(result.Succeeded, node)
		} else {
			result.Failed = append(result.Failed, node)
		}
	}
	result.Failed = append(result.Failed, providerTargetList(extraFailed)...)
	return result
}

func (e *providerEditor) toggleCopyTarget(node string) {
	if node == "" || node == e.owner || !providerAuthAllowsMultipleTargets(e.auth()) {
		return
	}
	if index := slices.Index(e.targets, node); index >= 0 {
		e.targets = slices.Delete(e.targets, index, index+1)
	} else {
		e.targets = append(e.targets, node)
	}
	e.targetSucceeded, e.targetFailed, e.targetUncertain = nil, nil, nil
	e.targetUnconfirmed = nil
}

func (e *providerEditor) targetHosts(m *Model) []ProviderHost {
	if m.ProviderActions.Hosts == nil {
		return nil
	}
	hosts := m.ProviderActions.Hosts()
	for _, node := range e.targets {
		if !slices.ContainsFunc(hosts, func(host ProviderHost) bool { return host.Node == node }) {
			hosts = append(hosts, ProviderHost{Node: node, Name: node})
		}
	}
	return hosts
}

func (e *providerEditor) normalizedTargets() []string {
	if e.owner == "" {
		return nil
	}
	if !providerAuthAllowsMultipleTargets(e.auth()) {
		return []string{e.owner}
	}
	targets := providerTargetList(e.targets)
	if !slices.Contains(targets, e.owner) {
		targets = append(targets, e.owner)
	}
	return targets
}

// keyUpdate keeps displaying a stored key from creating a new credential
// on a save that changes only the channel's configuration.
func (e *providerEditor) keyUpdate() string {
	if e.key == e.originalKey {
		return ""
	}
	return e.key
}

func (e *providerEditor) login() bool { return e.method.Login != "" }

// billsByRate reports a channel that saves its own billing rate: one the
// Node keeps as API-key authentication, by a key, an optional key or a
// package preset without one. Sign-ins and plugin connections have none.
func (e *providerEditor) billsByRate() bool {
	switch e.method.Auth {
	case preview.AuthKey, viewmodel.AuthOptionalKey, preview.AuthNone, viewmodel.AuthUnresolved:
		return !e.login()
	}
	return false
}

// billingRateValue is the rate a save sends: nil where the channel has none
// or still bills at the 100% it never saved, else the typed rate.
func (e *providerEditor) billingRateValue() *uint32 {
	if !e.billsByRate() {
		return nil
	}
	bps, _ := parseBillingRate(e.billingRate)
	if e.billingRateBase == nil && bps == protocol.DefaultBillingRateBps {
		return nil
	}
	return &bps
}

func cloneBillingRate(bps *uint32) *uint32 {
	if bps == nil {
		return nil
	}
	value := *bps
	return &value
}

// formatBillingRate is a rate in basis points as the percent the form shows,
// with the decimals it has: 10000 is "100", 1250 "12.5", nil "100".
func formatBillingRate(bps *uint32) string {
	value := protocol.DefaultBillingRateBps
	if bps != nil {
		value = *bps
	}
	text := strconv.FormatUint(uint64(value/100), 10)
	if fraction := value % 100; fraction != 0 {
		text += "." + strings.TrimRight(fmt.Sprintf("%02d", fraction), "0")
	}
	return text
}

// parseBillingRate reads a typed percent, a trailing % allowed, into basis
// points. ok is false for a sign, more than two decimals or more than a
// uint32 holds.
func parseBillingRate(text string) (bps uint32, ok bool) {
	text = strings.TrimSpace(strings.TrimSuffix(strings.TrimSpace(text), "%"))
	whole, fraction, _ := strings.Cut(text, ".")
	digits := func(s string) bool {
		return !strings.ContainsFunc(s, func(r rune) bool { return r < '0' || r > '9' })
	}
	if whole+fraction == "" || len(fraction) > 2 || !digits(whole) || !digits(fraction) {
		return 0, false
	}
	var value uint64
	if whole != "" {
		n, err := strconv.ParseUint(whole, 10, 64)
		if err != nil || n > math.MaxUint32 {
			return 0, false
		}
		value = n * 100
	}
	if fraction != "" {
		n, _ := strconv.ParseUint(fraction+strings.Repeat("0", 2-len(fraction)), 10, 64)
		value += n
	}
	if value > math.MaxUint32 {
		return 0, false
	}
	return uint32(value), true
}

// Check validates the editor's configuration: the i18n key of the problem, or
// "".
func (e *providerEditor) Check() string {
	if strings.TrimSpace(e.name) == "" {
		return "provider_name_required"
	}
	if e.current().Custom && !strings.HasPrefix(e.endpoint, "http://") && !strings.HasPrefix(e.endpoint, "https://") {
		return "provider_endpoint_required"
	}
	if e.usesKey() && e.method.Auth != viewmodel.AuthOptionalKey && !e.keySet && strings.TrimSpace(e.key) == "" {
		return "provider_key_required"
	}
	if e.azure() && strings.TrimSpace(e.apiVersion) == "" {
		return "provider_cloud_required"
	}
	if !e.login() {
		if _, ok := parseHeaders(e.headers); !ok {
			return "provider_headers_invalid"
		}
	}
	if e.billsByRate() {
		if _, ok := parseBillingRate(e.billingRate); !ok {
			return "provider_billing_rate_invalid"
		}
	}
	if len(e.models) == 0 && !e.login() {
		return "provider_model_required"
	}
	seen := map[string]bool{}
	for _, d := range e.models {
		id := strings.TrimSpace(d.id)
		if id == "" {
			return "provider_model_required"
		}
		if seen[id] {
			return "provider_model_duplicate"
		}
		seen[id] = true
		if n, err := strconv.ParseUint(d.context, 10, 32); err != nil || n == 0 || n < uint64(d.model.Output) {
			return "provider_limits_invalid"
		}
		// The efforts are edited in place: each named, none twice.
		named := map[string]bool{}
		for _, x := range d.efforts {
			x = strings.TrimSpace(x)
			if x == "" || named[x] {
				return "provider_effort_invalid"
			}
			named[x] = true
		}
	}
	return ""
}

// header is a request header's row as the editor shows it.
type header struct{ Name, Value string }

// parseHeaders reads the header rows; rows left empty are skipped. ok is
// false when a row has a value but no name. The Node checks the names and
// values it accepts.
func parseHeaders(rows []header) (headers map[string]string, ok bool) {
	for _, row := range rows {
		name, value := strings.TrimSpace(row.Name), strings.TrimSpace(row.Value)
		if name == "" {
			if value != "" {
				return nil, false
			}
			continue
		}
		if headers == nil {
			headers = map[string]string{}
		}
		headers[name] = value
	}
	return headers, true
}

// headerRows are headers as rows in name order.
func headerRows(headers map[string]string) []header {
	rows := make([]header, 0, len(headers))
	for _, name := range slices.Sorted(maps.Keys(headers)) {
		rows = append(rows, header{name, headers[name]})
	}
	return rows
}

// save writes the channel; the editor stays open with its draft when
// Check finds a problem.
func (e *providerEditor) save(m *Model) string {
	if problem := e.Check(); problem != "" {
		return problem
	}
	targets := e.normalizedTargets()
	if m.Live && m.ProviderActions.Hosts != nil &&
		(len(targets) == 0 || !providerAuthAllowsMultipleTargets(e.auth()) && len(targets) != 1) {
		return "provider_host_required"
	}
	models := make([]preview.ChannelModel, len(e.models))
	for i, d := range e.models {
		x := d.model
		x.ID = strings.TrimSpace(d.id)
		if x.Name == "" || x.Name == d.model.ID {
			x.Name = x.ID
		}
		x.Context, _ = strconv.Atoi(d.context)
		x.Efforts = nil
		for _, effort := range d.efforts {
			x.Efforts = append(x.Efforts, strings.TrimSpace(effort))
		}
		if len(x.Efforts) > 0 {
			x.DefaultEffort = x.Efforts[min(max(d.def, 0), len(x.Efforts)-1)]
		} else {
			x.DefaultEffort = "default"
		}
		models[i] = x
	}
	auth := e.auth()
	brand := ""
	if e.brand != nil {
		brand = e.brand.ID
	}
	route := e.current()
	rate := e.billingRateValue()
	if m.Live && m.ProviderActions.Save != nil {
		if e.probing {
			return ""
		}
		e.probing, e.saving = true, true
		channel := preview.Channel{Node: e.owner, Group: e.group, Targets: slices.Clone(targets), TargetIDs: maps.Clone(e.targetIDs), TargetRevisions: maps.Clone(e.targetRevisions), ID: e.id, Revision: e.revision, Name: strings.TrimSpace(e.name), Brand: brand, Icon: e.method.Icon, Method: route.ID, API: route.API, Endpoint: e.endpoint, Auth: auth, Enabled: true, KeySet: e.keySet, Models: models, Default: e.def, BillingRateBps: rate}
		if e.azure() {
			channel.APIVersion = strings.TrimSpace(e.apiVersion)
		}
		if !e.login() {
			channel.Headers, _ = parseHeaders(e.headers)
		}
		generation := e.generation
		signIn := e.login() && e.editing == ""
		signInNode, id := e.owner, e.id
		e.targetSucceeded, e.targetFailed, e.targetUncertain = nil, nil, nil
		e.targetUnconfirmed = nil
		save := m.ProviderActions.Save
		if e.resumeSave != nil {
			save = e.resumeSave
		}
		save(ProviderDraft{Channel: channel, Key: e.keyUpdate(), ExpectedRevision: e.revision, EditorGeneration: generation, OAuth: e.oauth, Group: e.group, Targets: slices.Clone(targets), TargetRevisions: maps.Clone(e.targetRevisions)}, func(result ProviderSaveResult) {
			if e.generation != generation || !e.open {
				return
			}
			e.probing, e.saving = false, false
			e.resumeSave = result.Resume
			result = completeProviderSaveResult(targets, result)
			e.targetSucceeded, e.targetFailed = slices.Clone(result.Succeeded), slices.Clone(result.Failed)
			e.targetUncertain = slices.Clone(result.Uncertain)
			e.targetUnconfirmed = slices.Clone(result.Unconfirmed)
			if result.Group != "" || result.TargetIDs != nil {
				e.group = result.Group
			}
			if e.targetIDs == nil {
				e.targetIDs = map[string]string{}
			}
			for node, id := range result.TargetIDs {
				e.targetIDs[node] = id
			}
			if e.targetRevisions == nil {
				e.targetRevisions = map[string]uint64{}
			}
			for node, revision := range result.TargetRevisions {
				e.targetRevisions[node] = revision
			}
			if revision, ok := result.TargetRevisions[e.owner]; ok {
				e.revision = revision
			}
			if len(result.Failed)+len(result.Uncertain)+len(result.Unconfirmed) > 0 {
				if signIn && slices.Contains(result.Succeeded, signInNode) {
					if targetID := result.TargetIDs[signInNode]; targetID != "" {
						id = targetID
					}
					m.signIn(signInNode, id)
				}
				return
			}
			e.open = false
			e.key = ""
			if signIn {
				if targetID := result.TargetIDs[signInNode]; targetID != "" {
					id = targetID
				}
				m.signIn(signInNode, id)
			}
		})
		return ""
	}
	if ch := m.channel(channelKey(e.owner, e.editing)); ch != nil {
		ch.Name, ch.Endpoint, ch.Models, ch.Default = strings.TrimSpace(e.name), e.endpoint, models, e.def
		ch.Method, ch.API = route.ID, route.API
		if e.azure() {
			ch.APIVersion = strings.TrimSpace(e.apiVersion)
		}
		if !e.login() {
			ch.Headers, _ = parseHeaders(e.headers)
		}
		ch.BillingRateBps = rate
		ch.KeySet = ch.KeySet || e.key != ""
		return ""
	}
	id := fmt.Sprintf("%s-%d", e.method.ID, len(m.Channels)+1)
	m.Channels = append(m.Channels, preview.Channel{Node: e.owner, Group: e.group, Targets: slices.Clone(targets), TargetIDs: maps.Clone(e.targetIDs), ID: id, Name: strings.TrimSpace(e.name), Brand: brand, Icon: e.method.Icon, Method: route.ID,
		API: route.API, Endpoint: e.endpoint, Auth: auth, Enabled: true, KeySet: e.key != "", Models: models, Default: e.def, BillingRateBps: rate})
	if e.login() {
		node := e.owner
		if len(targets) > 0 {
			node = targets[0]
		}
		m.signIn(node, id)
	}
	return ""
}

// providerEditorDialog is 560 wide for brands, 540 for methods, 640 to
// configure; a back button and the step's title over it.
func providerEditorDialog(c *ui.Context, m *Model) {
	e := &m.provider
	if !e.open {
		e.key, e.originalKey, e.keyShown = "", "", false
		return
	}
	p := theme.Of(c)
	t := func(k string) string { return i18n.T(c, k) }
	title := map[providerStep]string{stepBrand: "provider_add_title", stepMethod: "provider_choose_method", stepConfigure: "provider_configure"}[e.step]
	if e.editing != "" {
		title = "provider_edit_title"
	}
	w := map[providerStep]float32{stepBrand: 560, stepMethod: 540, stepConfigure: 640}[e.step]
	kit.Dialog(c.Key("provider-editor"), &e.open, func() {
		ui.Row(c.Key("heading")).Gap(8).Children(func() {
			if e.step != stepBrand && e.editing == "" {
				kit.Action(c.Key("back"), icons.ChevronLeft, t("provider_step_back"), false).OnClick(e.back)
			}
			ui.Text(c, t(title)).FontSize(16).FontWeight(600).TextColor(p.PopoverForeground)
		})
		ui.Scroll(c.Key("body")).Grow(1).Shrink(1).Children(func() {
			switch e.step {
			case stepBrand:
				brandPicker(c, m, e.owner, nil, nil)
			case stepMethod:
				methodList(c, m)
			default:
				providerForm(c, m, e)
			}
		})
		if e.step == stepConfigure {
			action := t("provider_save")
			if e.login() && e.editing == "" {
				action = t("provider_save_login")
			}
			footer(c, action, func() { e.open = false }, func() {
				if problem := e.save(m); problem != "" {
					kit.Toast(c, kit.Failed, t(problem))
					return
				}
				if !m.Live {
					e.open = false
				}
			}, e.saving)
		}
	}).Width(min(w, width(c)-48)).Label(t(title))
	if !e.open {
		e.key = ""
	}
}

// loginLabel names an account sign-in: the account's product (ChatGPT,
// Claude), else the method.
func loginLabel(c *ui.Context, x preview.Method) string {
	if x.Login != "" && x.Login != "provider_access_login" {
		return i18n.T(c, x.Login)
	}
	return x.Name
}

// pickerMethods limits the methods offered after selecting a brand to those
// matching its access group. A brand may offer several ways to connect.
func pickerMethods(b *preview.Brand, group string) []preview.Method {
	var methods []preview.Method
	for _, x := range b.Methods {
		access := "provider_access_key"
		if x.Login != "" {
			access = "provider_access_login"
		} else if x.Auth == preview.AuthNone {
			access = "provider_access_no_key"
		}
		if access == group {
			methods = append(methods, x)
		}
	}
	return methods
}

// brandPicker opens the selected brand's only method or asks which method
// to use. The welcome guide wraps the selection with before and after.
func brandPicker(c *ui.Context, m *Model, node string, before, after func()) {
	p := theme.Of(c)
	chip := func(key, label, mark string, pick func()) {
		b := ui.ButtonBase(c.Key(key)).Label(label).Height(32).Padding(0, 14, 0, 10).Gap(8).Radius(999).
			Border(1, p.Border).Background(p.ButtonSecondary).Cursor(ui.CursorPointer).OnClick(func() {
			if m.Live && !m.providerHostAvailable(node) {
				kit.Toast(c, kit.Failed, i18n.T(c, "onboarding_host_unavailable"))
				return
			}
			if !requireProviderCatalog(c, m, node) {
				return
			}
			if before != nil {
				before()
			}
			pick()
			if after != nil {
				after()
			}
		})
		if b.Hovered() {
			b.Background(p.ButtonSecondaryHover)
		}
		b.Children(func() {
			kit.Logo(c, mark, label, 16)
			ui.Text(c, label).FontSize(14).SingleLine()
		})
	}
	group := func(key string, n int, fill func()) {
		if n == 0 {
			return
		}
		ui.Column(c.Key(key)).Gap(8).FillWidth().Children(func() {
			ui.Text(c, i18n.T(c, key)).FontSize(14).TextColor(p.MutedForeground)
			ui.Row(c).Wrap().Gap(8).Children(fill)
		})
	}
	type choice struct {
		brand   *preview.Brand
		methods []preview.Method
		label   string
		mark    string
	}
	ui.Column(c.Key("picker")).Gap(20).Padding(10, 12, 28, 12).Children(func() {
		for _, access := range []string{"provider_access_login", "provider_access_key", "provider_access_no_key"} {
			var choices []choice
			for i := range m.Brands {
				b := &m.Brands[i]
				if node != "" && b.Node != node {
					continue
				}
				methods := pickerMethods(b, access)
				if len(methods) == 0 {
					continue
				}
				x := choice{b, methods, b.Name, b.ID}
				if access == "provider_access_login" && len(methods) == 1 {
					x.label = loginLabel(c, methods[0])
					if icons.Brand(methods[0].Icon) != nil {
						x.mark = methods[0].Icon
					}
				}
				choices = append(choices, x)
			}
			if access == "provider_access_login" {
				slices.SortStableFunc(choices, func(a, b choice) int { return strings.Compare(strings.ToLower(a.label), strings.ToLower(b.label)) })
			}
			group(access, len(choices), func() {
				for _, x := range choices {
					chip(channelKey(x.brand.Node, x.brand.ID), x.label, x.mark, func() { m.chooseProvider(x.brand, x.methods) })
				}
			})
		}
	})
}

// methodList is the brand over the methods its choice offered, each a row
// with how it is accessed and a chevron.
func methodList(c *ui.Context, m *Model) {
	e := &m.provider
	if e.brand == nil {
		return
	}
	ui.Column(c.Key("methods")).Padding(15, 12, 30, 12).Children(func() {
		ui.Row(c).Gap(12).Padding(0, 0, 22, 0).Children(func() {
			ui.Row(c).Size(29, 29).Center().Children(func() { kit.Tile(c, e.brand.Name, icons.Brand(e.brand.ID)) })
			ui.Text(c, e.brand.Name).FontSize(18).FontWeight(500).SingleLine()
		})
		modalList(c, "list", "", "", "", len(e.methods), func() {
			for _, x := range e.methods {
				listRow{subtitle: i18n.T(c, x.Access), chevron: true, onClick: func() { m.configureProvider(x) }}.row(c, channelKey(x.Node, x.ID), x.Name)
			}
		}, nil)
	})
}

// protocolName names a wire protocol: the two OpenAI ones by their i18n
// keys, another as it is written.
func protocolName(c *ui.Context, api string) string {
	switch api {
	case "responses":
		return i18n.T(c, "provider_api_responses")
	case "chat_completions":
		return i18n.T(c, "provider_api_chat_completions")
	}
	return api
}

// routeName names a route in an axis's control.
func routeName(c *ui.Context, r preview.Route, axis routeAxis) string {
	switch {
	case axis == axisProtocol:
		return protocolName(c, r.API)
	case axis == axisRegion && r.Region == "china":
		return i18n.T(c, "provider_region_china")
	case axis == axisRegion:
		return i18n.T(c, "provider_region_global")
	case r.ProductName != "":
		return r.ProductName
	case r.Product == "coding":
		return i18n.T(c, "provider_coding")
	}
	return i18n.T(c, "provider_metered")
}

// routeField is an axis's segmented control where the method has several
// routes along it. An edited channel may change its protocol while keeping
// the same product and region.
func routeField(c *ui.Context, e *providerEditor, key, label string, axis routeAxis) bool {
	choices := e.choices(axis)
	if len(choices) < 2 {
		return false
	}
	names := make([]string, len(choices))
	for i, j := range choices {
		names[i] = routeName(c, methodRoutes(e.method)[j], axis)
	}
	cur := e.current()
	chosen := max(0, slices.IndexFunc(choices, func(j int) bool {
		r := methodRoutes(e.method)[j]
		return map[routeAxis]bool{axisProtocol: r.API == cur.API, axisProduct: r.Product == cur.Product, axisRegion: r.Region == cur.Region}[axis]
	}))
	kit.Field(c, key, label, "", func() {
		if kit.Segmented(c, "segments", &chosen, names, kit.SegmentedProps{Block: true, Disabled: e.probing || e.editing != "" && axis != axisProtocol}).Label(label).Changed() {
			e.switchRoute(choices[chosen])
		}
	})
	return true
}

// providerForm is the preview note, the fields the method needs, Get API key,
// the OAuth fields, and the models.
func providerForm(c *ui.Context, m *Model, e *providerEditor) {
	p := theme.Of(c)
	t := func(k string) string { return i18n.T(c, k) }
	input := func(key, label, hint string, value *string, secret bool) {
		kit.Field(c, key, label, "", func() {
			if secret {
				kit.SecretInput(c, value, &e.keyShown, kit.SecretInputProps{Label: label, Placeholder: hint, Disabled: e.probing}).FillWidth()
				return
			}
			ui.TextInput(c.Key("input"), value).Label(label).Placeholder(hint).FillWidth().Disabled(e.probing)
		})
	}
	ui.Column(c.Key("form")).Gap(16).Children(func() {
		input("name", t("settings_name"), t("provider_name_hint"), &e.name, false)
		providerTargets(c, m, e)
		// The protocol is a choice where the method has several, and the
		// plain name of a method that names its only one.
		if !routeField(c, e, "api", t("provider_api"), axisProtocol) && len(e.method.Routes) == 0 && e.method.API != "" {
			kit.Field(c, "api", t("provider_api"), "", func() {
				ui.Text(c, protocolName(c, e.method.API)).FontSize(14).Height(32).Padding(6, 0).FillWidth().TextColor(p.MutedForeground)
			})
		}
		if e.current().Custom {
			input("endpoint", t("provider_endpoint"), t("provider_endpoint_hint"), &e.endpoint, false)
		}
		routeField(c, e, "product", t("provider_product"), axisProduct)
		routeField(c, e, "region", t("provider_region"), axisRegion)
		if e.azure() {
			input("api-version", t("provider_api_version"), t("form_api_version_hint"), &e.apiVersion, false)
		}
		if e.usesKey() {
			hint := t("provider_key_hint")
			if e.keySet {
				hint = t("backup_keep_secret")
			}
			input("key", t("provider_api_key"), hint, &e.key, true)
		}
		if e.method.KeyURL != "" {
			b := ui.ButtonBase(c.Key("key-link")).Label(t("provider_get_key")).Cursor(ui.CursorPointer).AlignSelf(ui.Start).OnClick(func() {
				if m.Live {
					c.OpenURL(e.method.KeyURL)
				} else {
					notice(c)
				}
			})
			b.Children(func() { ui.Text(c, t("provider_get_key")).FontSize(12).TextColor(p.Link) })
		}
		if e.billsByRate() {
			label := t("provider_billing_rate")
			kit.Field(c, "billing-rate", label, t("provider_billing_rate_hint"), func() { billingRateInput(c, e, label) })
		}
		if e.method.OAuth != ([4]string{}) || !e.login() {
			// A disclosure: the chevron turns down while the settings show.
			label, chevron := t("provider_oauth_advanced"), icons.ChevronRight
			if e.advanced {
				chevron = icons.ChevronDown
			}
			kit.GhostLook(c, ui.ButtonBase(c.Key("advanced")).Label(label).Expanded(e.advanced), false).Height(28).PaddingX(4).Gap(4).AlignSelf(ui.Start).
				Children(func() {
					ui.Icon(c, chevron).FontSize(14).TextColor(theme.Of(c).MutedForeground)
					ui.Text(c, label).FontSize(14).SingleLine()
				}).OnClick(func() {
				e.advanced = !e.advanced
				// Opening starts a channel without headers on an empty row;
				// saving skips rows left empty.
				if e.advanced && len(e.headers) == 0 {
					e.headers = []header{{}}
				}
			})
			if e.advanced {
				for i, k := range []string{"provider_oauth_catalog_version", "provider_oauth_user_agent", "provider_oauth_editor_version", "provider_oauth_editor_plugin_version"} {
					// Only the settings this sign-in has.
					if e.method.OAuth[i] != "" {
						input(k, t(k), "", &e.oauth[i], false)
					}
				}
				// An API-key channel's own request headers; sign-ins keep their client identity.
				if !e.login() {
					// A row a header, as the MCP editor's: its name, its value
					// and its removal; Add header is the + beside the title.
					ui.Column(c.Key("headers")).Gap(8).Children(func() {
						ui.Row(c.Key("title")).Gap(4).AlignItems(ui.Center).Children(func() {
							ui.Text(c, t("provider_headers")).FontSize(14).FontWeight(500).TextColor(theme.Of(c).PopoverForeground)
							kit.Action(c.Key("add"), icons.Plus, t("mcp_add_header"), e.probing).Size(24, 24).
								OnClick(func() { e.headers = append(e.headers, header{}) })
						})
						ui.Column(c.Key("rows")).Gap(6).Children(func() {
							for i := range e.headers {
								ui.Row(c.Key([2]int{e.headersRemoved, i})).Gap(4).Children(func() {
									ui.TextInput(c.Key("name"), &e.headers[i].Name).Label(t("provider_headers") + " " + t("mcp_slot_name")).
										Placeholder(t("mcp_slot_name")).Width(200).Font("monospace").Disabled(e.probing)
									ui.TextInput(c.Key("value"), &e.headers[i].Value).Label(t("provider_headers") + " " + t("mcp_slot_value")).
										Placeholder(t("mcp_slot_value")).Grow(1).Font("monospace").Disabled(e.probing)
									kit.Action(c.Key("remove"), icons.Close, t("settings_delete"), e.probing).OnClick(func() {
										e.headers = slices.Delete(e.headers, i, i+1)
										e.headersRemoved++
									})
								})
							}
						})
					})
				}
			}
		}
		ui.Row(c.Key("models-heading")).Gap(8).Wrap().AlignItems(ui.Center).Children(func() {
			ui.Text(c, t("provider_models")).FontSize(14).FontWeight(500).Grow(1)
			label := t("provider_discover")
			if e.probing {
				label = t("provider_querying")
			}
			kit.SecondaryBusy(c.Key("discover"), label, e.probing && !e.saving).Disabled(e.probing).OnClick(func() {
				if !e.login() && !requireProviderCatalog(c, m, e.owner) {
					return
				}
				if m.Live && m.ProviderActions.Discover != nil {
					e.probing = true
					generation := e.generation
					draft := ProviderDraft{Channel: preview.Channel{Node: e.owner, Group: e.group, Targets: slices.Clone(e.targets), TargetIDs: maps.Clone(e.targetIDs), TargetRevisions: maps.Clone(e.targetRevisions), ID: e.id, API: e.current().API, Endpoint: e.endpoint, Method: e.current().ID, APIVersion: strings.TrimSpace(e.apiVersion)}, Key: e.keyUpdate(), OAuth: e.oauth, ExpectedRevision: e.revision, EditorGeneration: generation, Group: e.group, Targets: slices.Clone(e.targets), TargetRevisions: maps.Clone(e.targetRevisions)}
					m.ProviderActions.Discover(draft, func(models []preview.ChannelModel) {
						if e.generation != generation || !e.open {
							return
						}
						e.probing = false
						for _, x := range models {
							if !slices.ContainsFunc(e.models, func(d modelDraft) bool { return d.id == x.ID }) {
								e.models = append(e.models, draftsOf([]preview.ChannelModel{x})...)
							}
						}
					})
					return
				}
				// The preview's listing: Discovered merged in, manual models
				// and the default kept.
				for _, x := range preview.Discovered {
					if !slices.ContainsFunc(e.models, func(d modelDraft) bool { return d.id == x.ID }) {
						e.models = append(e.models, draftsOf([]preview.ChannelModel{x})...)
					}
				}
				kit.Toast(c, kit.Done, t("provider_discovered_preview"))
			})
			kit.Secondary(c.Key("add-model"), t("provider_add_model")).Disabled(e.probing).OnClick(func() {
				if !requireProviderCatalog(c, m, e.owner) {
					return
				}
				// A model typed in starts with the tags a save gives it.
				e.models = append(e.models, modelDraft{open: true, model: preview.ChannelModel{Vision: true, Tools: true}})
			})
		})
		modalList(c, "models", "", "", t("provider_models_empty"), len(e.models), func() {
			for i := range e.models {
				e.models[i].pending = e.probing
				providerModel(c, m, e, i)
			}
		}, nil)
	})
}

// billingRateInput is the rate's number input with % at its end, marked
// invalid while the text is not a rate; the draft stays as typed. MyGo has
// no trailing input slot, so the sign sits over the input's end padding as
// SecretInput's reveal button does, passing pointer input to the field.
func billingRateInput(c *ui.Context, e *providerEditor, label string) {
	_, valid := parseBillingRate(e.billingRate)
	ui.Box(c.Key("billing-rate")).FillWidth().Children(func() {
		ui.TextInput(c.Key("input"), &e.billingRate).Label(label + " (%)").Placeholder(formatBillingRate(nil)).
			Invalid(!valid).PaddingEnd(32).FillWidth().Disabled(e.probing)
		ui.Row(c.Key("unit")).Absolute().Top(0).Bottom(0).End(12).AlignItems(ui.Center).IgnoreInput(true).Children(func() {
			ui.Text(c, "%").FontSize(14).TextColor(theme.Of(c).MutedForeground)
		})
	})
}

// providerTargets is the form's host choice: one multi-select dropdown of
// every host, the channel's own host checked and fixed, the others toggled
// as copy destinations. It shows only for an auth that can be copied and
// while another host can be chosen; otherwise the channel stays where it is.
func providerTargets(c *ui.Context, m *Model, e *providerEditor) {
	hosts := e.targetHosts(m)
	if providerAuthAllowsMultipleTargets(e.auth()) && slices.ContainsFunc(hosts, func(host ProviderHost) bool {
		return host.Node != e.owner && (host.Connected || slices.Contains(e.targets, host.Node))
	}) {
		providerHostSelect(c, hosts, e)
	}
}

func providerHostSelect(c *ui.Context, hosts []ProviderHost, e *providerEditor) {
	label := i18n.T(c, "provider_hosts")
	names := make([]string, 0, len(e.targets)+1)
	for _, host := range hosts {
		if host.Node == e.owner || slices.Contains(e.targets, host.Node) {
			names = append(names, providerHostName(c, host))
		}
	}
	const id = "provider-hosts"
	kit.Field(c, "hosts", label, "", func() {
		kit.MenuButton(c.Key("hosts"), id, func(b ui.Element) {
			kit.SelectTrigger(c, b.Label(label).Disabled(e.probing), strings.Join(names, ", "), false, kit.MenuOpen(c, id)).FillWidth()
		}, func(menu *kit.Menu) {
			for _, host := range hosts {
				owner := host.Node == e.owner
				selected := owner || slices.Contains(e.targets, host.Node)
				item := menu.Item(providerHostName(c, host)).Icon(icons.Platform(host.Platform)).Checked(selected).
					Disabled(owner || !host.Connected && !selected)
				if !host.Connected {
					item.Trailing(i18n.T(c, "connections_offline"))
				}
				if item.Chosen() && !owner {
					e.toggleCopyTarget(host.Node)
					kit.OpenMenu(c, id, true)
				}
			}
		}, kit.MenuOptions{CheckRight: true, MaxHeight: 320, Match: true})
	})
}

func providerHostName(c *ui.Context, host ProviderHost) string {
	if host.Local {
		return i18n.T(c, "composer_host_local")
	}
	if host.Name != "" {
		return i18n.T(c, host.Name)
	}
	return host.Node
}

func (m *Model) providerHostAvailable(node string) bool {
	if node != "" && m.ProviderActions.Hosts != nil {
		return slices.ContainsFunc(m.ProviderActions.Hosts(), func(host ProviderHost) bool {
			return host.Node == node && host.Connected
		})
	}
	return m.ProviderActions.Available == nil || m.ProviderActions.Available()
}

// providerModel is the model's title, Default, its check to make it the
// default and its removal; open, its ID, context limit and reasoning efforts.
func providerModel(c *ui.Context, m *Model, e *providerEditor, i int) {
	p := theme.Of(c)
	t := func(k string) string { return i18n.T(c, k) }
	d := &e.models[i]
	title := d.id
	if title == d.model.ID && d.model.Name != "" {
		title = d.model.Name
	}
	if title == "" {
		title = t("provider_new_model")
	}
	selected := e.def == i
	meta := ""
	if selected {
		meta = t("provider_model_default")
	}
	listRow{meta: meta, expanded: &d.open, onClick: func() { d.open = !d.open },
		trailing: func() {
			tip := t("provider_set_default")
			if selected {
				tip = t("provider_default")
			}
			b := kit.Action(c.Key("default"), icons.Check, tip, selected).Disabled(e.probing)
			if selected {
				b.TextColor(p.Success).Opacity(1)
			}
			b.OnClick(func() { e.def = i })
		},
		remove: func() {
			if e.probing {
				return
			}
			e.models = slices.Delete(e.models, i, i+1)
			e.removed++
			switch {
			case e.def == i:
				e.def = 0
			case e.def > i:
				e.def--
			}
		}, removeLabel: t("settings_delete"),
		content: func() {
			e.lookupModelCapabilities(c, m, d)
			modelForm(c, d, e.current().API)
		},
	}.row(c, [2]int{e.removed, i}, title)
}

// modelForm is the model ID, the context limit with its common sizes, and the
// reasoning efforts with the default. An Azure OpenAI model is named by its
// deployment.
func modelForm(c *ui.Context, d *modelDraft, api string) {
	p := theme.Of(c)
	t := func(k string) string { return i18n.T(c, k) }
	name := t("provider_model_id")
	if api == "azure_open_ai" {
		name = t("provider_deployment_id")
	}
	ui.Column(c.Key("model-form")).Gap(16).Children(func() {
		kit.Field(c, "id", name, "", func() {
			ui.TextInput(c.Key("input"), &d.id).Label(name).Placeholder(t("form_model_hint")).FillWidth().Disabled(d.pending)
		})
		// The label with the common sizes at its right, the input under
		// them at full width.
		ui.Column(c.Key("context")).Gap(8).Children(func() {
			ui.Row(c).Gap(6).AlignItems(ui.Center).Children(func() {
				ui.Text(c, t("provider_context")).FontSize(14).FontWeight(500).Grow(1).Shrink(1).MinWidth(0).SingleLine().
					TextColor(p.PopoverForeground)
				for _, n := range []int{128000, 200000, 400000, 1000000} {
					s := strconv.Itoa(n)
					b := kit.Ghost(c.Key(n), tokens(c, n), d.context == s).Height(24).PaddingX(8).Border(1, p.Border).Shrink(0).Disabled(d.pending)
					b.Children(func() { ui.Text(c, tokens(c, n)).FontSize(12) }).OnClick(func() { d.context = s })
				}
			})
			ui.TextInput(c.Key("input"), &d.context).Label(t("provider_context")).Placeholder(t("form_tokens_hint")).FillWidth().Disabled(d.pending)
		})
		capabilityForm(c, d, api)
		if d.model.ID == "" || len(d.model.Efforts) > 0 {
			effortsForm(c, d)
		}
	})
}

// effortsForm is the model's reasoning efforts as tags in one field, an
// input after them: Enter or a comma adds what it holds, Backspace in it
// empty removes the last, and each tag has its removal. A click on a tag
// makes it the default, which shows in the primary tint and stays the
// effort it was as others go. CrychicUI's TagInput has neither a default
// nor a tag's own removal, and takes its input apart from its tags.
func effortsForm(c *ui.Context, d *modelDraft) {
	p := theme.Of(c)
	th := c.Theme()
	t := func(k string) string { return i18n.T(c, k) }
	if d.rejected {
		d.rejected = false
		kit.Toast(c, kit.Failed, t("provider_effort_invalid"))
	}
	// add takes a typed effort; it reports whether the input may clear.
	add := func(text string) bool {
		x := strings.TrimSpace(text)
		switch {
		case x == "":
			return true
		case slices.Contains(d.efforts, x):
			d.rejected = true
			return false
		}
		d.efforts = append(d.efforts, x)
		return true
	}
	remove := func(j int) {
		d.efforts = slices.Delete(d.efforts, j, j+1)
		if d.def > j {
			d.def--
		}
		if d.def >= len(d.efforts) {
			d.def = 0
		}
	}
	kit.Field(c, "efforts", t("provider_efforts"), "", func() {
		field := ui.Row(c.Key("field")).FillWidth().Wrap().GapX(6).GapY(6).AlignItems(ui.Center).
			Padding(th.Space(1.5), th.Space(2.5)).Radius(th.Radius).Background(th.Surface).Border(1, th.Border)
		if c.FocusWithin(d.effortInput) {
			field.Border(1, p.Ring)
		}
		field.Children(func() {
			for j, x := range d.efforts {
				effortTag(c, j, x, j == d.def, d.pending, func() { d.def = j }, func() { remove(j) })
			}
			in := ui.TextInput(c.Key("input"), &d.effort).Label(t("provider_effort_placeholder")).Placeholder(t("provider_effort_placeholder")).
				Grow(1).MinWidth(120).Padding(0).Background(ui.Transparent).Border(0, ui.Transparent).Disabled(d.pending).Bind(&d.effortInput)
			in.OnTextKey(func(k ui.TextKey) bool {
				switch {
				case k.Key == ui.KeyEnter && k.Mods == 0:
					if add(k.Text()) {
						k.SetText("")
					}
					return true
				case k.Key == ui.KeyBackspace && k.Text() == "" && len(d.efforts) > 0:
					remove(len(d.efforts) - 1)
					return true
				}
				return false
			})
			if typed, ok := strings.CutSuffix(d.effort, ","); ok {
				d.effort = typed
				if add(typed) {
					d.effort = ""
				}
			}
		})
	})
}

// effortTag is one effort in the efforts field: its name, a click on which
// makes it the default, and beside it its removal, two buttons in one
// pill.
func effortTag(c *ui.Context, j int, name string, isDefault, pending bool, choose, remove func()) {
	p := theme.Of(c)
	tip := i18n.T(c, "provider_set_default")
	if isDefault {
		tip = i18n.T(c, "provider_effort_default")
	}
	pill := ui.Row(c.Key(j)).Height(24).PaddingEnd(2).Radius(999).AlignItems(ui.Center).Shrink(0).TextColor(p.Foreground)
	pill.Children(func() {
		b := ui.ButtonBase(c.Key("choose")).Label(name + " · " + tip).Height(24).PaddingStart(8).PaddingEnd(2).Gap(2).
			AlignItems(ui.Center).Cursor(ui.CursorPointer).Disabled(pending).AccessSelected(isDefault)
		kit.Tooltip(c, b, tip)
		b.OnClick(choose).Children(func() {
			if isDefault {
				ui.Icon(c, icons.Check).FontSize(12).Shrink(0)
			}
			ui.Text(c, name).FontSize(13).SingleLine()
		})
		switch {
		case isDefault:
			pill.Background(p.Primary.Alpha(0.14)).TextColor(p.Primary)
		case b.Hovered():
			pill.Background(p.Hover)
		default:
			pill.Background(p.Muted)
		}
		kit.IconButton(c.Key("remove"), icons.Close, i18n.T(c, "settings_delete"), 20, false).Disabled(pending).OnClick(remove)
	})
}

// inputLabels name ModelInputs' kinds in the capability form, by i18n key.
var inputLabels = map[string]string{"text": "provider_cap_text", "image": "media_image", "video": "media_video",
	"audio": "provider_cap_audio_input", "pdf": "provider_cap_pdf"}

// setInput ticks or clears an input kind. An unknown list first takes the
// inputs it stood for, so the other kinds stay as shown; Vision follows
// the image input.
func (d *modelDraft) setInput(kind string, on bool) {
	x := &d.model
	inputs := []string{}
	for _, k := range viewmodel.ModelInputs {
		if k == kind && on || k != kind && x.AcceptsInput(k) {
			inputs = append(inputs, k)
		}
	}
	x.Inputs = inputs
	x.Vision = x.AcceptsInput("image")
}

// capabilityForm shows the native output metadata beside the editable
// input, function and generation tags. Manual tags survive catalog refresh.
func capabilityForm(c *ui.Context, d *modelDraft, api string) {
	p := theme.Of(c)
	t := func(k string) string { return i18n.T(c, k) }
	x := &d.model
	check := func(group, key, name string, on *bool) bool {
		changed := ui.Checkbox(c.Key(key), on, t(name)).Label(t(group) + " " + t(name)).Disabled(d.pending).Changed()
		x.CustomCapabilities = x.CustomCapabilities || changed
		return changed
	}
	input := func(kind string) {
		on := x.AcceptsInput(kind)
		if check("provider_cap_input", "input-"+kind, inputLabels[kind], &on) {
			d.setInput(kind, on)
		}
	}
	generates := func(key, name string, on bool, set func(bool)) {
		if check("provider_cap_generates", key, name, &on) {
			set(on)
		}
	}
	toggle := func(value string) func(bool) {
		return func(on bool) {
			x.Generates = slices.DeleteFunc(x.Generates, func(v string) bool { return v == value })
			if on {
				x.Generates = append(x.Generates, value)
			}
		}
	}
	// A group is a row of the capability table: its label in the first
	// column, sized to the longest, with its tooltip, and its boxes beside it.
	group := func(key, tip string, boxes func()) {
		label := ui.Text(c.Key(key+"-label"), t(key)).FontSize(14).TextColor(p.MutedForeground).SingleLine().AlignSelf(ui.Center)
		if tip != "" {
			kit.Tooltip(c, label, t(tip))
		}
		ui.Row(c.Key(key)).Wrap().GapX(16).GapY(6).AlignItems(ui.Center).MinWidth(0).Children(boxes)
	}
	ui.Column(c.Key("capabilities")).Gap(8).Children(func() {
		// The heading with Follow catalog at its right, which wraps under it
		// in a narrow form.
		ui.Row(c.Key("head")).Wrap().GapX(12).GapY(6).AlignItems(ui.Center).Children(func() {
			ui.Text(c.Key("heading"), t("provider_capabilities")).FontSize(14).FontWeight(500).SingleLine().Grow(1).
				TextColor(p.PopoverForeground)
			ui.Row(c.Key("follow")).Gap(8).AlignItems(ui.Center).Shrink(0).Children(func() {
				follow := !x.CustomCapabilities
				label := ui.Text(c.Key("label"), t("provider_cap_follow_catalog")).FontSize(13).SingleLine().TextColor(p.MutedForeground)
				if follow && x.Inputs == nil && strings.TrimSpace(d.id) != "" {
					kit.Tooltip(c, label, t("provider_cap_catalog_unknown"))
				}
				if ui.Switch(c.Key("switch"), &follow).Label(t("provider_cap_follow_catalog")).Disabled(d.pending).Changed() {
					if follow {
						d.followCatalog()
					} else {
						// The tags as shown become the user's.
						x.CustomCapabilities = true
					}
				}
			})
		})
		// The form's surface, not the window's, under the unchecked boxes.
		surface := *c.Theme()
		surface.Background = ui.Transparent
		ui.Themed(c, &surface).Background(ui.Transparent).Children(func() {
			ui.Grid(c).ColumnTracks(ui.FitContent(), ui.Fr(1)).GapX(16).GapY(10).Children(func() {
				// Inputs are what the model accepts, not what every protocol
				// and attachment kind sends.
				group("provider_cap_input", "provider_cap_input_hint", func() {
					for _, kind := range viewmodel.ModelInputs {
						input(kind)
					}
				})
				var outputs []string
				id := strings.TrimSpace(d.id)
				if x.ID == id || d.catalog != nil && d.catalog.ID == id {
					for _, kind := range x.Outputs {
						if key := inputLabels[kind]; key != "" {
							outputs = append(outputs, t(key))
						}
					}
				}
				if len(outputs) != 0 {
					group("provider_cap_output", "", func() {
						ui.Text(c.Key("native"), strings.Join(outputs, ", ")).FontSize(14).TextColor(p.PopoverForeground)
					})
				}
				group("provider_cap_functions", "", func() {
					check("provider_cap_functions", "tools", "provider_cap_tools", &x.Tools)
					check("provider_cap_functions", "reasoning", "provider_cap_reasoning", &x.Reasoning)
				})
				group("provider_cap_generates", "", func() {
					// The image route follows the model's output kind and the
					// provider protocol, rather than treating a tool as an endpoint.
					image := slices.Contains(x.Generates, viewmodel.GenerateImage) || slices.Contains(x.Generates, viewmodel.GenerateImageTool)
					generates("image", "media_image", image, func(on bool) {
						toggle(viewmodel.GenerateImage)(false)
						toggle(viewmodel.GenerateImageTool)(false)
						if on {
							kind := d.imageGeneration(api)
							d.hosted = kind == viewmodel.GenerateImageTool
							toggle(kind)(true)
						}
					})
					generates("video", "media_video", slices.Contains(x.Generates, viewmodel.GenerateVideo), toggle(viewmodel.GenerateVideo))
					generates("speech", "media_speech", slices.Contains(x.Generates, viewmodel.GenerateSpeech), toggle(viewmodel.GenerateSpeech))
				})
			})
		})
	})
}

// ProviderPicker is the providers' brand picker for the welcome guide
// (Workspace::provider_picker): a choice opens the add editor on that
// brand or login, then runs chosen, which shows Providers. back reopens
// the guide from the editor's first Back (ReturnTo).
func ProviderPicker(c *ui.Context, s *State, chosen, back func()) {
	var node string
	if s.Live && s.ProviderActions.Hosts != nil {
		for _, host := range s.ProviderActions.Hosts() {
			if host.Local {
				node = host.Node
				break
			}
		}
	}
	brandPicker(c, s.Model, node, func() { s.provider.addOn(s.Model, node) }, func() {
		s.ManagedNode = ""
		s.provider.returnTo = back
		chosen()
	})
}

func providerAuthorizationDialog(c *ui.Context, m *Model) {
	view := m.ProviderAuthorization()
	authorization(c, "login", view, func() {
		if ch := m.channel(m.loginFor); ch != nil {
			channelTile(c, ch, 48)
		}
	}, view.Retry, func() {
		if strings.TrimSpace(view.Pasted) == "" {
			kit.Toast(c, kit.Failed, i18n.T(c, "provider_login_callback_required"))
			return
		}
		if view.Submit != nil {
			view.Submit(view.Pasted)
		} else {
			notice(c)
		}
	})
	if w := &m.loginWarning; w.open {
		proceed, cancel := w.proceed, w.cancel
		confirmed := confirm(c, "login-warning", &w.open, i18n.T(c, w.verb)+" "+w.title, w.text, i18n.T(c, "provider_login_continue"))
		if confirmed && proceed != nil {
			proceed()
		} else if !w.open && cancel != nil {
			cancel()
		}
		if !w.open {
			*w = loginWarning{}
		}
	}
}

// providerHostLabel is label with its host's name when several hosts are
// connected.
func providerHostLabel(c *ui.Context, m *Model, node, label string) string {
	if node == "" || m.ProviderActions.Hosts == nil {
		return label
	}
	hosts := m.ProviderActions.Hosts()
	if len(hosts) < 2 {
		return label
	}
	for _, host := range hosts {
		if host.Node == node {
			return label + " (" + providerHostName(c, host) + ")"
		}
	}
	return label + " (" + node + ")"
}
