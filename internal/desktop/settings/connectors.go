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

// connectorMethods are the filter's choices, "" for all. No authentication
// is a service without an account: its servers connect as they are.
var connectorMethods = []string{"", "connector_no_auth", "connector_oauth", "connector_application", "connector_token", "connector_cli"}

// ConnStatus is a service's status: the i18n key of a problem, or "" with the
// status kind. Reason is why a failed or unavailable service is so, when the
// host said.
type ConnStatus struct {
	Problem    string
	Connecting bool
	Failed     bool
	Reason     string
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
	tools, up, connecting := 0, false, false
	for _, x := range s.Servers {
		if !x.Allowed {
			return ConnStatus{Problem: "connector_server_disabled", Tools: -1}
		}
		if x.Problem != "" {
			return ConnStatus{Problem: x.Problem, Reason: x.Error, Tools: -1}
		}
		switch x.State {
		case preview.ServerAuth:
			return ConnStatus{Problem: "connector_not_connected", Tools: -1}
		case preview.ServerDown:
			return ConnStatus{Failed: true, Reason: x.Error, Tools: -1}
		case preview.ServerStarting:
			connecting = true
		case preview.ServerUp:
			up = true
			tools += x.Tools
		}
	}
	switch {
	case connecting:
		return ConnStatus{Connecting: true, Tools: -1}
	case up:
		return ConnStatus{Tools: tools}
	case !s.Account:
		// Nothing connected yet: no server, or none asked to connect.
		return ConnStatus{Problem: "connector_not_connected", Tools: -1}
	}
	return ConnStatus{Tools: -1}
}

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
	marks     []slotMark // beside slots: what is saved for each and how it shows
	revision  uint64
	// source is the transport the saved server has: a credential saved for
	// a command does not carry over to a URL, nor one for a URL to a command.
	source int
	// declared is a plugin's own server: its name and transport are fixed.
	declared bool
	// loading is the saved configuration being read and unread that the
	// read failed, which leaves the editor locked with Reload. deferred is
	// an editor that opens only once its configuration is read.
	loading, unread, deferred bool
	// uncertain is a save whose outcome is not known: the draft is locked
	// and Save retries that same save. detail is the host's reason for the
	// last failure, shown while details is on.
	uncertain bool
	detail    string
	details   bool
	// request is the save under way: Save waits for it, and the editor that
	// replaced this one has another or none, so a late completion does not
	// close it.
	request *int
	// removed counts the rows taken out of args and slots. It is part of a
	// row's identity: a row that moves into a removed one's place does not
	// take its text state (undo history, selection) with the place.
	removed int
}

// slotMark is a slot's saved credential: the name it is saved under, "" for
// none, whether saving clears it, and whether its typed value shows.
type slotMark struct {
	saved string
	clear bool
	shown bool
}

// mark is the slot's mark, made on first use for a slot added since.
func (e *mcpEditor) mark(i int) *slotMark {
	for len(e.marks) < len(e.slots) {
		e.marks = append(e.marks, slotMark{})
	}
	return &e.marks[i]
}

// kept reports whether saving keeps the slot's saved credential: nothing
// typed over it, under the name and the kind of transport it was saved for,
// and not marked to clear.
func (e *mcpEditor) kept(i int) bool {
	mark := e.mark(i)
	return e.pinned(i) && !mark.clear && (e.source == 0) == (e.transport == 0)
}

// pinned reports whether the slot still names its saved credential with no
// value typed over it.
func (e *mcpEditor) pinned(i int) bool {
	mark := e.mark(i)
	return mark.saved != "" && mark.saved == strings.TrimSpace(e.slots[i].Key) && e.slots[i].Value == ""
}

