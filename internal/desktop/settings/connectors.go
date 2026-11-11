package settings

import (
	"fmt"
	"maps"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The Connectors page and the MCP editor (settings/mcp*): a toolbar of
// search, method filter, Add and Add MCP; each service a row with its status,
// its switch and menu; the add dialog (choose a connector, then its method
// and fields), the details dialog, and the MCP server editor.

// connectorMethods are the filter's choices, "" for all.
var connectorMethods = []string{"", "connector_oauth", "connector_application", "connector_token", "connector_cli"}

// ConnStatus is a service's status: the i18n key of a problem, or "" with the
// status kind.
type ConnStatus struct {
	Problem    string
	Connecting bool
	Failed     bool
	Tools      int // connected: its tools; -1 not connected
}

// StatusOf derives a preview service's connection state.
func StatusOf(s *preview.Service) ConnStatus {
	if s.Account {
		if !s.AccountOn {
			return ConnStatus{Tools: -1}
		}
		switch s.State {
		case preview.Unconnected:
			return ConnStatus{Problem: "connector_not_connected", Tools: -1}
		case preview.Expired:
			return ConnStatus{Problem: "connector_expired", Tools: -1}
		case preview.NeedsConfirmation:
			return ConnStatus{Problem: "connector_confirmation", Tools: -1}
		case preview.Connecting:
			return ConnStatus{Connecting: true, Tools: -1}
		case preview.ConnFailed:
			return ConnStatus{Problem: "connector_failed", Tools: -1}
		}
	}
	tools, up := 0, len(s.Servers) > 0
	for _, x := range s.Servers {
		if !x.Allowed {
			return ConnStatus{Problem: "connector_server_disabled", Tools: -1}
		}
		switch x.State {
		case preview.ServerAuth:
			return ConnStatus{Problem: "connector_not_connected", Tools: -1}
		case preview.ServerDown:
			return ConnStatus{Failed: true, Tools: -1}
		case preview.ServerStarting:
			return ConnStatus{Connecting: true, Tools: -1}
		}
		tools += x.Tools
	}
	if !up && !s.Account {
		return ConnStatus{Problem: "connector_not_connected", Tools: -1}
	}
	if !up {
		return ConnStatus{Tools: -1}
	}
	return ConnStatus{Tools: tools}
}

// connectorEditor is the add dialog's query, the type chosen, its method tab,
// the fields typed and the CLI account.
type connectorEditor struct {
	open     bool
	query    string
	kind     *preview.ConnectorType
	service  string // the service edited, "" for a new one
	revision uint64
	method   int
	fields   map[string]string
	account  int
	pending  bool
	// request is the save under way. The editor that replaced this one has
	// another or none, so a late completion does not close it.
	request *int
	// accounts is the host's look at its local CLI accounts: the account
	// row's skeleton shows while it runs.
	accounts   preview.Fetch
	cliStarted bool
}

// mcpEditor is a server's name, transport, command and arguments or URL, and
// its environment or headers.
type mcpEditor struct {
	open      bool
	original  string // the service edited, "" for a new one
	name      string
	transport int
	command   string
	args      []string
	url       string
	slots     []preview.EnvVar
	saved     []bool // the slot has a saved value
	revision  uint64
	// request is the save under way: Save waits for it, and the editor that
	// replaced this one has another or none, so a late completion does not
	// close it.
	request *int
	// removed counts the rows taken out of args and slots. It is part of a
	// row's identity: a row that moves into a removed one's place does not
	// take its text state (undo history, selection) with the place.
	removed int
}

func connectors(c *ui.Context, m *Model) {
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	ui.Column(c.Key("connectors")).Gap(16).Children(func() {
		ui.Row(c.Key("toolbar")).Gap(12).Children(func() {
			ui.SearchField(c.Key("search"), &m.ConnQuery).Placeholder(t("connector_search")).Label(t("connector_search")).Grow(1)
			labels := []string{t("connector_all_methods")}
			for _, k := range connectorMethods[1:] {
				labels = append(labels, t(k))
			}
			pick(c, "method", t("connector_filter"), &m.ConnMethod, labels, false).Width(192)
			iconButton(c, "add", nil, t("settings_add"), true, false).OnClick(func() {
				m.connector = connectorEditor{open: true, fields: map[string]string{}}
			})
			kit.Secondary(c.Key("add-mcp"), t("mcp_add")).OnClick(func() {
				m.mcp = mcpEditor{open: true, transport: 0}
			})
		})
		if !m.Live {
			kit.Tag(c, t("connector_preview")).AlignSelf(ui.Start)
		}
		q := strings.ToLower(strings.TrimSpace(m.ConnQuery))
		var shown []int
		for i := range m.Services {
			s := &m.Services[i]
			if q != "" && !strings.Contains(strings.ToLower(s.Name+" "+s.Description), q) {
				continue
			}
			if f := connectorMethods[m.ConnMethod]; f != "" && s.Method != f {
				continue
			}
			shown = append(shown, i)
		}
		if len(shown) == 0 {
			kit.Result(c, kit.ResultProps{Icon: icons.Network, Title: t("connector_empty"), Card: true})
			return
		}
		kit.Card(c, func() {
			for _, i := range shown {
				serviceRow(c, m, &m.Services[i])
			}
		})
	})
	connectorDialog(c, m)
	mcpDialog(c, m)
	serviceDetails(c, m)
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
		authorization(c, "connector-login", &actions.Authorization, func() {
			serviceTile(c, actions.Authorization.Title)
		}, actions.Retry, func() {
			if actions.Submit != nil {
				actions.Submit(actions.Authorization.Pasted)
			}
		})
	}
	if s := m.service(m.connRemoving); s != nil {
		open := true
		if confirm(c, "remove", &open, t("connector_remove")+" "+s.Name, t("connector_remove_effect", "name", s.Name), t("connector_remove")) {
			if m.Live && m.ConnectorActions != nil && m.ConnectorActions.Remove != nil {
				m.ConnectorActions.Remove(s.ID)
			} else {
				m.Services = slices.DeleteFunc(m.Services, func(x preview.Service) bool { return x.ID == s.ID })
				kit.Toast(c, kit.Done, t("connector_removed"))
			}
		}
		if !open {
			m.connRemoving = ""
		}
	}
}

