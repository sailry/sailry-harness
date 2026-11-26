package desktop

import (
	"maps"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// connectorEditor is the add dialog's query, the type chosen, its method tab,
// the fields typed and the CLI account.
type connectorEditor struct {
	open     bool
	query    string
	kind     *preview.ConnectorType
	direct   bool   // opened on its connector: no list to go back to
	service  string // the service edited, "" for a new one
	revision uint64
	method   int
	fields   map[string]string
	account  int
	pending  bool
	// reacquire is an existing OAuth or CLI connection asked to authorize or
	// bind again: without it the editor saves the connection's fields.
	reacquire bool
	// request is the save under way. The editor that replaced this one has
	// another or none, so a late completion does not close it.
	request *int
	// accounts is the host's look at its local CLI accounts: the account
	// row's skeleton shows while it runs.
	accounts   preview.Fetch
	cliStarted bool
}

// addCredentials opens the editor on the service's connector for a new
// account: its methods and fields, as Add after choosing the connector.
func (m *page) addCredentials(s *preview.Service) {
	if k := m.connType(s.Type); k != nil {
		m.connector = connectorEditor{open: true, kind: k, fields: map[string]string{}, direct: true, method: defaultMethod(k)}
	}
}

// defaultMethod is the method a new connection starts on: the local CLI when
// the connector offers it, which binds an account already signed in.
func defaultMethod(k *preview.ConnectorType) int {
	return max(0, slices.Index(k.Methods, "connector_cli"))
}

func (m *page) editService(s *preview.Service) {
	for i := range m.ConnTypes {
		if m.ConnTypes[i].ID == s.Type {
			k := &m.ConnTypes[i]
			fields := maps.Clone(s.Fields)
			if fields == nil {
				fields = map[string]string{}
			}
			m.connector = connectorEditor{open: true, kind: k, service: s.ID, fields: fields, revision: s.Revision}
			m.connector.method = max(0, slices.Index(k.Methods, s.Method))
			return
		}
	}
}

// authenticate opens the connector that authorizes the service's servers:
// its account when it has one, else the connector's editor for a new one.
func (m *page) authenticate(s *preview.Service) {
	if account := m.service(s.Type); account != nil && account.Account {
		m.editService(account)
		return
	}
	m.addCredentials(s)
}

func connectorAuthorizationDialog(c *ui.Context, m *page) {
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	if m.Live && m.ConnectorActions != nil {
		actions := m.ConnectorActions
		if consent := actions.Consent; consent != nil && consent.Open {
			lines := []string{t("connector_consent_effect")}
			for _, change := range consent.Changes {
				lines = append(lines, t("connector_changed_"+change))
			}
			if kit.AlertDialog(c.Key("connector-consent"), &consent.Open, t("connector_confirm")+" "+consent.Target, strings.Join(lines, "\n"), t("settings_cancel"), t("connector_confirm")) == 1 {
				consent.Apply(true)
			}
		}
		settings.AuthorizationDialog(c, "connector-login", &actions.Authorization, func() {
			serviceTile(c, actions.Authorization.Title, "")
		}, actions.Retry, func() {
			if actions.Submit != nil {
				actions.Submit(actions.Authorization.Pasted)
			}
		})
	}
}

// connectorDialog is the connector list to choose from, then the connector's
// header, its methods as segments, new or saved, the chosen method's account
// list or fields, and Cancel with the method's action.
func connectorDialog(c *ui.Context, m *page) {
	e := &m.connector
	if !e.open {
		return
	}
	p := theme.Of(c)
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	kit.Dialog(c.Key("connector-editor"), &e.open, func() {
		if e.kind == nil {
			kit.DialogHeader(c, t("connector_add"))
			ui.SearchField(c.Key("query"), &e.query).Placeholder(t("connector_search")).Label(t("connector_search")).FillWidth()
			q := strings.ToLower(strings.TrimSpace(e.query))
			var kinds []*preview.ConnectorType
			for i := range m.ConnTypes {
				k := &m.ConnTypes[i]
				if q == "" || strings.Contains(strings.ToLower(k.Name+" "+k.Description), q) {
					kinds = append(kinds, k)
				}
			}
			ui.Scroll(c.Key("types")).MaxHeight(420).Children(func() {
				settings.ModalList(c, "types", "", "", t("connector_empty"), len(kinds), func() {
					for _, k := range kinds {
						added := m.service(k.ID) != nil
						meta := ""
						if added {
							meta = t("connector_added")
						}
						settings.ListRow{Subtitle: k.Description, Meta: meta, Chevron: true, Disabled: !k.Enabled && !added,
							Leading: func() { ui.Row(c).Size(32, 32).Center().Children(func() { serviceTile(c, k.Name, k.Icon) }) },
							OnClick: func() {
								if s := m.service(k.ID); s != nil {
									m.editService(s)
									return
								}
								e.kind, e.method = k, defaultMethod(k)
							}}.Render(c, k.ID, k.Name)
					}
				}, nil)
			})
			return
		}
		k := e.kind
		existing := m.service(e.service)
		kit.DialogHeaderRow(c, func() {
			if e.service == "" && !e.direct {
				kit.Action(c.Key("back"), icons.ArrowLeft, t("provider_step_back"), e.pending).OnClick(func() { e.kind = nil })
			}
			ui.Row(c).Size(32, 32).Center().Children(func() { serviceTile(c, k.Name, k.Icon) })
			ui.Column(c).MinWidth(0).Gap(2).Children(func() {
				ui.Text(c, k.Name).FontSize(16).FontWeight(600).TextColor(p.PopoverForeground)
				if existing != nil && existing.Identity != "" {
					ui.Text(c, existing.Identity).FontSize(14).TextColor(p.MutedForeground)
				}
			})
		})
		// A saved connection shows the same methods, on the one it uses.
		if len(k.Methods) > 1 {
			kit.Segmented(c, "methods", &e.method, settings.Labels(c, k.Methods), kit.SegmentedProps{Block: true}).Label(t("connector_filter")).Disabled(e.pending)
		}
		method := k.Methods[min(e.method, len(k.Methods)-1)]
		// The connection's own method keeps what it holds; another method
		// authorizes the connection again with that method.
		same := existing != nil && existing.Method == method
		var fields []preview.ConnectorField
		for _, f := range k.Fields {
			if len(f.Methods) == 0 || slices.Contains(f.Methods, method) {
				fields = append(fields, f)
			}
		}
		draft := func() settings.ConnectorDraft {
			input := settings.ConnectorDraft{Type: k.ID, Service: e.service, Method: method, ExpectedRevision: e.revision, Fields: make(map[string]string, len(fields))}
			for _, field := range fields {
				input.Fields[field.Name] = e.fields[field.Name]
			}
			return input
		}
		// A saved CLI connection, or a connected OAuth one, with fields saves
		// them as they are; binding or authorizing again is asked for apart.
		acquired := method == "connector_oauth" || method == "connector_cli"
		configuring := same && acquired && len(fields) > 0 && !e.reacquire && (method == "connector_cli" || existing.State == preview.Connected)
		ready := true
		ui.Column(c.Key("content")).Gap(12).Children(func() {
			if method == "connector_cli" && !configuring {
				ready = cliAccounts(c, m, e, k, existing)
			} else if method == "connector_oauth" && len(fields) == 0 {
				ui.Column(c.Key("browser")).AlignItems(ui.Center).Gap(12).Padding(12).Children(func() {
					ui.Icon(c, icons.Globe).FontSize(32).TextColor(p.MutedForeground)
					ui.Text(c, t("connector_browser")).FontSize(14)
				})
			}
			for _, f := range fields {
				v := e.fields[f.Name]
				kit.Field(c, f.Name, f.Title, "", func() {
					ui.Row(c).Gap(4).Children(func() {
						in := ui.TextInput(c.Key("input"), &v).Label(f.Title).Grow(1).Disabled(e.pending)
						if f.Secret {
							in.Password().Placeholder(t("form_secret_hint"))
						}
						if f.Help {
							kit.Action(c.Key("help"), icons.ExternalLink, t("menu_help"), false).OnClick(func() {
								if m.Live && m.ConnectorActions != nil && m.ConnectorActions.Help != nil {
									if url := m.ConnectorActions.Help(k.ID, f.Name); url != "" {
										c.OpenURL(url)
									}
								} else {
									settings.PreviewNotice(c)
								}
							})
						}
					})
				})
				e.fields[f.Name] = v
				// A field the method needs has a value, or for the method's
				// saved secret the one the Node holds. Authorizing in the
				// browser or binding the CLI asks for its own.
				if (configuring || !acquired) && !f.Optional && !(m.Live && f.Secret && same) && strings.TrimSpace(v) == "" {
					ready = false
				}
			}
			if configuring {
				again := "connector_reauthorize"
				if method == "connector_cli" {
					again = "connector_bind"
				}
				kit.TextAction(c.Key("reacquire"), t(again), e.pending || !k.Enabled).AlignSelf(ui.Start).OnClick(func() {
					e.reacquire, e.cliStarted, e.accounts = true, false, preview.Fetch{}
				})
			}
			if method == "connector_application" {
				kit.Secondary(c.Key("register"), t("connector_register")).Height(28).AlignSelf(ui.Start).Disabled(e.pending).OnClick(func() {
					if e.pending {
						return
					}
					if m.Live && m.ConnectorActions != nil && m.ConnectorActions.Save != nil {
						input := draft()
						input.Registration = true
						m.ConnectorActions.Save(input, e.saving())
					} else {
						settings.PreviewNotice(c)
					}
				})
			}
		})
		action := "settings_save"
		switch {
		case configuring:
		case method == "connector_oauth" && same:
			action = "connector_reauthorize"
		case method == "connector_oauth":
			action = "connector_authorize"
		case method == "connector_cli":
			action = "connector_bind"
		}
		kit.DialogFooter(c, func() {
			kit.Secondary(c.Key("cancel"), t("authorization_cancel")).OnClick(func() { e.open = false })
			pendingButton(c, "ok", t(action), !ready, e.pending).OnClick(func() {
				if !ready || e.pending {
					return
				}
				if m.Live && m.ConnectorActions != nil && m.ConnectorActions.Save != nil {
					input := draft()
					input.Update = configuring
					if method == "connector_cli" && !configuring {
						if e.account < 0 || e.account >= len(m.ConnAccounts) {
							return
						}
						input.CLIAccount = m.ConnAccounts[e.account]
					}
					m.ConnectorActions.Save(input, e.saving())
					return
				}
				if s := m.service(e.service); s != nil {
					s.Method = method
				} else {
					st := preview.Connected
					if method == "connector_oauth" {
						st = preview.Connecting
					}
					m.Services = append(m.Services, preview.Service{ID: k.ID, Type: k.ID, Name: k.Name, Description: k.Description,
						Method: method, Account: true, State: st, AccountOn: true})
				}
				if method == "connector_oauth" && !configuring {
					settings.PreviewNotice(c)
				} else {
					kit.Toast(c, kit.Done, t("connector_saved"))
				}
				e.open = false
			})
		})
	}).Width(settings.DialogWidth(c, 520)).Label(t("connector_add"))
}

// cliAccounts is the local CLI's accounts to bind, the connection's own
// selected, once the host looked at the CLI; with none, Retry looks again.
// It reports whether there is an account to bind.
func cliAccounts(c *ui.Context, m *page, e *connectorEditor, k *preview.ConnectorType, existing *preview.Service) bool {
	t := func(key string, a ...any) string { return i18n.T(c, key, a...) }
	identity := ""
	if existing != nil {
		identity = existing.Identity
	}
	if m.Live {
		if !e.cliStarted && m.ConnectorActions != nil && m.ConnectorActions.InspectCLI != nil {
			e.cliStarted = true
			e.accounts.Loading = true
			m.ConnAccounts = nil
			typeID := k.ID
			m.ConnectorActions.InspectCLI(typeID, func(accounts []string) {
				if e.open && e.kind != nil && e.kind.ID == typeID {
					m.ConnAccounts = accounts
					e.accounts.Loading, e.accounts.Loaded = false, true
					e.account = max(0, slices.Index(accounts, identity))
				}
			})
		}
		if e.accounts.Loading {
			accountSkeleton(c)
			return false
		}
	} else {
		if !e.accounts.Loaded && !e.accounts.Loading {
			e.accounts.Start()
		}
		if e.accounts.Advance(c.Now()) {
			c.After(preview.Tick)
			accountSkeleton(c)
			return false
		}
	}
	names := m.ConnAccounts
	settings.ModalList(c, "accounts", t("connector_choose_account"), "", t("connector_not_connected"), len(names), func() {
		for i, a := range names {
			sel := i == e.account
			settings.ListRow{Selected: &sel, OnClick: func() { e.account = i }}.Render(c, i, a)
		}
	}, nil)
	if len(names) == 0 {
		kit.TextAction(c.Key("retry"), t("chat_retry"), e.pending).AlignSelf(ui.End).OnClick(func() {
			e.cliStarted, e.accounts = false, preview.Fetch{}
		})
	}
	return len(names) > 0
}

// pendingButton is the primary button of an action that takes time: it
// cannot be pressed while it cannot act or while the action is under way,
// which shows as a spinner before its label.
func pendingButton(c *ui.Context, key any, label string, disabled, pending bool) ui.Element {
	return kit.Primary(c.Key(key), label, disabled || pending).Busy(pending).Children(func() {
		if pending {
			kit.Busy(c)
		}
		ui.Text(c, label).FontSize(14)
	})
}

// saving marks a save under way and returns its completion, which closes
// the editor that asked when the save went through.
func (e *connectorEditor) saving() func(bool) {
	request := new(int)
	e.request, e.pending = request, true
	return func(ok bool) {
		if e.request != request {
			return
		}
		e.request, e.pending = nil, false
		if ok {
			e.open = false
		} else if e.cliStarted {
			e.cliStarted, e.accounts = false, preview.Fetch{}
		}
	}
}

// accountSkeleton holds the account row's place while the Node inspects the
// local CLI: a 24 DIP round avatar beside a name and a secondary line.
func accountSkeleton(c *ui.Context) {
	ui.Column(c.Key("account-skeleton")).FillWidth().Busy(true).Children(func() {
		settings.ModalList(c, "accounts", i18n.T(c, "connector_choose_account"), "", "", 1, func() {
			settings.ListRow{Leading: func() {
				ui.Row(c).Gap(12).Children(func() {
					kit.SkeletonAvatar(c, 24, true, kit.SkeletonFade)
					ui.Column(c).Gap(4).Children(func() {
						kit.Skeleton(c, kit.SkeletonProps{Width: 120, Height: 12, Radius: 2})
						kit.Skeleton(c, kit.SkeletonProps{Width: 80, Height: 10, Radius: 2, Secondary: true})
					})
				})
			}}.Render(c, "account-loading", "")
		}, nil)
	})
}