// locked reports whether the draft cannot change now: its configuration is
// not read, or a save is under way or uncertain.
func (e *mcpEditor) locked() bool {
	return e.loading || e.unread || e.request != nil || e.uncertain
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
				m.mcp = mcpEditor{open: true}
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
			switch f := connectorMethods[m.ConnMethod]; {
			case f == "":
			case f == "connector_no_auth":
				if s.Account {
					continue
				}
			case s.Method != f:
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
	// The connector's editor is over the server's, whose Authentication opens
	// it, and a package's settings form is where its servers are configured.
	serviceDetails(c, m)
	mcpDialog(c, m)
	PackageConfigurationDialog(c, m)
	connectorDialog(c, m)
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
			serviceTile(c, actions.Authorization.Title, "")
		}, actions.Retry, func() {
			if actions.Submit != nil {
				actions.Submit(actions.Authorization.Pasted)
			}
		})
	}
	if s := m.service(m.connRemoving); s != nil {
		open := true
		if confirm(c, "remove", &open, t("connector_remove"), t("connector_remove_effect", "name", s.Name), t("connector_remove")) {
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

// serviceTile is a service's icon: the one its connector or package
// declares, the plug for none.
func serviceTile(c *ui.Context, name, icon string) {
	iconTile(c, name, icon, "", icons.Plug, kit.TileSize)
}

// statusView is a problem as muted text, a spinner while connecting, a dot
// with the tools or the failure; busy puts a spinner before a status that
// has none while a command of the service runs. Its tooltip is the known
// reason and when the server last reported, updated.
func statusView(c *ui.Context, st ConnStatus, updated string, busy bool) {
	p := theme.Of(c)
	row := ui.Row(c.Key("status")).Shrink(0).Gap(8).Busy(busy || st.Connecting)
	tip := st.Reason
	if updated != "" {
		if tip != "" {
			tip += "\n"
		}
		tip += i18n.T(c, "connector_mcp_updated", "time", updated)
	}
	kit.Tooltip(c, row, tip)
	row.Children(func() {
		muted := func(s string) { ui.Text(c, s).FontSize(14).TextColor(p.MutedForeground).SingleLine() }
		if busy && !st.Connecting {
			kit.Busy(c)
		}
		switch {
		case st.Problem != "":
			muted(i18n.T(c, st.Problem))
		case st.Connecting:
			kit.Busy(c)
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
	// A command of the service under way holds its controls: none of them
	// is asked for twice.
	busy := m.Live && m.ConnectorActions != nil && m.ConnectorActions.Busy != nil && m.ConnectorActions.Busy(s.ID)
	items := m.serviceItems(s)
	kit.Entry(c, s.ID, func() {
		kit.MediaRow(c, s.Name, desc, func() { serviceTile(c, s.Name, s.Icon) }, nil)
	}, func() {
		ui.Row(c).Gap(12).Children(func() {
			// Connect is in the menu (serviceItems), not a button here.
			statusView(c, st, serviceUpdated(s), busy)
			switch {
			case s.Account:
				if ui.Switch(c.Key("account"), &s.AccountOn).Label(t("connector_account_enabled")+" "+s.Name).Disabled(busy || s.State == preview.Connecting).Changed() && m.Live && m.ConnectorActions != nil && m.ConnectorActions.Enable != nil {
					m.ConnectorActions.Enable(s.ID, s.AccountOn)
				}
			case len(s.Servers) == 1:
				if ui.Switch(c.Key("allowed"), &s.Servers[0].Allowed).Label(t("connector_server_allowed") + " " + s.Name).Disabled(busy).Changed() {
					if m.Live && m.ConnectorActions != nil && m.ConnectorActions.AllowMCP != nil {
						m.ConnectorActions.AllowMCP(s.ID, s.Servers[0].Name, s.Servers[0].Allowed)
					} else {
						kit.Toast(c, kit.Done, t("connector_saved"))
					}
				}
			}
			if len(items) == 0 || busy {
				kit.Action(c.Key("menu"), icons.Ellipsis, t("more"), true)
				return
			}
			kit.MenuButton(c.Key("menu"), "connector-menu-"+s.ID, func(b ui.Element) {
				kit.IconLook(c, b, icons.Ellipsis, t("more"), "", 28, false)
			}, func(menu *Menu) {
				for _, k := range items {
					it := menu.Item(t(k))
					if k == "connector_remove" {
						it.Danger()
					}
					if it.Chosen() {
						m.serviceAction(c, s, k)
					}
				}
			}, kit.MenuOptions{At: ui.AnchorBottomEnd, Self: ui.AnchorTopEnd})
		})
	}, nil).ContextMenu(func(menu *ui.Menu) {
		// The row's own menu, as the system draws it.
		for _, k := range items {
			if menu.Item(t(k)).Disabled(busy).Chosen() {
				m.serviceAction(c, s, k)
			}
		}
	})
}

// serviceUpdated is when the service's server last reported: the latest of
// several.
func serviceUpdated(s *preview.Service) (updated string) {
	if s.Account && !s.AccountOn {
		return ""
	}
	for _, x := range s.Servers {
		if x.Allowed && x.Problem == "" {
			updated = max(updated, x.Updated)
		}
	}
	return updated
}

// serviceAction does the row menu's item k for the service.
func (m *Model) serviceAction(c *ui.Context, s *preview.Service, k string) {
	actions := m.ConnectorActions
	if !m.Live || actions == nil {
		actions = &ConnectorActions{}
	}
	switch k {
	case "connector_remove":
		m.connRemoving = s.ID
	case "connector_details":
		m.connDetails = s.ID
	case "connector_confirm":
		if actions.Confirm != nil {
			actions.Confirm(s.ID)
		} else {
			m.editService(s)
		}
	case "connector_probe":
		if actions.Probe != nil {
			actions.Probe(s.ID)
		} else {
			notice(c)
		}
	case "connector_mcp_reconnect":
		if actions.Reconnect != nil {
			actions.Reconnect(s.ID)
		} else {
			notice(c)
		}
	case "connector_add_credentials":
		m.addCredentials(s)
	case "connector_connect":
		if s.Account {
			m.editService(s)
		} else {
			m.addCredentials(s)
		}
	case "connector_configure", "connector_bind", "connector_reauthorize":
		if s.Manual {
			m.configureServer(s)
		} else {
			m.editService(s)
		}
	}
}

// configureServer opens the editor of a server added by hand. The preview's
// row is the whole server; a live row is not, so its editor opens locked on
// what the row has while the Node reads the saved configuration and which
// slots hold a credential.
func (m *Model) configureServer(s *preview.Service) {
	if len(s.Servers) == 0 {
		return
	}
	if !m.Live || m.ConnectorActions == nil || m.ConnectorActions.ConfigureMCP == nil {
		var saved []string
		for _, slot := range s.Servers[0].Env {
			saved = append(saved, slot.Key)
		}
		m.editServer(s, saved)
		return
	}
	m.editServer(s, nil)
	m.mcp.loading = true
	m.ConnectorActions.ConfigureMCP(s.ID)
}

// configurePackage asks for the configuration of a plugin's own servers: its
// settings form when it declares one, else the server's editor once read.
func (m *Model) configurePackage(s *preview.Service) {
	m.mcp = mcpEditor{original: s.ID, loading: true, deferred: true}
	m.ConnectorActions.ConfigureMCP(s.ID)
}

// serviceItems are the actions the service's row can do now, as i18n keys in
// order: configure and delete for servers added by hand; the account's edit,
// check and delete; the credentials of a service whose plugin declares a
// connector; then reconnect for servers that may connect and details for
// services with servers. An action that cannot act is not listed.
func (m *Model) serviceItems(s *preview.Service) []string {
	var out []string
	kind := m.connType(s.Type)
	waiting := slices.ContainsFunc(s.Servers, func(x preview.Server) bool {
		return x.Allowed && x.Problem == "" && x.State == preview.ServerAuth
	})
	switch {
	case s.Manual:
		out = []string{"connector_configure", "connector_remove"}
		if waiting && kind != nil {
			out = append(out, "connector_connect")
		}
	case s.Account:
		if kind != nil {
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
			if kind.Enabled && s.AccountOn && len(s.Servers) == 0 && (s.State == preview.Connected || s.State == preview.ConnFailed || s.State == preview.Expired) {
				out = append(out, "connector_probe")
			}
		}
		out = append(out, "connector_remove")
	case kind != nil && kind.Enabled && len(kind.Methods) > 0:
		if waiting {
			out = append(out, "connector_connect")
		} else {
			out = append(out, "connector_add_credentials")
		}
	}
	if reconnectable(s) {
		out = append(out, "connector_mcp_reconnect")
	}
	if len(s.Servers) > 0 {
		out = append(out, "connector_details")
	}
	return out
}

// reconnectable reports whether a server of the service may be asked to
// connect: allowed, with nothing in its way and not connecting already.
func reconnectable(s *preview.Service) bool {
	if s.Account && !s.AccountOn {
		return false
	}
	return slices.ContainsFunc(s.Servers, func(x preview.Server) bool {
		return x.Allowed && x.Problem == "" && x.State != preview.ServerAuth && x.State != preview.ServerStarting
	})
}

func (m *Model) connType(id string) *preview.ConnectorType {
	for i := range m.ConnTypes {
		if id != "" && m.ConnTypes[i].ID == id {
			return &m.ConnTypes[i]
		}
	}
	return nil
}

// addCredentials opens the editor on the service's connector for a new
// account: its methods and fields, as Add after choosing the connector.
func (m *Model) addCredentials(s *preview.Service) {
	if k := m.connType(s.Type); k != nil {
		m.connector = connectorEditor{open: true, kind: k, fields: map[string]string{}, direct: true}
	}
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

// editServer opens the editor on the service's server. The slots named in
// saved hold a credential; no slot carries a value.
func (m *Model) editServer(s *preview.Service, saved []string) {
	x := s.Servers[0]
	m.mcp = mcpEditor{open: true, original: s.ID, revision: s.Revision, name: s.Name, transport: x.Transport,
		source: x.Transport, command: x.Command, args: slices.Clone(x.Args), url: x.URL}
	for _, slot := range x.Env {
		mark := slotMark{}
		if slices.Contains(saved, slot.Key) {
			mark.saved = slot.Key
		}
		m.mcp.slots = append(m.mcp.slots, preview.EnvVar{Key: slot.Key})
		m.mcp.marks = append(m.mcp.marks, mark)
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
							leading: func() { ui.Row(c).Size(32, 32).Center().Children(func() { serviceTile(c, k.Name, k.Icon) }) },
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
			if e.service == "" && !e.direct {
				kit.Action(c.Key("back"), icons.ArrowLeft, t("provider_step_back"), e.pending).OnClick(func() { e.kind = nil })
			}
			ui.Row(c).Size(32, 32).Center().Children(func() { serviceTile(c, k.Name, k.Icon) })
			ui.Text(c, k.Name).FontSize(18).FontWeight(600)
		})
		if len(k.Methods) > 1 && e.service == "" {
			kit.Segmented(c, "methods", &e.method, labels(c, k.Methods), kit.SegmentedProps{Block: true}).Label(t("connector_filter"))
		}
		if s := m.service(e.service); s != nil && s.Identity != "" {
			ui.Text(c, s.Identity).FontSize(14).TextColor(p.MutedForeground)
		}
		method := k.Methods[min(e.method, len(k.Methods)-1)]
		// An existing OAuth or CLI connection with fields saves them as they
		// are; authorizing or binding again is asked for apart.
		existing := m.service(e.service)
		acquired := method == "connector_oauth" || method == "connector_cli"
		configuring := acquired && existing != nil && len(k.Fields) > 0 && !e.reacquire &&
			(method == "connector_cli" || existing.State == preview.Connected)
		ready := true
		ui.Column(c.Key("content")).Gap(12).Children(func() {
			switch {
			case method == "connector_cli" && !configuring:
				// There is an account to bind once the local CLI was looked at
				// and has one.
				ready = false
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
				ready = len(names) > 0
				if !ready {
					// Nothing to bind: look at the local CLI again.
					kit.TextAction(c.Key("retry"), t("chat_retry"), e.pending).AlignSelf(ui.End).OnClick(func() {
						e.cliStarted, e.accounts = false, preview.Fetch{}
					})
				}
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
										notice(c)
									}
								})
							}
						})
					})
					e.fields[f.Name] = v
					// A field the method needs has a value, or for a saved
					// connection's secret the one the Node holds. Authorizing
					// in the browser asks for its own.
					if (configuring || !acquired) && !f.Optional && !(m.Live && f.Secret && e.service != "") && strings.TrimSpace(v) == "" {
						ready = false
					}
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
						m.ConnectorActions.Save(ConnectorDraft{Type: k.ID, Service: e.service, Method: method, ExpectedRevision: e.revision, Fields: maps.Clone(e.fields), Registration: true}, e.saving())
					} else {
						notice(c)
					}
				})
			}
		})
		action := "settings_save"
		switch {
		case configuring:
		case method == "connector_oauth" && e.service != "":
			action = "connector_reauthorize"
		case method == "connector_oauth":
			action = "connector_authorize"
		case method == "connector_cli":
			action = "connector_bind"
		}
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("cancel"), t("authorization_cancel")).OnClick(func() { e.open = false })
			pendingButton(c, "ok", t(action), !ready, e.pending).OnClick(func() {
				if !ready || e.pending {
					return
				}
				if m.Live && m.ConnectorActions != nil && m.ConnectorActions.Save != nil {
					m.ConnectorActions.Save(ConnectorDraft{Type: k.ID, Service: e.service, Method: method, ExpectedRevision: e.revision, Fields: maps.Clone(e.fields), Update: configuring}, e.saving())
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
				if method == "connector_oauth" && !configuring {
					notice(c)
				} else {
					kit.Toast(c, kit.Done, t("connector_saved"))
				}
				e.open = false
			})
		})
	}).Width(min(520, width(c)-48)).Label(t("connector_add"))
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
	live := m.Live && m.ConnectorActions != nil
	busy := live && m.ConnectorActions.Busy != nil && m.ConnectorActions.Busy(s.ID)
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
		if st.Reason != "" {
			summary += " · " + st.Reason
		}
		kit.MediaRow(c, s.Name, summary, func() { serviceTile(c, s.Name, s.Icon) }, nil)
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
					if len(s.Servers) > 1 {
						// One server's status is the summary already.
						one := *s
						one.Servers = []preview.Server{x}
						fact("status", t("connector_mcp_status"), func() { statusView(c, StatusOf(&one), "", false) })
					}
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
			// A click is handled after this pass, so it closes through the
			// model, not through this pass's open.
			kit.Secondary(c.Key("close"), t("close")).OnClick(func() { m.connDetails = "" })
			if live && !s.Manual && m.ConnectorActions.ConfigureMCP != nil {
				// A plugin's own servers: its settings form, or the server's
				// editor. A server added by hand has Configure in its menu.
				kit.Secondary(c.Key("configure"), t("connector_configure")).Disabled(busy).OnClick(func() {
					m.connDetails = ""
					m.configurePackage(s)
				})
			}
			if connecting := st.Connecting && live; connecting || reconnectable(s) {
				pendingButton(c, "reconnect", t("connector_mcp_reconnect"), busy, connecting).OnClick(func() {
					if !busy && reconnectable(s) {
						m.serviceAction(c, s, "connector_mcp_reconnect")
					}
				})
			}
		})
	}).Width(min(560, width(c)-48)).Label(s.Name)
	if !open {
		m.connDetails = ""
	}
}