func (m *Model) service(id string) *preview.Service {
	for i := range m.Services {
		if id != "" && m.Services[i].ID == id {
			return &m.Services[i]
		}
	}
	return nil
}

func serviceTile(c *ui.Context, name string) {
	kit.Tile(c, name, icons.Plug)
}

// statusView is a problem as muted text, a spinner while connecting, a dot
// with the tools or the failure.
func statusView(c *ui.Context, st ConnStatus) {
	p := theme.Of(c)
	ui.Row(c.Key("status")).Shrink(0).Gap(8).Children(func() {
		muted := func(s string) { ui.Text(c, s).FontSize(14).TextColor(p.MutedForeground).SingleLine() }
		switch {
		case st.Problem != "":
			muted(i18n.T(c, st.Problem))
		case st.Connecting:
			ui.Spinner(c).Size(14, 14)
			muted(i18n.T(c, "connector_mcp_connecting"))
		case st.Failed:
			kit.StatusDot(c, kit.Negative, i18n.T(c, "connector_mcp_failed")).Size(6, 6)
			muted(i18n.T(c, "connector_mcp_failed"))
		case st.Tools >= 0:
			kit.StatusDot(c, kit.Positive, i18n.T(c, "connector_connected")).Size(6, 6)
			muted(i18n.T(c, "connector_mcp_tools", "n", st.Tools))
		}
	})
}

