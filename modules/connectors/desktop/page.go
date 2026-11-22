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
)

func Page() settings.NativePage {
	return settings.NativePage{
		Section: settings.Connectors,
		Group:   "settings_group_ai",
		Icon:    icons.Plug,
		Order:   150,
		Render:  render,
	}
}

type page struct {
	*settings.Model
	query  string
	method int
	dialogs
}

type dialogs struct {
	connector    connectorEditor
	mcp          mcpEditor
	connDetails  string
	connRemoving string
}

func pageFor(m *settings.Model) *page {
	if p, ok := m.ConnectorPageState.(*page); ok {
		return p
	}
	p := &page{Model: m}
	m.ConnectorPageState = p
	return p
}

func (m *page) CloseDialogs() { m.dialogs = dialogs{} }

func render(c *ui.Context, s *settings.State) {
	pageFor(s.Model).render(c)
}

// The Connectors page and the MCP editor (settings/mcp*): a toolbar of
// search, method filter, Add and Add MCP; each service a row with its status,
// its switch and menu; the add dialog (choose a connector, then its method
// and fields), the details dialog, and the MCP server editor.

// connectorMethods are the filter's choices, "" for all. No authentication
// is a service without an account: its servers connect as they are.
var connectorMethods = []string{"", "connector_no_auth", "connector_oauth", "connector_application", "connector_token", "connector_cli"}

func (m *page) render(c *ui.Context) {
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	ui.Column(c.Key("connectors")).Gap(16).Children(func() {
		ui.Row(c.Key("toolbar")).Gap(12).Children(func() {
			ui.SearchField(c.Key("search"), &m.query).Placeholder(t("connector_search")).Label(t("connector_search")).Grow(1)
			labels := []string{t("connector_all_methods")}
			for _, k := range connectorMethods[1:] {
				labels = append(labels, t(k))
			}
			settings.Pick(c, "method", t("connector_filter"), &m.method, labels, false).Width(192)
			settings.IconButton(c, "add", nil, t("settings_add"), true, false).OnClick(func() {
				m.connector = connectorEditor{open: true, fields: map[string]string{}}
			})
			kit.Secondary(c.Key("add-mcp"), t("mcp_add")).OnClick(func() {
				m.mcp = mcpEditor{open: true}
			})
		})
		if !m.Live {
			kit.Tag(c, t("connector_preview")).AlignSelf(ui.Start)
		}
		q := strings.ToLower(strings.TrimSpace(m.query))
		var shown []int
		for i := range m.Services {
			s := &m.Services[i]
			if q != "" && !strings.Contains(strings.ToLower(s.Name+" "+s.Description), q) {
				continue
			}
			switch f := connectorMethods[m.method]; {
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
	// A package's settings form is where its servers are configured; the
	// connector's editor opens from the service's row.
	serviceDetails(c, m)
	mcpDialog(c, m)
	settings.PackageConfigurationDialog(c, m.Model)
	connectorDialog(c, m)
	connectorAuthorizationDialog(c, m)
	if s := m.service(m.connRemoving); s != nil {
		open := true
		if settings.Confirm(c, "remove", &open, t("connector_remove"), t("connector_remove_effect", "name", s.Name), t("connector_remove")) {
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

func (m *page) service(id string) *preview.Service {
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
	settings.IconTile(c, name, icon, "", icons.Plug, kit.TileSize)
}

// statusView is a problem as muted text, a spinner while connecting, a dot
// with the tools or the failure; busy puts a spinner before a status that
// has none while a command of the service runs. Its tooltip is the known
// reason and when the server last reported, updated.
func statusView(c *ui.Context, st settings.ConnStatus, updated string, busy bool) {
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
func serviceRow(c *ui.Context, m *page, s *preview.Service) {
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	desc := s.Description
	if s.Identity != "" {
		desc = s.Identity
	}
	st := settings.StatusOf(s)
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
			}, func(menu *kit.Menu) {
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
func (m *page) serviceAction(c *ui.Context, s *preview.Service, k string) {
	actions := m.ConnectorActions
	if !m.Live || actions == nil {
		actions = &settings.ConnectorActions{}
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
			settings.PreviewNotice(c)
		}
	case "connector_mcp_reconnect":
		if actions.Reconnect != nil {
			actions.Reconnect(s.ID)
		} else {
			settings.PreviewNotice(c)
		}
	case "connector_add_credentials":
		m.authenticate(s)
	case "connector_connect":
		if s.Account {
			m.editService(s)
		} else {
			m.authenticate(s)
		}
	case "connector_configure", "connector_bind", "connector_reauthorize":
		if s.Manual {
			m.configureServer(s)
		} else {
			m.editService(s)
		}
	}
}

// serviceItems are the actions the service's row can do now, as i18n keys in
// order: configure and delete for servers added by hand; the account's edit,
// check and delete; the credentials of a service whose plugin declares a
// connector; then reconnect for servers that may connect and details for
// services with servers. An action that cannot act is not listed.
func (m *page) serviceItems(s *preview.Service) []string {
	var out []string
	kind := m.connType(s.Type)
	waiting := slices.ContainsFunc(s.Servers, func(x preview.Server) bool {
		return x.Allowed && x.Problem == "" && x.State == preview.ServerAuth
	})
	switch {
	case s.Manual:
		out = []string{"connector_configure", "connector_remove"}
		if waiting && kind != nil && kind.Enabled && len(kind.Methods) > 0 {
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
		// An account the connector has already is edited on its own row.
		account := m.service(s.Type)
		switch {
		case waiting:
			out = append(out, "connector_connect")
		case account == nil || !account.Account:
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

func (m *page) connType(id string) *preview.ConnectorType {
	for i := range m.ConnTypes {
		if id != "" && m.ConnTypes[i].ID == id {
			return &m.ConnTypes[i]
		}
	}
	return nil
}

// serviceDetails is the service with its summary over each server's facts,
// Close and Reconnect.
func serviceDetails(c *ui.Context, m *page) {
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
		st := settings.StatusOf(s)
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
						fact("status", t("connector_mcp_status"), func() { statusView(c, settings.StatusOf(&one), "", false) })
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
	}).Width(settings.DialogWidth(c, 560)).Label(s.Name)
	if !open {
		m.connDetails = ""
	}
}