// authenticate opens the connector that authorizes the service's servers:
// its account when it has one, else the connector's editor for a new one.
func (m *Model) authenticate(s *preview.Service) {
	if account := m.service(s.Type); account != nil && account.Account {
		m.editService(account)
		return
	}
	m.addCredentials(s)
}

// mcpDialog is name, transport, the command and its arguments or the URL,
// then the environment or headers, whose saved credentials stay unless typed
// over or cleared; then the last failure's reason when asked for, and
// Authentication, Reload, Details, Cancel and Save. A saved server's editor
// is locked until its configuration is read, and while a save is uncertain.
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
	locked := e.locked()
	live := m.Live && m.ConnectorActions != nil
	kit.Dialog(c.Key("mcp-editor"), &e.open, func() {
		ui.Row(c.Key("title")).Gap(8).Children(func() {
			ui.Text(c, title).FontSize(16).FontWeight(600)
			if e.loading {
				kit.Busy(c)
			}
		})
		ui.Scroll(c.Key("body")).Grow(1).Shrink(1).Gap(16).Busy(e.loading).Children(func() {
			kit.Field(c, "name", t("settings_name"), t("mcp_name_hint"), func() {
				// A saved server keeps the name it was added under.
				ui.TextInput(c.Key("input"), &e.name).Label(t("settings_name")).FillWidth().Disabled(locked || e.original != "")
			})
			kit.Field(c, "transport", t("mcp_transport"), "", func() {
				kit.Segmented(c, "tabs", &e.transport, []string{t("plugins_stdio"), t("plugins_http"), t("plugins_sse")}, kit.SegmentedProps{Block: true}).
					Label(t("mcp_transport")).Disabled(locked || e.declared)
			})
			if e.transport == 0 {
				kit.Field(c, "command", t("mcp_command"), "", func() {
					ui.TextInput(c.Key("input"), &e.command).Label(t("mcp_command")).Placeholder(t("mcp_command_hint")).FillWidth().Font("monospace").Disabled(locked)
				})
				kit.Field(c, "args", t("mcp_arguments"), "", func() {
					ui.Column(c).Gap(6).Children(func() {
						for i := range e.args {
							ui.Row(c.Key([2]int{e.removed, i})).Gap(4).Children(func() {
								ui.TextInput(c.Key("input"), &e.args[i]).Label(t("mcp_argument")).Grow(1).Font("monospace").Disabled(locked)
								kit.Action(c.Key("remove"), icons.Close, t("mcp_remove_argument"), locked).OnClick(func() {
									e.args = slices.Delete(e.args, i, i+1)
									e.removed++
								})
							})
						}
						kit.Secondary(c.Key("add"), t("mcp_add_argument")).Height(28).AlignSelf(ui.Start).Disabled(locked).OnClick(func() { e.args = append(e.args, "") })
					})
				})
			} else {
				kit.Field(c, "url", t("mcp_url"), "", func() {
					ui.TextInput(c.Key("input"), &e.url).Label(t("mcp_url")).Placeholder(t("mcp_url_hint")).FillWidth().Disabled(locked)
				})
			}
			slots, add := "mcp_environment", "mcp_add_environment"
			if e.transport != 0 {
				slots, add = "mcp_headers", "mcp_add_header"
			}
			kit.Field(c, "slots", t(slots), "", func() {
				ui.Column(c).Gap(6).Children(func() {
					for i := range e.slots {
						ui.Column(c.Key([2]int{e.removed, i})).Gap(4).Children(func() {
							shown := e.mark(i).shown
							ui.Row(c).Gap(4).Children(func() {
								ui.TextInput(c.Key("name"), &e.slots[i].Key).Label(t("mcp_slot_name")).Placeholder(t("mcp_slot_name")).Width(160).Disabled(locked)
								value := ui.TextInput(c.Key("value"), &e.slots[i].Value).Label(t("mcp_slot_value")).Placeholder(t("form_secret_hint")).Grow(1).Disabled(locked)
								mask := "mcp_hide_secret"
								if !shown {
									value.Password()
									mask = "mcp_show_secret"
								}
								kit.IconButton(c.Key("mask"), icons.Eye, t(mask), 28, shown).OnClick(func() { e.mark(i).shown = !shown })
								kit.Action(c.Key("remove"), icons.Close, t("mcp_remove_slot"), locked).OnClick(func() {
									e.mark(i)
									e.slots, e.marks = slices.Delete(e.slots, i, i+1), slices.Delete(e.marks, i, i+1)
									e.removed++
								})
							})
							if !e.pinned(i) {
								return
							}
							// The saved credential under this name: kept as it
							// is, or cleared by the save when asked to or when
							// the transport it was saved for is left.
							ui.Row(c.Key("saved")).Gap(8).Children(func() {
								note := "mcp_secret_cleared"
								if e.kept(i) {
									note = "mcp_secret_saved"
								}
								ui.Text(c, t(note)).FontSize(12).TextColor(p.MutedForeground).Grow(1).Shrink(1).MinWidth(0)
								if (e.source == 0) != (e.transport == 0) {
									return
								}
								toggle := "mcp_clear_secret"
								if e.mark(i).clear {
									toggle = "mcp_keep_secret"
								}
								kit.TextAction(c.Key("clear"), t(toggle), locked).OnClick(func() { e.mark(i).clear = !e.mark(i).clear })
							})
						})
					}
					kit.Secondary(c.Key("add"), t(add)).Height(28).AlignSelf(ui.Start).Disabled(locked).OnClick(func() { e.slots = append(e.slots, preview.EnvVar{}) })
				})
			})
			help := "mcp_install_stdio_help"
			if e.transport != 0 {
				help = "mcp_install_http_help"
			}
			ui.Text(c, t(help)).FontSize(12).TextColor(p.MutedForeground)
		})
		if e.details && e.detail != "" {
			ui.Text(c.Key("detail"), e.detail).FontSize(12).TextColor(p.MutedForeground).Selectable()
		}
		reload := func() {
			if live && m.ConnectorActions.ConfigureMCP != nil && !e.loading {
				e.loading = true
				m.ConnectorActions.ConfigureMCP(e.original)
			}
		}
		save := func() {
			if e.request != nil || e.loading || e.unread {
				return
			}
			if problem := e.Check(); problem != "" {
				kit.Toast(c, kit.Failed, t(problem))
				return
			}
			if live && m.ConnectorActions.SaveMCP != nil {
				request := new(int)
				e.request = request
				draft := e.draft()
				draft.Failed = func(uncertain bool, detail string) {
					if e.request == request {
						e.uncertain, e.detail = uncertain, detail
					}
				}
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
		}
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			if s := m.service(e.original); s != nil {
				// The connector that authorizes this server, when its package
				// declares one: its editor opens over this one.
				if k := m.connType(s.Type); k != nil && k.Enabled && len(k.Methods) > 0 {
					kit.TextAction(c.Key("authorize"), t("connector_authentication"), e.loading || e.request != nil).OnClick(func() { m.authenticate(s) })
				}
			}
			if e.unread {
				kit.Secondary(c.Key("reload"), t("plugins_settings_reload")).Disabled(e.loading).OnClick(reload)
			}
			if e.detail != "" {
				kit.TextAction(c.Key("diagnostics"), t("plugins_settings_diagnostics"), false).OnClick(func() { e.details = !e.details })
			}
			kit.Secondary(c.Key("cancel"), t("settings_cancel")).OnClick(func() { e.open = false })
			// A save whose outcome is unknown is asked for again as it was.
			action := t("settings_save")
			if e.uncertain {
				action = t("plugins_retry")
			}
			pendingButton(c, "ok", action, e.loading || e.unread, e.request != nil).OnClick(save)
		})
	}).Width(min(520, width(c)-48)).Label(title)
}

// draft is the server as it is saved: its slots under their trimmed names,
// and the ones whose saved credential stays.
func (e *mcpEditor) draft() MCPDraft {
	name := strings.TrimSpace(e.name)
	draft := MCPDraft{Service: e.original, Name: name, Revision: e.revision,
		Server: preview.Server{Name: name, Transport: e.transport, Command: e.command, Args: slices.Clone(e.args), URL: e.url}}
	for i, slot := range e.slots {
		key := strings.TrimSpace(slot.Key)
		draft.Server.Env = append(draft.Server.Env, preview.EnvVar{Key: key, Value: slot.Value})
		if e.kept(i) {
			draft.Keep = append(draft.Keep, key)
		}
	}
	return draft
}

// Check validates the server: the i18n key of the problem, or "". A saved
// server's name is not the draft's to change.
func (e *mcpEditor) Check() string {
	n := strings.TrimSpace(e.name)
	ok := n != ""
	for _, r := range n {
		if !(r >= 'a' && r <= 'z' || r >= '0' && r <= '9' || r == '.' || r == '-') {
			ok = false
		}
	}
	if !ok && e.original == "" {
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