// serviceRow shows a service and its controls.
func serviceRow(c *ui.Context, m *Model, s *preview.Service) {
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	desc := s.Description
	if s.Identity != "" {
		desc = s.Identity
	}
	st := StatusOf(s)
	kit.Entry(c, s.ID, func() {
		kit.MediaRow(c, s.Name, desc, func() { serviceTile(c, s.Name) }, nil)
	}, func() {
		ui.Row(c).Gap(12).Children(func() {
			// Connect is in the menu (serviceItems), not a button here.
			statusView(c, st)
			switch {
			case s.Account:
				if ui.Switch(c.Key("account"), &s.AccountOn).Label(t("connector_account_enabled")+" "+s.Name).Disabled(s.State == preview.Connecting).Changed() && m.Live && m.ConnectorActions != nil && m.ConnectorActions.Enable != nil {
					m.ConnectorActions.Enable(s.ID, s.AccountOn)
				}
			case len(s.Servers) == 1:
				if ui.Switch(c.Key("allowed"), &s.Servers[0].Allowed).Label(t("connector_server_allowed") + " " + s.Name).Changed() {
					if m.Live && m.ConnectorActions != nil && m.ConnectorActions.AllowMCP != nil {
						m.ConnectorActions.AllowMCP(s.ID, s.Servers[0].Name, s.Servers[0].Allowed)
					} else {
						kit.Toast(c, kit.Done, t("connector_saved"))
					}
				}
			}
			kit.MenuButton(c.Key("menu"), "connector-menu-"+s.ID, func(b ui.Element) {
				kit.IconLook(c, b, icons.Ellipsis, t("more"), "", 28, false)
			}, func(menu *Menu) {
				for _, k := range serviceItems(s) {
					it := menu.Item(t(k))
					if k == "connector_remove" {
						it.Danger()
					}
					if it.Chosen() {
						switch k {
						case "connector_remove":
							m.connRemoving = s.ID
						case "connector_details":
							m.connDetails = s.ID
						case "connector_confirm":
							if m.Live && m.ConnectorActions != nil && m.ConnectorActions.Confirm != nil {
								m.ConnectorActions.Confirm(s.ID)
							} else {
								m.editService(s)
							}
						case "connector_probe":
							if m.Live && m.ConnectorActions != nil && m.ConnectorActions.Probe != nil {
								m.ConnectorActions.Probe(s.ID)
							} else {
								notice(c)
							}
						case "connector_mcp_reconnect":
							if m.Live && m.ConnectorActions != nil && m.ConnectorActions.Reconnect != nil {
								m.ConnectorActions.Reconnect(s.ID)
							} else {
								notice(c)
							}
						case "connector_add_credentials":
							if m.Live && m.ConnectorActions != nil && m.ConnectorActions.ConfigureMCP != nil {
								m.ConnectorActions.ConfigureMCP(s.ID)
							} else {
								m.editService(s)
							}
						case "connector_configure", "connector_bind", "connector_reauthorize", "connector_connect":
							if s.Manual && k != "connector_connect" {
								m.editServer(s)
							} else {
								m.editService(s)
							}
						default:
							notice(c)
						}
					}
				}
			}, kit.MenuOptions{At: ui.AnchorBottomEnd, Self: ui.AnchorTopEnd})
		})
	}, nil)
}

// serviceItems is configure and delete for servers added by hand; the
// account's edit, check and delete; credentials; then reconnect and details
// for services with servers. They are i18n keys, in order.
func serviceItems(s *preview.Service) []string {
	var out []string
	switch {
	case s.Manual:
		out = []string{"connector_configure", "connector_remove"}
		if slices.ContainsFunc(s.Servers, func(server preview.Server) bool { return server.State == preview.ServerAuth }) {
			out = append(out, "connector_connect")
		}
	case s.Account:
		label := "connector_configure"
		switch {
		case s.State == preview.NeedsConfirmation:
			label = "connector_confirm"
		case s.Method == "connector_cli":
			label = "connector_bind"
		case s.Method == "connector_oauth" && s.State == preview.Unconnected:
			label = "connector_connect"
		case s.Method == "connector_oauth":
			label = "connector_reauthorize"
		}
		out = append(out, label)
		if s.AccountOn && len(s.Servers) == 0 && (s.State == preview.Connected || s.State == preview.ConnFailed || s.State == preview.Expired) {
			out = append(out, "connector_probe")
		}
		out = append(out, "connector_remove")
	default:
		key := "connector_add_credentials"
		if slices.ContainsFunc(s.Servers, func(x preview.Server) bool { return x.State == preview.ServerAuth }) {
			key = "connector_connect"
		}
		out = append(out, key)
	}
	if slices.ContainsFunc(s.Servers, func(x preview.Server) bool { return x.Allowed && x.State != preview.ServerAuth }) {
		out = append(out, "connector_mcp_reconnect")
	}
	if len(s.Servers) > 0 {
		out = append(out, "connector_details")
	}
	return out
}

