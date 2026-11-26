package desktop

import (
	"maps"
	"slices"
	"strconv"
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

// providerEditorDialog is 560 wide for brands, 540 for methods, 640 to
// configure; a back button and the step's title over it.
func providerEditorDialog(c *ui.Context, m *Controller) {
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
		kit.DialogHeaderRow(c, func() {
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
			settings.DialogFooter(c, action, func() { e.open = false }, func() {
				if problem := e.save(m); problem != "" {
					kit.Toast(c, kit.Failed, t(problem))
					return
				}
				if !m.Live {
					e.open = false
				}
			}, e.saving)
		}
	}).Width(settings.DialogWidth(c, w)).Label(t(title))
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
func brandPicker(c *ui.Context, m *Controller, node string, before, after func()) {
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
func methodList(c *ui.Context, m *Controller) {
	e := &m.provider
	if e.brand == nil {
		return
	}
	ui.Column(c.Key("methods")).Padding(15, 12, 30, 12).Children(func() {
		ui.Row(c).Gap(12).Padding(0, 0, 22, 0).Children(func() {
			ui.Row(c).Size(29, 29).Center().Children(func() { kit.Tile(c, e.brand.Name, icons.Brand(e.brand.ID)) })
			ui.Text(c, e.brand.Name).FontSize(18).FontWeight(500).SingleLine()
		})
		settings.ModalList(c, "list", "", "", "", len(e.methods), func() {
			for _, x := range e.methods {
				settings.ListRow{Subtitle: i18n.T(c, x.Access), Chevron: true, OnClick: func() { m.configureProvider(x) }}.Render(c, channelKey(x.Node, x.ID), x.Name)
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
func providerForm(c *ui.Context, m *Controller, e *providerEditor) {
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
		if e.current().Custom && !e.bedrock() && !e.vertex() {
			input("endpoint", t("provider_endpoint"), t("provider_endpoint_hint"), &e.endpoint, false)
		}
		routeField(c, e, "product", t("provider_product"), axisProduct)
		routeField(c, e, "region", t("provider_region"), axisRegion)
		if e.azure() {
			input("api-version", t("provider_api_version"), t("form_api_version_hint"), &e.apiVersion, false)
		}
		if e.bedrock() {
			input("cloud-region", t("provider_region"), bedrockDefaultRegion, &e.region, false)
		}
		if e.vertex() {
			input("cloud-project", t("provider_project"), "", &e.project, false)
			input("cloud-location", t("provider_location"), vertexDefaultLocation, &e.location, false)
		}
		if e.usesKey() {
			label, hint := t("provider_api_key"), t("provider_key_hint")
			if e.vertex() {
				// The service account's JSON key is the channel's credential.
				label, hint = t("provider_service_account"), t("provider_service_account_hint")
			}
			if e.keySet {
				hint = t("backup_keep_secret")
			}
			input("key", label, hint, &e.key, true)
		}
		if e.method.KeyURL != "" {
			b := ui.ButtonBase(c.Key("key-link")).Label(t("provider_get_key")).Cursor(ui.CursorPointer).AlignSelf(ui.Start).OnClick(func() {
				if m.Live {
					c.OpenURL(e.method.KeyURL)
				} else {
					settings.PreviewNotice(c)
				}
			})
			b.Children(func() { ui.Text(c, t("provider_get_key")).FontSize(12).TextColor(p.Link) })
		}
		if e.billsByRate() {
			label := t("provider_billing_rate")
			kit.Field(c, "billing-rate", label, t("provider_billing_rate_hint"), func() { billingRateInput(c, e, label) })
		}
		if e.method.OAuth != ([4]string{}) || !e.login() {
			// A disclosure: the chevron after the label turns down while the settings show.
			label, chevron := t("provider_oauth_advanced"), icons.ChevronRight
			if e.advanced {
				chevron = icons.ChevronDown
			}
			kit.GhostLook(c, ui.ButtonBase(c.Key("advanced")).Label(label).Expanded(e.advanced), false).Height(28).PaddingX(4).Gap(4).AlignSelf(ui.Start).
				Children(func() {
					ui.Text(c, label).FontSize(14).SingleLine()
					ui.Icon(c, chevron).FontSize(14).TextColor(theme.Of(c).MutedForeground)
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
					draft := settings.ProviderDraft{Channel: preview.Channel{Node: e.owner, Group: e.group, Targets: slices.Clone(e.targets), TargetIDs: maps.Clone(e.targetIDs), TargetRevisions: maps.Clone(e.targetRevisions), ID: e.id, API: e.current().API, Endpoint: e.channelEndpoint(), Method: e.current().ID, APIVersion: strings.TrimSpace(e.apiVersion), Region: strings.TrimSpace(e.region), Project: strings.TrimSpace(e.project), Location: strings.TrimSpace(e.location)}, Key: e.keyUpdate(), OAuth: e.oauth, ExpectedRevision: e.revision, EditorGeneration: generation, Group: e.group, Targets: slices.Clone(e.targets), TargetRevisions: maps.Clone(e.targetRevisions)}
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
		settings.ModalList(c, "models", "", "", t("provider_models_empty"), len(e.models), func() {
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
func providerTargets(c *ui.Context, m *Controller, e *providerEditor) {
	hosts := e.targetHosts(m)
	if providerAuthAllowsMultipleTargets(e.auth()) && slices.ContainsFunc(hosts, func(host settings.ProviderHost) bool {
		return host.Node != e.owner && (host.Connected || slices.Contains(e.targets, host.Node))
	}) {
		providerHostSelect(c, hosts, e)
	}
}

func providerHostSelect(c *ui.Context, hosts []settings.ProviderHost, e *providerEditor) {
	label := i18n.T(c, "provider_hosts")
	names := make([]string, 0, len(e.targets)+1)
	for _, host := range hosts {
		if host.Node == e.owner || slices.Contains(e.targets, host.Node) {
			names = append(names, settings.ProviderHostName(c, host))
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
				item := menu.Item(settings.ProviderHostName(c, host)).Icon(icons.Platform(host.Platform)).Checked(selected).
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

// providerModel is the model's title, Default, its check to make it the
// default and its removal; open, its ID, context limit and reasoning efforts.
func providerModel(c *ui.Context, m *Controller, e *providerEditor, i int) {
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
	settings.ListRow{Meta: meta, Expanded: &d.open, OnClick: func() { d.open = !d.open },
		Trailing: func() {
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
		Remove: func() {
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
		}, RemoveLabel: t("settings_delete"),
		Content: func() {
			e.lookupModelCapabilities(c, m, d)
			modelForm(c, d, e.current().API)
		},
	}.Render(c, [2]int{e.removed, i}, title)
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
