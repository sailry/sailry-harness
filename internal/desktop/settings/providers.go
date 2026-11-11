package settings

import (
	"fmt"
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
)

// The Providers page (settings/providers): the model catalog with the
// compaction threshold, the channels filtered by family and search, each
// expanding to its models, and the editor that adds or edits one: brand,
// method, then its configuration and models. Account logins sign in in
// the authorization dialog.

var compactionMessages = Messages{"", "compaction_settings_failed", "compaction_settings_conflict", "compaction_settings_unknown"}

// MediaMessages are the toasts of a media models save.
var MediaMessages = Messages{"", "media_failed", "media_conflict", "media_unknown"}

// chips is how many model IDs an expanded row shows.
const chips = 8

// ProviderStatus is what a channel lacks: its key or its login; "" when it
// lacks nothing.
func ProviderStatus(ch *preview.Channel) string {
	switch {
	case ch.Auth == preview.AuthKey && !ch.KeySet:
		return "provider_status_missing_key"
	case ch.Auth == preview.AuthOAuth && !ch.SignedIn:
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
	ui.Column(c).Gap(24).Children(func() {
		catalog(c, m)
		mediaModels(c, m)
		ui.Column(c.Key("channels")).Gap(12).Children(func() {
			var shown []int
			for i := range m.Channels {
				ch := &m.Channels[i]
				if (m.Family == "" || ch.Brand == m.Family) && matches(ch, s.Query) {
					shown = append(shown, i)
				}
			}
			kit.Toolbar(c, i18n.T(c, "provider_channels"), len(m.Channels), &s.Query, i18n.T(c, "provider_list_search"), func() {
				families(c, m)
				iconButton(c, "add", icons.Plus, i18n.T(c, "settings_add"), true, false).OnClick(func() { m.provider.add(m) })
			})
			if len(shown) == 0 {
				kit.Result(c, kit.ResultProps{Icon: icons.Cpu, Title: i18n.T(c, "provider_empty_live"), Card: true})
				return
			}
			kit.Card(c, func() {
				for _, i := range shown {
					channelRow(c, m, &m.Channels[i])
				}
			})
		})
	})
	providerEditorDialog(c, m)
	authorization(c, "login", &m.login, func() {
		if ch := m.channel(m.loginFor); ch != nil {
			channelTile(c, ch, 48)
		}
	}, func() { m.signIn(m.loginFor) }, func() {
		if strings.TrimSpace(m.login.Pasted) == "" {
			kit.Toast(c, kit.Failed, i18n.T(c, "provider_login_callback_required"))
			return
		}
		if m.Live && m.ProviderActions.SubmitLogin != nil {
			m.ProviderActions.SubmitLogin(m.login.Pasted)
		} else {
			notice(c)
		}
	})
	if ch := m.channel(m.removing); ch != nil {
		open := true
		if confirm(c, "delete", &open, i18n.T(c, "provider_delete_title")+" "+ch.Name, i18n.T(c, "provider_remove_effect"), i18n.T(c, "settings_delete")) {
			if m.Live && m.ProviderActions.Remove != nil {
				m.ProviderActions.Remove(ch.ID, ch.Revision)
			} else {
				m.Channels = slices.DeleteFunc(m.Channels, func(x preview.Channel) bool { return x.ID == ch.ID })
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
	state := func(ok bool) func() {
		return func() {
			key, col := "provider_catalog_never", p.MutedForeground
			if ok {
				key, col = "provider_catalog_updated", p.Success
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
			kit.Row(c, "status", i18n.T(c, "provider_catalog_status"), "", state(m.Catalog.Updated))
			kit.Row(c, "scores", i18n.T(c, "provider_scores"), "", state(m.Catalog.Scores))
			last := m.Catalog.Last
			if last == "" {
				last = i18n.T(c, "provider_catalog_never")
			}
			kit.Row(c, "last", i18n.T(c, "provider_catalog_last_update"), "", func() {
				ui.Text(c, last).FontSize(14).TextColor(p.MutedForeground)
			})
			t := i18n.T(c, "provider_compaction_threshold")
			kit.Row(c, "compaction", t, "", func() {
				ui.Row(c).Width(240).Gap(12).Children(func() {
					if ui.StepSlider(c.Key("slider"), &m.Compaction.Draft, 50, 95, 5).Label(t).Disabled(m.Compaction.Pending).Grow(1).Changed() {
						if m.Live && m.ProviderActions.SaveCompaction != nil {
							m.ProviderActions.SaveCompaction()
						} else {
							report(c, m.Compaction.Commit(), compactionMessages)
						}
					}
					if m.Live && m.Compaction.Pending {
						ui.Spinner(c).Size(14, 14) // the threshold is being saved
					}
					ui.Textf(c, "%.0f%%", m.Compaction.Draft).FontSize(14).Width(40).TextAlign(ui.End)
				})
			})
		})
	})
}

// mediaModels is the model each media purpose uses: Vision, Image, Video and
// Speech, each a select of the models tagged for it under Follow conversation
// model. A choice saves at once; a failed save keeps it as the draft. A saved
// model that can no longer run is marked and stays the select's choice.
func mediaModels(c *ui.Context, m *Model) {
	p := theme.Of(c)
	kit.Group(c, "media", i18n.T(c, "settings_media"), "", nil, func() {
		kit.Card(c, func() {
			for i, kind := range viewmodel.MediaKinds {
				labels, values := []string{i18n.T(c, "media_follow")}, []string{""}
				for _, option := range m.MediaOptions {
					if slices.Contains(option.Kinds, kind) || option.Key == m.Media.Draft[i] {
						labels, values = append(labels, option.Label), append(values, option.Key)
					}
				}
				title := i18n.T(c, mediaTitles[i])
				kit.WideRow(c, kind, title, "", func() {
					if m.MediaUnavailable[i] && m.Media.Draft[i] != "" && m.Media.Draft[i] == m.Media.Saved[i] {
						ui.Row(c.Key("status")).Shrink(0).Gap(6).Children(func() {
							ui.Box(c).Size(6, 6).Radius(3).Background(p.Warning)
							ui.Text(c, i18n.T(c, "media_model_unavailable")).FontSize(14).TextColor(p.MutedForeground).SingleLine()
						})
					}
					b, changed := kit.Select(c.Key("select"), "media-"+kind, title, &m.Media.Draft[i], labels, m.Media.Pending, values)
					b.Grow(1).Shrink(1).MinWidth(0)
					if !changed {
						return
					}
					if m.Live && m.SaveMedia != nil {
						m.SaveMedia()
					} else {
						report(c, m.Media.Commit(), MediaMessages)
					}
				})
			}
		})
	})
}

// mediaTitles are the media rows' titles, in viewmodel.MediaKinds' order.
var mediaTitles = [4]string{"provider_cap_vision", "media_image", "media_video", "media_speech"}

// capabilityMarks are the small icons of what a model understands and
// generates, each named by its tooltip.
func capabilityMarks(c *ui.Context, x *preview.ChannelModel) {
	p := theme.Of(c)
	for _, kind := range viewmodel.Capabilities(x.Vision, x.Generates) {
		label := i18n.T(c, viewmodel.CapabilityLabels[kind])
		kit.Tooltip(c, ui.Icon(c.Key(kind), icons.Capabilities[kind]).FontSize(12).Shrink(0).TextColor(p.MutedForeground).Label(label), label)
	}
}

// families is All, then each brand, the list's filter.
func families(c *ui.Context, m *Model) {
	labels := []string{i18n.T(c, "provider_all")}
	ids := []string{""}
	for _, b := range m.Brands {
		labels = append(labels, b.Name)
		ids = append(ids, b.ID)
	}
	chosen := slices.Index(ids, m.Family)
	if pick(c, "families", i18n.T(c, "provider_type"), &chosen, labels, false).Changed() {
		m.Family = ids[chosen]
	}
}

func channelTile(c *ui.Context, ch *preview.Channel, size float32) {
	ui.Row(c).Size(size, size).Center().Children(func() { kit.Tile(c, ch.Name, icons.Providers[ch.Brand]) })
}

// channelRow is the channel, its quota, what it lacks, its switch and menu;
// clicking it shows its models.
func channelRow(c *ui.Context, m *Model, ch *preview.Channel) {
	p := theme.Of(c)
	ui.Column(c.Key(ch.ID)).Children(func() {
		ui.Row(c.Key("row")).Padding(12, 0).Gap(12).Children(func() {
			desc := ch.API + " · " + i18n.T(c, "provider_model_count", "count", len(ch.Models))
			if !m.Live {
				desc = i18n.T(c, "preview") + " · " + desc
			}
			b := ui.ButtonBase(c.Key("details")).Label(ch.Name).Grow(1).Shrink(1).MinWidth(0).Justify(ui.Start).Cursor(ui.CursorPointer).
				Opacity(enabledOpacity(ch.Enabled)).OnClick(func() {
				if m.expanded == ch.ID {
					m.expanded = ""
				} else {
					m.expanded = ch.ID
				}
			})
			b.Children(func() {
				kit.MediaRow(c, ch.Name, desc, func() { kit.Tile(c, ch.Name, icons.Providers[ch.Brand]) }, nil)
			})
			if ch.Quota != nil && ch.Auth == preview.AuthOAuth {
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
			if ui.Switch(c.Key("enabled"), &ch.Enabled).Label(i18n.T(c, "provider_enabled")+" "+ch.Name).Changed() && m.Live && m.ProviderActions.Enable != nil {
				m.ProviderActions.Enable(ch.ID, ch.Revision, ch.Enabled)
			}
			channelMenu(c, m, ch)
		})
		if m.expanded == ch.ID {
			channelModels(c, ch)
		}
	})
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
	kit.MenuButton(c.Key("menu"), "provider-menu-"+ch.ID, func(b ui.Element) {
		kit.IconLook(c, b, icons.Ellipsis, i18n.T(c, "more"), "", 28, false)
	}, func(menu *Menu) {
		if menu.Item(i18n.T(c, "settings_edit")).Icon(icons.Pencil).Chosen() {
			m.provider.edit(m, ch)
		}
		if ch.Auth == preview.AuthOAuth {
			key := "provider_connect"
			if ch.Account != "" || ch.SignedIn {
				key = "provider_reconnect"
			}
			if menu.Item(i18n.T(c, key)).Icon(icons.CircleUser).Chosen() {
				m.signIn(ch.ID)
			}
		}
		menu.Separator()
		if menu.Item(i18n.T(c, "settings_delete")).Icon(icons.Trash).Danger().Chosen() {
			m.removing = ch.ID
		}
	}, kit.MenuOptions{At: ui.AnchorBottomEnd, Self: ui.AnchorTopEnd})
}

// Menu is kit.Menu, named here for the builders' signatures.
type Menu = kit.Menu

func (m *Model) channel(id string) *preview.Channel {
	if id == "" {
		return nil
	}
	for i := range m.Channels {
		if m.Channels[i].ID == id {
			return &m.Channels[i]
		}
	}
	return nil
}

// signIn opens the authorization of a channel's account login. The
// preview has no provider: ChatGPT shows a device code, Claude its
// browser page with the pasted callback.
func (m *Model) signIn(id string) {
	if m.Live {
		if m.ProviderActions.SignIn != nil {
			m.ProviderActions.SignIn(id)
		}
		return
	}
	ch := m.channel(id)
	if ch == nil {
		return
	}
	m.loginFor = id
	m.login = Authorization{Open: true, Title: ch.Name, Flow: FlowBrowser, URL: "https://claude.ai/oauth/authorize", Callback: true}
	if ch.Method == "chatgpt-login" {
		m.login = Authorization{Open: true, Title: ch.Name, Flow: FlowDevice, Code: "PREV-IEW0", URL: "https://auth.openai.com/codex/device", Expires: "14:52"}
	}
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
	model   preview.ChannelModel
	id      string
	context string
	efforts []string
	effort  string // the effort being added
	def     int    // the default effort
	open    bool
	// hosted keeps the image tool the model came with: ticking Image again
	// restores it rather than the images endpoint.
	hosted bool
	// catalog is the model as it came when its tags were the catalog's,
	// which a reset of the tags shows again; nil when they were the user's.
	catalog *preview.ChannelModel
}

// providerEditor is the step, the brand and method chosen, the fields and the
// models. editing is the channel edited, "" for a new one.
type providerEditor struct {
	generation          uint64
	id                  string
	revision            uint64
	open                bool
	step                providerStep
	editing             string
	brand               *preview.Brand
	method              preview.Method
	name, endpoint, key string
	keySet              bool
	oauth               [4]string // catalog, user agent, editor, editor plugin versions
	advanced            bool
	models              []modelDraft
	removed             int // models taken out, part of a model row's identity (mcpEditor.removed)
	def                 int
	probing             bool
	saving              bool // probing is the save's, not a discovery's
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
		x.Generates = slices.Clone(x.Generates)
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
	*e = providerEditor{open: true, step: stepBrand, generation: e.generation + 1}
	if m.Live && m.ProviderActions.NewID != nil {
		e.id = m.ProviderActions.NewID()
	}
}

func (e *providerEditor) edit(m *Model, ch *preview.Channel) {
	*e = providerEditor{open: true, step: stepConfigure, editing: ch.ID, id: ch.ID, generation: e.generation + 1, name: ch.Name, endpoint: ch.Endpoint,
		keySet: ch.KeySet, models: draftsOf(ch.Models), def: ch.Default}
	e.revision = ch.Revision
	e.oauth = ch.OAuth
	if m.Live && ch.Auth == preview.AuthKey && m.ProviderActions.ReadKey != nil {
		id := ch.ID
		generation := e.generation
		m.ProviderActions.ReadKey(id, func(set bool) {
			if e.open && e.generation == generation && e.editing == id {
				e.keySet = set
			}
		})
	}
	for i := range m.Brands {
		b := &m.Brands[i]
		for _, x := range b.Methods {
			if x.ID == ch.Method {
				e.brand, e.method = b, x
			}
		}
	}
	if e.brand == nil {
		e.method = preview.Method{ID: ch.Method, Name: ch.Name, API: ch.API, Access: "provider_access_key"}
	}
}

// choose takes a brand: one method configures it at once, several ask which.
func (e *providerEditor) choose(b *preview.Brand) {
	e.brand = b
	if len(b.Methods) == 1 {
		e.configure(b.Methods[0])
		return
	}
	e.step = stepMethod
}

func (e *providerEditor) configure(x preview.Method) {
	e.method, e.step = x, stepConfigure
	e.name, e.endpoint = x.Name, x.Endpoint
}

// back is Configure goes to Method when the brand has several, else to Brand.
func (e *providerEditor) back() {
	if e.step == stepConfigure && e.brand != nil && len(e.brand.Methods) > 1 {
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
	return e.method.Access == "provider_access_key" || e.method.Access == "provider_access_cloud"
}

func (e *providerEditor) login() bool { return e.method.Login != "" }

// Check validates the editor's configuration: the i18n key of the problem, or
// "".
func (e *providerEditor) Check() string {
	if strings.TrimSpace(e.name) == "" {
		return "provider_name_required"
	}
	if e.method.Custom && !strings.HasPrefix(e.endpoint, "http://") && !strings.HasPrefix(e.endpoint, "https://") {
		return "provider_endpoint_required"
	}
	if e.usesKey() && !e.keySet && strings.TrimSpace(e.key) == "" {
		return "provider_key_required"
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
	}
	return ""
}

// save writes the channel; the editor stays open with its draft when
// Check finds a problem.
func (e *providerEditor) save(m *Model) string {
	if problem := e.Check(); problem != "" {
		return problem
	}
	models := make([]preview.ChannelModel, len(e.models))
	for i, d := range e.models {
		x := d.model
		x.ID = strings.TrimSpace(d.id)
		if x.Name == "" || x.Name == d.model.ID {
			x.Name = x.ID
		}
		x.Context, _ = strconv.Atoi(d.context)
		x.Efforts = d.efforts
		if len(d.efforts) > 0 {
			x.DefaultEffort = d.efforts[min(max(d.def, 0), len(d.efforts)-1)]
		} else {
			x.DefaultEffort = "default"
		}
		models[i] = x
	}
	auth := preview.AuthKey
	switch {
	case e.login():
		auth = preview.AuthOAuth
	case e.method.Access == "provider_access_endpoint":
		auth = preview.AuthNone
	}
	brand := ""
	if e.brand != nil {
		brand = e.brand.ID
	}
	if m.Live && m.ProviderActions.Save != nil {
		if e.probing {
			return ""
		}
		e.probing, e.saving = true, true
		channel := preview.Channel{ID: e.id, Revision: e.revision, Name: strings.TrimSpace(e.name), Brand: brand, Method: e.method.ID, API: e.method.API, Endpoint: e.endpoint, Auth: auth, Enabled: true, KeySet: e.keySet, Models: models, Default: e.def}
		generation := e.generation
		signIn := e.login() && e.editing == ""
		id := e.id
		m.ProviderActions.Save(ProviderDraft{Channel: channel, Key: e.key, ExpectedRevision: e.revision, OAuth: e.oauth}, func(ok bool) {
			if e.generation != generation || !e.open {
				return
			}
			e.probing, e.saving = false, false
			if ok {
				e.open = false
				e.key = ""
				if signIn {
					m.signIn(id)
				}
			}
		})
		return ""
	}
	if ch := m.channel(e.editing); ch != nil {
		ch.Name, ch.Endpoint, ch.Models, ch.Default = strings.TrimSpace(e.name), e.endpoint, models, e.def
		ch.KeySet = ch.KeySet || e.key != ""
		return ""
	}
	id := fmt.Sprintf("%s-%d", e.method.ID, len(m.Channels)+1)
	m.Channels = append(m.Channels, preview.Channel{ID: id, Name: strings.TrimSpace(e.name), Brand: brand, Method: e.method.ID,
		API: e.method.API, Endpoint: e.endpoint, Auth: auth, Enabled: true, KeySet: e.key != "", Models: models, Default: e.def})
	if e.login() {
		m.signIn(id)
	}
	return ""
}

// providerEditorDialog is 560 wide for brands, 540 for methods, 640 to
// configure; a back button and the step's title over it.
func providerEditorDialog(c *ui.Context, m *Model) {
	e := &m.provider
	if !e.open {
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
				brandPicker(c, m, nil, nil)
			case stepMethod:
				methodList(c, e)
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

// brandPicker is account logins, then the brands with API keys, as outlined
// pills. before runs ahead of a choice and after it, as the welcome guide
// opens the editor (nil for none).
func brandPicker(c *ui.Context, m *Model, before, after func()) {
	p := theme.Of(c)
	chip := func(key, label string, brand string, pick func()) {
		b := ui.ButtonBase(c.Key(key)).Label(label).Height(32).Padding(0, 14, 0, 10).Gap(8).Radius(999).
			Border(1, p.Border).Background(p.ButtonSecondary).Cursor(ui.CursorPointer).OnClick(func() {
			if m.Live && m.ProviderActions.Available != nil && !m.ProviderActions.Available() {
				kit.Toast(c, kit.Failed, i18n.T(c, "onboarding_host_unavailable"))
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
			ui.Row(c).Size(16, 16).Center().Children(func() {
				if ic := icons.Providers[brand]; ic != nil {
					// In the mark's own brand colors (Lobe Icons').
					ui.Image(c, ic).Size(16, 16)
				} else {
					ui.Text(c, string([]rune(label)[:1])).FontSize(12).FontWeight(600)
				}
			})
			ui.Text(c, label).FontSize(14).SingleLine()
		})
	}
	group := func(key string, fill func()) {
		ui.Column(c.Key(key)).Gap(8).FillWidth().Children(func() {
			ui.Text(c, i18n.T(c, key)).FontSize(12).TextColor(p.MutedForeground)
			ui.Row(c).Wrap().Gap(8).Children(fill)
		})
	}
	e := &m.provider
	ui.Column(c.Key("picker")).Gap(20).Padding(10, 12, 28, 12).Children(func() {
		if !m.Live {
			group("provider_access_login", func() {
				for i := range m.Brands {
					b := &m.Brands[i]
					for _, x := range b.Methods {
						if x.Login != "" {
							chip("login-"+x.ID, i18n.T(c, x.Login), b.ID, func() { e.brand = b; e.configure(x) })
						}
					}
				}
			})
		}
		group("provider_access_key", func() {
			for i := range m.Brands {
				b := &m.Brands[i]
				if slices.ContainsFunc(b.Methods, func(x preview.Method) bool { return x.Login == "" }) {
					chip("api-"+b.ID, b.Name, b.ID, func() {
						api := *b
						api.Methods = slices.DeleteFunc(slices.Clone(b.Methods), func(x preview.Method) bool { return x.Login != "" })
						e.choose(&api)
					})
				}
			}
		})
	})
}

// methodList is the brand over its methods, each a row with how it is
// accessed and a chevron.
func methodList(c *ui.Context, e *providerEditor) {
	if e.brand == nil {
		return
	}
	ui.Column(c.Key("methods")).Padding(15, 12, 30, 12).Children(func() {
		ui.Row(c).Gap(12).Padding(0, 0, 22, 0).Children(func() {
			ui.Row(c).Size(29, 29).Center().Children(func() { kit.Tile(c, e.brand.Name, icons.Providers[e.brand.ID]) })
			ui.Text(c, e.brand.Name).FontSize(18).FontWeight(500).SingleLine()
		})
		modalList(c, "list", "", "", "", len(e.brand.Methods), func() {
			for _, x := range e.brand.Methods {
				listRow{subtitle: i18n.T(c, x.Access), chevron: true, onClick: func() { e.configure(x) }}.row(c, x.ID, x.Name)
			}
		}, nil)
	})
}

// providerForm is the preview note, the fields the method needs, Get API key,
// the OAuth fields, and the models.
func providerForm(c *ui.Context, m *Model, e *providerEditor) {
	p := theme.Of(c)
	t := func(k string) string { return i18n.T(c, k) }
	input := func(key, label, hint string, value *string, secret bool) {
		kit.Field(c, key, label, "", func() {
			in := ui.TextInput(c.Key("input"), value).Label(label).Placeholder(hint).FillWidth().Disabled(e.probing)
			if secret {
				in.Password()
			}
		})
	}
	ui.Column(c.Key("form")).Gap(16).Children(func() {
		input("name", t("settings_name"), t("provider_name_hint"), &e.name, false)
		if e.method.API != "" {
			kit.Field(c, "api", t("provider_api"), "", func() {
				ui.Text(c, e.method.API).FontSize(14).Height(32).Padding(6, 12).FillWidth().Border(1, p.Input).Radius(p.Radius).
					TextColor(p.MutedForeground)
			})
		}
		if e.method.Custom {
			input("endpoint", t("provider_endpoint"), t("provider_endpoint_hint"), &e.endpoint, false)
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
		if e.method.OAuthFields {
			kit.Secondary(c.Key("advanced"), t("provider_oauth_advanced")).AlignSelf(ui.Start).OnClick(func() { e.advanced = !e.advanced })
			if e.advanced {
				for i, k := range []string{"provider_oauth_catalog_version", "provider_oauth_user_agent", "provider_oauth_editor_version", "provider_oauth_editor_plugin_version"} {
					input(k, t(k), "", &e.oauth[i], false)
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
				if m.Live && m.ProviderActions.Discover != nil {
					e.probing = true
					draft := ProviderDraft{Channel: preview.Channel{ID: e.id, API: e.method.API, Endpoint: e.endpoint, Method: e.method.ID}, Key: e.key, ExpectedRevision: e.revision}
					generation := e.generation
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
				// A model typed in starts with the tags a save gives it.
				e.models = append(e.models, modelDraft{open: true, model: preview.ChannelModel{Vision: true, Tools: true}})
			})
		})
		modalList(c, "models", "", "", t("provider_models_empty"), len(e.models), func() {
			for i := range e.models {
				e.models[i].pending = e.probing
				providerModel(c, e, i)
			}
		}, nil)
	})
}

// providerModel is the model's title, Default, its check to make it the
// default and its removal; open, its ID, context limit and reasoning efforts.
func providerModel(c *ui.Context, e *providerEditor, i int) {
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
		content: func() { modelForm(c, d) },
	}.row(c, [2]int{e.removed, i}, title)
}

// modelForm is the model ID, the context limit with its common sizes, and the
// reasoning efforts with the default.
func modelForm(c *ui.Context, d *modelDraft) {
	p := theme.Of(c)
	t := func(k string) string { return i18n.T(c, k) }
	ui.Column(c.Key("model-form")).Gap(12).Children(func() {
		kit.Field(c, "id", t("provider_model_id"), "", func() {
			ui.TextInput(c.Key("input"), &d.id).Label(t("provider_model_id")).Placeholder(t("form_model_hint")).FillWidth().Disabled(d.pending)
		})
		// The label with the common sizes at its right, the input under
		// them at full width.
		ui.Column(c.Key("context")).Gap(6).Children(func() {
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
		capabilityForm(c, d)
		if d.model.ID == "" || len(d.model.Efforts) > 0 {
			ui.Column(c.Key("efforts")).Gap(6).Children(func() {
				ui.Text(c, t("provider_efforts")).FontSize(14)
				for j, x := range d.efforts {
					ui.Row(c.Key(x)).Gap(6).Children(func() {
						ui.Text(c, x).FontSize(14).Grow(1)
						tip := t("provider_set_default")
						if j == d.def {
							tip = t("provider_effort_default")
						}
						b := kit.Action(c.Key("default"), icons.Check, tip, j == d.def).Disabled(d.pending)
						if j == d.def {
							b.TextColor(p.Success).Opacity(1)
						}
						b.OnClick(func() { d.def = j })
						kit.Action(c.Key("remove"), icons.Close, t("settings_delete"), false).Disabled(d.pending).OnClick(func() {
							d.efforts = slices.Delete(d.efforts, j, j+1)
							// The default stays the effort it was, as a
							// removed model keeps the default model.
							if d.def > j {
								d.def--
							}
							if d.def >= len(d.efforts) {
								d.def = 0
							}
						})
					})
				}
				ui.Row(c.Key("add")).Gap(6).Children(func() {
					ui.TextInput(c.Key("input"), &d.effort).Label(t("provider_effort_placeholder")).Placeholder(t("provider_effort_placeholder")).Grow(1).Disabled(d.pending)
					kit.Secondary(c.Key("add"), t("settings_add")).Disabled(d.pending).OnClick(func() {
						x := strings.TrimSpace(d.effort)
						if x == "" || slices.Contains(d.efforts, x) {
							kit.Toast(c, kit.Failed, t("provider_effort_invalid"))
							return
						}
						d.efforts, d.effort = append(d.efforts, x), ""
					})
				})
			})
		}
	})
}

// capabilityForm is the model's capability tags as checkboxes in three
// groups: what it understands, what it can do and what it generates. A
// change marks the tags as the user's (CustomCapabilities), which a save
// then keeps over the catalog's. Tags that are the user's can be reset: a
// save then leaves them to the catalog again, and the boxes show the
// catalog's tags where the model came with them.
func capabilityForm(c *ui.Context, d *modelDraft) {
	p := theme.Of(c)
	t := func(k string) string { return i18n.T(c, k) }
	x := &d.model
	check := func(group, key, name string, on *bool) bool {
		changed := ui.Checkbox(c.Key(key), on, t(name)).Label(t(group) + " " + t(name)).Disabled(d.pending).Changed()
		x.CustomCapabilities = x.CustomCapabilities || changed
		return changed
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
	group := func(key string, boxes func()) {
		ui.Row(c.Key(key)).Gap(8).AlignItems(ui.Center).Children(func() {
			ui.Text(c, t(key)).FontSize(12).TextColor(p.MutedForeground).SingleLine()
			boxes()
		})
	}
	ui.Column(c.Key("capabilities")).Gap(6).Children(func() {
		ui.Row(c.Key("heading")).Gap(6).AlignItems(ui.Center).Children(func() {
			ui.Text(c, t("provider_capabilities")).FontSize(14).FontWeight(500).Grow(1).Shrink(1).MinWidth(0).SingleLine().TextColor(p.PopoverForeground)
			if x.CustomCapabilities {
				b := kit.Ghost(c.Key("reset"), t("provider_cap_reset"), false).Height(24).PaddingX(8).Border(1, p.Border).Shrink(0).Disabled(d.pending)
				b.Children(func() { ui.Text(c, t("provider_cap_reset")).FontSize(12) }).OnClick(func() {
					x.CustomCapabilities = false
					if d.catalog != nil {
						x.Vision, x.Tools, x.Reasoning, x.Generates = d.catalog.Vision, d.catalog.Tools, d.catalog.Reasoning, slices.Clone(d.catalog.Generates)
					}
				})
			}
		})
		// The form's surface, not the window's, under the unchecked boxes.
		surface := *c.Theme()
		surface.Background = ui.Transparent
		ui.Themed(c, &surface).Background(ui.Transparent).Children(func() {
			ui.Row(c).Wrap().Gap(20).Children(func() {
				group("provider_cap_understands", func() { check("provider_cap_understands", "vision", "media_image", &x.Vision) })
				group("provider_cap_can", func() {
					check("provider_cap_can", "tools", "provider_cap_tools", &x.Tools)
					check("provider_cap_can", "reasoning", "provider_cap_reasoning", &x.Reasoning)
				})
				group("provider_cap_generates", func() {
					// One Image box for both ways of generating: the hosted
					// tool the model came with is kept, else the endpoint.
					image := slices.Contains(x.Generates, viewmodel.GenerateImage) || slices.Contains(x.Generates, viewmodel.GenerateImageTool)
					generates("image", "media_image", image, func(on bool) {
						toggle(viewmodel.GenerateImage)(false)
						toggle(viewmodel.GenerateImageTool)(false)
						switch {
						case on && d.hosted:
							toggle(viewmodel.GenerateImageTool)(true)
						case on:
							toggle(viewmodel.GenerateImage)(true)
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
	brandPicker(c, s.Model, func() { s.provider.add(s.Model) }, func() {
		s.provider.returnTo = back
		chosen()
	})
}