func (m *Model) editService(s *preview.Service) {
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

func (m *Model) editServer(s *preview.Service) {
	x := s.Servers[0]
	m.mcp = mcpEditor{open: true, original: s.ID, revision: s.Revision, name: strings.TrimPrefix(x.Name, "mcp-"), transport: x.Transport,
		command: x.Command, args: slices.Clone(x.Args), url: x.URL, slots: slices.Clone(x.Env)}
	m.mcp.saved = make([]bool, len(x.Env))
	for i := range m.mcp.saved {
		m.mcp.saved[i] = true
	}
}

// connectorDialog is the connector list to choose from, then the connector's
// header, its methods as segments, its form and Cancel with the method's
// action.
func connectorDialog(c *ui.Context, m *Model) {
	e := &m.connector
	if !e.open {
		return
	}
	p := theme.Of(c)
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	kit.Dialog(c.Key("connector-editor"), &e.open, func() {
		if e.kind == nil {
			ui.Text(c, t("connector_add")).FontSize(18).FontWeight(600)
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
				modalList(c, "types", "", "", t("connector_empty"), len(kinds), func() {
					for _, k := range kinds {
						added := m.service(k.ID) != nil
						meta := ""
						if added {
							meta = t("connector_added")
						}
						listRow{subtitle: k.Description, meta: meta, chevron: true, disabled: !k.Enabled && !added,
							leading: func() { ui.Row(c).Size(32, 32).Center().Children(func() { serviceTile(c, k.Name) }) },
							onClick: func() {
								if s := m.service(k.ID); s != nil {
									m.editService(s)
									return
								}
								e.kind = k
							}}.row(c, k.ID, k.Name)
					}
				}, nil)
			})
			return
		}
		k := e.kind
		ui.Row(c.Key("header")).Gap(12).Children(func() {
			if e.service == "" {
				kit.Action(c.Key("back"), icons.ArrowLeft, t("provider_step_back"), e.pending).OnClick(func() { e.kind = nil })
			}
			ui.Row(c).Size(32, 32).Center().Children(func() { serviceTile(c, k.Name) })
			ui.Text(c, k.Name).FontSize(18).FontWeight(600)
		})
		if len(k.Methods) > 1 && e.service == "" {
			kit.Segmented(c, "methods", &e.method, labels(c, k.Methods), kit.SegmentedProps{Block: true}).Label(t("connector_filter"))
		}
		if s := m.service(e.service); s != nil && s.Identity != "" {
			ui.Text(c, s.Identity).FontSize(14).TextColor(p.MutedForeground)
		}
		method := k.Methods[min(e.method, len(k.Methods)-1)]
		ui.Column(c.Key("content")).Gap(12).Children(func() {
			switch {
			case method == "connector_cli":
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
							}
						})
					}
					if e.accounts.Loading {
						accountSkeleton(c)
						break
					}
				} else {
					if !e.accounts.Loaded && !e.accounts.Loading {
						e.accounts.Start()
					}
					if e.accounts.Advance(c.Now()) {
						c.After(preview.Tick)
						accountSkeleton(c)
						break
					}
				}
				names := m.ConnAccounts
				modalList(c, "accounts", t("connector_choose_account"), "", t("connector_not_connected"), len(names), func() {
					for i, a := range names {
						sel := i == e.account
						listRow{selected: &sel, onClick: func() { e.account = i }}.row(c, i, a)
					}
				}, nil)
			case method == "connector_oauth" && len(k.Fields) == 0:
				ui.Column(c.Key("browser")).AlignItems(ui.Center).Gap(12).Padding(12).Children(func() {
					ui.Icon(c, icons.Globe).FontSize(32).TextColor(p.MutedForeground)
					ui.Text(c, t("connector_browser")).FontSize(14)
				})
			default:
				for _, f := range k.Fields {
					v := e.fields[f.Name]
					kit.Field(c, f.Name, f.Title, "", func() {
						ui.Row(c).Gap(4).Children(func() {
							in := ui.TextInput(c.Key("input"), &v).Label(f.Title).Grow(1)
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
										notice(c)
									}
								})
							}
						})
					})
					e.fields[f.Name] = v
				}
			}
			if method == "connector_application" {
				kit.Secondary(c.Key("register"), t("connector_register")).Height(28).AlignSelf(ui.Start).OnClick(func() {
					if m.Live && m.ConnectorActions != nil && m.ConnectorActions.Save != nil {
						m.ConnectorActions.Save(ConnectorDraft{Type: k.ID, Service: e.service, Method: method, ExpectedRevision: e.revision, Fields: maps.Clone(e.fields), Registration: true}, e.saving())
					} else {
						notice(c)
					}
				})
			}
		})
		action := "settings_save"
		switch method {
		case "connector_oauth":
			action = "connector_authorize"
			if e.service != "" {
				action = "connector_reauthorize"
			}
		case "connector_cli":
			action = "connector_bind"
		}
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("cancel"), t("authorization_cancel")).OnClick(func() { e.open = false })
			kit.Primary(c.Key("ok"), t(action), e.pending).Children(func() { ui.Text(c, t(action)).FontSize(14) }).OnClick(func() {
				for _, f := range k.Fields {
					if method != "connector_oauth" && method != "connector_cli" && !f.Optional && !(m.Live && f.Secret && e.service != "") && strings.TrimSpace(e.fields[f.Name]) == "" {
						kit.Toast(c, kit.Failed, t("mcp_invalid"))
						return
					}
				}
				if m.Live && m.ConnectorActions != nil && m.ConnectorActions.Save != nil {
					m.ConnectorActions.Save(ConnectorDraft{Type: k.ID, Service: e.service, Method: method, ExpectedRevision: e.revision, Fields: maps.Clone(e.fields)}, e.saving())
					return
				}
				if s := m.service(e.service); s == nil {
					st := preview.Connected
					if method == "connector_oauth" {
						st = preview.Connecting
					}
					m.Services = append(m.Services, preview.Service{ID: k.ID, Type: k.ID, Name: k.Name, Description: k.Description,
						Method: method, Account: true, State: st, AccountOn: true})
				}
				if method == "connector_oauth" {
					notice(c)
				} else {
					kit.Toast(c, kit.Done, t("connector_saved"))
				}
				e.open = false
			})
		})
	}).Width(min(520, width(c)-48)).Label(t("connector_add"))
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
		}
	}
}

// serviceDetails is the service with its summary over each server's facts,
// Close and Reconnect.
func serviceDetails(c *ui.Context, m *Model) {
	s := m.service(m.connDetails)
	if s == nil {
		return
	}
	p := theme.Of(c)
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	open := true
	kit.Dialog(c.Key("connector-details"), &open, func() {
		ui.Text(c, s.Name).FontSize(16).FontWeight(600)
		st := StatusOf(s)
		summary := s.Description
		switch {
		case st.Tools >= 0:
			summary = t("connector_connected") + " · " + t("connector_mcp_tools", "n", st.Tools)
		case st.Connecting:
			summary = t("connector_mcp_connecting")
		case st.Failed:
			summary = t("connector_mcp_failed")
		case st.Problem != "":
			summary = t(st.Problem)
		}
		kit.MediaRow(c, s.Name, summary, func() { serviceTile(c, s.Name) }, nil)
		fact := func(key, label string, value func()) {
			ui.Row(c.Key(key)).Gap(12).Padding(8, 12).Children(func() {
				ui.Text(c, label).FontSize(14).Width(96).Shrink(0).TextColor(p.MutedForeground)
				ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Children(value)
			})
		}
		ui.Column(c.Key("facts")).Border(1, p.Border).Radius(p.RadiusLg).Dividers(1, p.Border).Children(func() {
			for i, x := range s.Servers {
				ui.Column(c.Key(i)).Dividers(1, p.Border).Children(func() {
					fact("server", t("connector_mcp_server"), func() { ui.Text(c, x.Name).Font("monospace").FontSize(14).SingleLine() })
					if x.Tools > 0 && x.State != preview.ServerUp {
						fact("tools", t("market_details_tools"), func() { ui.Text(c, t("connector_mcp_tools", "n", x.Tools)).FontSize(14) })
					}
					if x.Updated != "" {
						fact("updated", t("connector_mcp_updated_at"), func() { ui.Text(c, x.Updated).FontSize(14) })
					}
					if x.Error != "" {
						fact("error", t("connector_mcp_error"), func() { ui.Text(c, x.Error).FontSize(14).Selectable() })
					}
				})
			}
		})
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("close"), t("close")).OnClick(func() { open = false })
			kit.Primary(c.Key("reconnect"), t("connector_mcp_reconnect"), false).
				Children(func() { ui.Text(c, t("connector_mcp_reconnect")).FontSize(14) }).OnClick(func() {
				if m.Live && m.ConnectorActions != nil && m.ConnectorActions.Reconnect != nil {
					m.ConnectorActions.Reconnect(s.ID)
				} else {
					notice(c)
				}
			})
		})
	}).Width(min(560, width(c)-48)).Label(s.Name)
	if !open {
		m.connDetails = ""
	}
}

// mcpDialog is name, transport, the command and its arguments or the URL,
// then the environment or headers, whose saved values stay masked; Cancel and
// Save.
func mcpDialog(c *ui.Context, m *Model) {
	e := &m.mcp
	if !e.open {
		return
	}
	p := theme.Of(c)
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	title := t("mcp_add")
	if e.original != "" {
		title = t("mcp_edit")
	}
	kit.Dialog(c.Key("mcp-editor"), &e.open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		ui.Scroll(c.Key("body")).Grow(1).Shrink(1).Gap(16).Children(func() {
			kit.Field(c, "name", t("settings_name"), t("mcp_name_hint"), func() {
				ui.TextInput(c.Key("input"), &e.name).Label(t("settings_name")).FillWidth()
			})
			kit.Field(c, "transport", t("mcp_transport"), "", func() {
				kit.Segmented(c, "tabs", &e.transport, []string{t("plugins_stdio"), t("plugins_http"), t("plugins_sse")}, kit.SegmentedProps{Block: true}).Label(t("mcp_transport"))
			})
			if e.transport == 0 {
				kit.Field(c, "command", t("mcp_command"), "", func() {
					ui.TextInput(c.Key("input"), &e.command).Label(t("mcp_command")).Placeholder(t("mcp_command_hint")).FillWidth().Font("monospace")
				})
				kit.Field(c, "args", t("mcp_arguments"), "", func() {
					ui.Column(c).Gap(6).Children(func() {
						for i := range e.args {
							ui.Row(c.Key([2]int{e.removed, i})).Gap(4).Children(func() {
								ui.TextInput(c.Key("input"), &e.args[i]).Label(t("mcp_argument")).Grow(1).Font("monospace")
								kit.Action(c.Key("remove"), icons.Close, t("mcp_remove_argument"), false).OnClick(func() {
									e.args = slices.Delete(e.args, i, i+1)
									e.removed++
								})
							})
						}
						kit.Secondary(c.Key("add"), t("mcp_add_argument")).Height(28).AlignSelf(ui.Start).OnClick(func() { e.args = append(e.args, "") })
					})
				})
			} else {
				kit.Field(c, "url", t("mcp_url"), "", func() {
					ui.TextInput(c.Key("input"), &e.url).Label(t("mcp_url")).Placeholder(t("mcp_url_hint")).FillWidth()
				})
			}
			slots, add := "mcp_environment", "mcp_add_environment"
			if e.transport != 0 {
				slots, add = "mcp_headers", "mcp_add_header"
			}
			kit.Field(c, "slots", t(slots), "", func() {
				ui.Column(c).Gap(6).Children(func() {
					for i := range e.slots {
						ui.Row(c.Key([2]int{e.removed, i})).Gap(4).Children(func() {
							ui.TextInput(c.Key("name"), &e.slots[i].Key).Label(t("mcp_slot_name")).Placeholder(t("mcp_slot_name")).Width(160)
							hint := t("form_secret_hint")
							if i < len(e.saved) && e.saved[i] {
								hint = t("mcp_keep_secret")
							}
							ui.TextInput(c.Key("value"), &e.slots[i].Value).Password().Label(t("mcp_slot_value")).Placeholder(hint).Grow(1)
							kit.Action(c.Key("remove"), icons.Close, t("mcp_remove_slot"), false).OnClick(func() {
								e.slots = slices.Delete(e.slots, i, i+1)
								if i < len(e.saved) {
									e.saved = slices.Delete(e.saved, i, i+1)
								}
								e.removed++
							})
						})
					}
					kit.Secondary(c.Key("add"), t(add)).Height(28).AlignSelf(ui.Start).OnClick(func() { e.slots = append(e.slots, preview.EnvVar{}) })
				})
			})
			help := "mcp_install_stdio_help"
			if e.transport != 0 {
				help = "mcp_install_http_help"
			}
			ui.Text(c, t(help)).FontSize(12).TextColor(p.MutedForeground)
		})
		footer(c, t("settings_save"), func() { e.open = false }, func() {
			if problem := e.Check(); problem != "" {
				kit.Toast(c, kit.Failed, t(problem))
				return
			}
			if m.Live && m.ConnectorActions != nil && m.ConnectorActions.SaveMCP != nil {
				if e.request != nil {
					return
				}
				request := new(int)
				e.request = request
				draft := MCPDraft{Service: e.original, Name: strings.TrimSpace(e.name), Revision: e.revision,
					Server: preview.Server{Name: strings.TrimSpace(e.name), Transport: e.transport, Command: e.command, Args: slices.Clone(e.args), URL: e.url, Env: slices.Clone(e.slots)}}
				m.ConnectorActions.SaveMCP(draft, func(ok bool) {
					if e.request != request {
						return
					}
					e.request = nil
					if ok {
						e.open = false
					}
				})
				return
			}
			e.save(m)
			kit.Toast(c, kit.Done, t("mcp_configuration_saved"))
			e.open = false
		})
	}).Width(min(520, width(c)-48)).Label(title)
}

// Check validates the server: the i18n key of the problem, or "".
func (e *mcpEditor) Check() string {
	n := strings.TrimSpace(e.name)
	ok := n != ""
	for _, r := range n {
		if !(r >= 'a' && r <= 'z' || r >= '0' && r <= '9' || r == '.' || r == '-') {
			ok = false
		}
	}
	if !ok {
		return "mcp_invalid"
	}
	if e.transport == 0 && strings.TrimSpace(e.command) == "" {
		return "mcp_invalid"
	}
	if e.transport != 0 && !strings.HasPrefix(e.url, "http://") && !strings.HasPrefix(e.url, "https://") {
		return "mcp_invalid"
	}
	for _, s := range e.slots {
		if strings.TrimSpace(s.Key) == "" {
			return "mcp_invalid"
		}
	}
	return ""
}

func (e *mcpEditor) save(m *Model) {
	name := strings.TrimSpace(e.name)
	x := preview.Server{Name: "mcp-" + name, Transport: e.transport, Command: e.command, Args: e.args, URL: e.url, Allowed: true, State: preview.ServerStarting}
	for _, s := range e.slots {
		x.Env = append(x.Env, preview.EnvVar{Key: s.Key}) // values are never kept
	}
	desc := e.url
	if e.transport == 0 {
		desc = strings.TrimSpace(e.command + " " + strings.Join(e.args, " "))
	}
	if s := m.service(e.original); s != nil {
		s.Name, s.Description, s.Servers = name, desc, []preview.Server{x}
		return
	}
	m.Services = append(m.Services, preview.Service{ID: fmt.Sprintf("mcp-%s-%d", name, len(m.Services)), Name: name, Description: desc, Manual: true, Servers: []preview.Server{x}})
}

// accountSkeleton holds the account row's place while the Node inspects the
// local CLI: a 24 DIP round avatar beside a name and a secondary line.
func accountSkeleton(c *ui.Context) {
	ui.Column(c.Key("account-skeleton")).FillWidth().Busy(true).Children(func() {
		modalList(c, "accounts", i18n.T(c, "connector_choose_account"), "", "", 1, func() {
			listRow{leading: func() {
				ui.Row(c).Gap(12).Children(func() {
					kit.SkeletonAvatar(c, 24, true, kit.SkeletonFade)
					ui.Column(c).Gap(4).Children(func() {
						kit.Skeleton(c, kit.SkeletonProps{Width: 120, Height: 12, Radius: 2})
						kit.Skeleton(c, kit.SkeletonProps{Width: 80, Height: 10, Radius: 2, Secondary: true})
					})
				})
			}}.row(c, "account-loading", "")
		}, nil)
	})
}
