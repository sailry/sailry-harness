package desktop

import (
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

var testCatalog = i18n.New(Locales)

// Connector statuses, menus and the MCP check.
func TestConnectors(t *testing.T) {
	m := pageFor(settings.NewModel(preview.SettingsData()))
	gh, linear, notion, fs := m.service("github"), m.service("linear"), m.service("notion"), m.service("mcp-filesystem")
	if st := settings.StatusOf(gh); st.Tools != 38 || st.Problem != "" {
		t.Errorf("github %+v", st)
	}
	if !settings.StatusOf(linear).Connecting || settings.StatusOf(notion).Problem != "connector_not_connected" || !settings.StatusOf(fs).Failed {
		t.Error("statuses")
	}
	if got := m.serviceItems(notion); got[0] != "connector_connect" {
		t.Errorf("notion items %v", got)
	}
	if got := m.serviceItems(fs); got[0] != "connector_configure" || got[len(got)-1] != "connector_details" {
		t.Errorf("manual items %v", got)
	}
	// Each state shows its own status and offers only the actions it has: a
	// server never asked to connect or waiting for credentials is not failed,
	// a failure and an unavailable helper carry their reason, and a row with
	// no action has a disabled menu button.
	t.Run("row states", func(t *testing.T) {
		m := pageFor(settings.NewModel(preview.SettingsData()))
		m.Live = true
		var reconnected []string
		m.ConnectorActions = &settings.ConnectorActions{Reconnect: func(id string) { reconnected = append(reconnected, id) }}
		m.ConnTypes = []preview.ConnectorType{{ID: "hub/hub", Package: "hub", Name: "Hub", Methods: []string{"connector_token"}, Fields: []preview.ConnectorField{{Name: "token", Title: "Token", Secret: true}}, Enabled: true}}
		one := func(id, kind string, x preview.Server) preview.Service {
			x.Name, x.Allowed = "server", true
			return preview.Service{ID: id, Type: kind, Name: id, Servers: []preview.Server{x}}
		}
		m.Services = []preview.Service{
			one("idle", "hub/hub", preview.Server{State: preview.ServerIdle}),
			one("waiting", "hub/hub", preview.Server{State: preview.ServerAuth}),
			one("failed", "hub/hub", preview.Server{State: preview.ServerDown, Error: "dial refused"}),
			one("helper", "", preview.Server{State: preview.ServerIdle, Problem: "connector_unavailable", Error: "computer helper is missing"}),
			one("disabled", "", preview.Server{State: preview.ServerIdle, Problem: "connector_package_disabled"}),
			one("unset", "", preview.Server{State: preview.ServerIdle, Problem: "connector_configuration_required"}),
			one("checking", "", preview.Server{State: preview.ServerStarting}),
			one("up", "", preview.Server{State: preview.ServerUp, Tools: 3}),
			{ID: "bare", Name: "bare"},
		}
		want := []struct {
			status settings.ConnStatus
			label  string
			items  []string
		}{
			{settings.ConnStatus{Problem: "connector_not_connected", Tools: -1}, "connector_not_connected", []string{"connector_add_credentials", "connector_mcp_reconnect", "connector_details"}},
			{settings.ConnStatus{Problem: "connector_not_connected", Tools: -1}, "connector_not_connected", []string{"connector_connect", "connector_details"}},
			{settings.ConnStatus{Failed: true, Reason: "dial refused", Tools: -1}, "connector_mcp_failed", []string{"connector_add_credentials", "connector_mcp_reconnect", "connector_details"}},
			{settings.ConnStatus{Problem: "connector_unavailable", Reason: "computer helper is missing", Tools: -1}, "connector_unavailable", []string{"connector_details"}},
			{settings.ConnStatus{Problem: "connector_package_disabled", Tools: -1}, "connector_package_disabled", []string{"connector_details"}},
			{settings.ConnStatus{Problem: "connector_configuration_required", Tools: -1}, "connector_configuration_required", []string{"connector_details"}},
			{settings.ConnStatus{Connecting: true, Tools: -1}, "connector_mcp_connecting", []string{"connector_details"}},
			{settings.ConnStatus{Tools: 3}, "", []string{"connector_mcp_reconnect", "connector_details"}},
			{settings.ConnStatus{Problem: "connector_not_connected", Tools: -1}, "connector_not_connected", nil},
		}
		for i, x := range want {
			s := &m.Services[i]
			if got := settings.StatusOf(s); got != x.status {
				t.Errorf("%s status %+v, want %+v", s.ID, got, x.status)
			}
			if got := m.serviceItems(s); !slices.Equal(got, x.items) {
				t.Errorf("%s items %v, want %v", s.ID, got, x.items)
			}
		}
		tt := ui.NewTesterWith(func(c *ui.Context) {
			i18n.Bind(c, testCatalog)
			m.render(c)
		}, 1000, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		tt.Advance(time.Second)
		texts := tt.Texts()
		count := func(key string) (n int) {
			for _, text := range texts {
				if text == testCatalog.In("en", key) {
					n++
				}
			}
			return n
		}
		// The failed row's dot is named as its text is.
		for key, n := range map[string]int{"connector_not_connected": 3, "connector_mcp_failed": 2, "connector_unavailable": 1, "connector_package_disabled": 1, "connector_configuration_required": 1, "connector_mcp_connecting": 1} {
			if got := count(key); got != n {
				t.Errorf("%d rows show %q, want %d: %q", got, testCatalog.In("en", key), n, texts)
			}
		}
		if !slices.Contains(texts, testCatalog.In("en", "connector_mcp_tools", "n", 3)) {
			t.Errorf("the connected row does not show its tools: %q", texts)
		}
		menus := func() (found []ui.AccessInfo) {
			for _, x := range tt.AccessAll() {
				if x.Name == testCatalog.In("en", "more") {
					found = append(found, x)
				}
			}
			return found
		}
		buttons := menus()
		if len(buttons) != len(want) {
			t.Fatalf("%d menu buttons for %d rows", len(buttons), len(want))
		}
		for i, b := range buttons {
			if b.Disabled != (want[i].items == nil) {
				t.Errorf("%s menu button disabled %v with items %v", m.Services[i].ID, b.Disabled, want[i].items)
			}
		}
		// The reason of a failure and of an unavailable helper is reachable
		// from the row: resting on the status shows it.
		for label, reason := range map[string]string{"connector_mcp_failed": "dial refused", "connector_unavailable": "computer helper is missing"} {
			box, ok := tt.Find(testCatalog.In("en", label))
			if !ok {
				t.Fatalf("no %q status", label)
			}
			tt.Move(box.X+box.W/2, box.Y+box.H/2)
			tt.Frame()
			tt.Advance(2 * time.Second)
			if !tt.HasText(reason) {
				t.Errorf("resting on %q does not show %q: %q", testCatalog.In("en", label), reason, tt.Texts())
			}
		}
		tt.Move(1, 1)
		tt.Advance(time.Second)
		// A menu lists its row's items alone, and each does its action.
		choose := func(row int, key string) {
			t.Helper()
			b := menus()[row].Bounds
			tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
			tt.Frame()
			tt.Advance(time.Second)
			var listed []string
			var target *ui.AccessInfo
			for _, x := range tt.AccessAll() {
				if x.Role != ui.RoleMenuItem {
					continue
				}
				listed = append(listed, x.Name)
				if x.Name == testCatalog.In("en", key) {
					target = &x
				}
			}
			var items []string
			for _, k := range want[row].items {
				items = append(items, testCatalog.In("en", k))
			}
			if !slices.Equal(listed, items) || target == nil {
				t.Fatalf("%s menu lists %q, want %q", m.Services[row].ID, listed, items)
			}
			tt.ClickAt(target.Bounds.X+target.Bounds.W/2, target.Bounds.Y+target.Bounds.H/2)
			tt.Frame()
			tt.Advance(time.Second)
		}
		choose(2, "connector_mcp_reconnect")
		if !slices.Equal(reconnected, []string{"failed"}) {
			t.Errorf("Reconnect asked for %v", reconnected)
		}
		choose(2, "connector_details")
		if m.connDetails != "failed" || !slices.Contains(tt.Texts(), "dial refused") {
			t.Errorf("Details opened %q: %q", m.connDetails, tt.Texts())
		}
		m.connDetails = ""
		tt.Frame()
		tt.Advance(time.Second)
		choose(0, "connector_add_credentials")
		if e := m.connector; !e.open || e.kind == nil || e.kind.ID != "hub/hub" || e.service != "" {
			t.Errorf("Add credentials opened %+v", e)
		}
		if _, ok := tt.AccessByRole(ui.RoleTextField, "Token"); !ok {
			t.Errorf("the editor does not show the connector's fields: %q", tt.Texts())
		}
		m.connector = connectorEditor{}
		tt.Frame()
		tt.Advance(time.Second)
		choose(1, "connector_connect")
		if e := m.connector; !e.open || e.kind == nil || e.kind.ID != "hub/hub" {
			t.Errorf("Connect opened %+v", e)
		}
	})
	e := mcpEditor{name: "Bad Name", command: "npx"}
	if e.Check() != "mcp_invalid" {
		t.Error("bad name passed")
	}
	e.name = "files"
	if e.Check() != "" {
		t.Errorf("good server: %q", e.Check())
	}
	e.transport, e.url = 1, "ftp://x"
	if e.Check() != "mcp_invalid" {
		t.Error("bad URL passed")
	}
	// A row that moves into a removed row's place keeps its own text state:
	// undo there does not bring back the removed row's earlier text.
	t.Run("removed argument row", func(t *testing.T) {
		m := pageFor(settings.NewModel(preview.SettingsData()))
		m.mcp = mcpEditor{open: true, name: "files", command: "npx", args: []string{"", ""}}
		undoAfterRemove(t, func(c *ui.Context) { mcpDialog(c, m) }, "mcp_argument", "mcp_remove_argument", 900)
		if !slices.Equal(m.mcp.args, []string{"second"}) {
			t.Errorf("undo in the row that moved up left arguments %q", m.mcp.args)
		}
	})
	t.Run("removed variable row", func(t *testing.T) {
		m := pageFor(settings.NewModel(preview.SettingsData()))
		m.mcp = mcpEditor{open: true, name: "files", command: "npx", slots: []preview.EnvVar{{}, {}}}
		undoAfterRemove(t, func(c *ui.Context) { mcpDialog(c, m) }, "mcp_slot_value", "mcp_remove_slot", 900)
		if len(m.mcp.slots) != 1 || m.mcp.slots[0].Value != "second" {
			t.Errorf("undo in the row that moved up left variables %+v", m.mcp.slots)
		}
	})
	// A save is asked for once while it runs, and its completion closes the
	// editor that asked, not one opened since.
	t.Run("late save", func(t *testing.T) {
		m := pageFor(settings.NewModel(preview.SettingsData()))
		m.Live = true
		var done []func(bool)
		m.ConnectorActions = &settings.ConnectorActions{SaveMCP: func(_ settings.MCPDraft, completed func(bool)) { done = append(done, completed) }}
		m.mcp = mcpEditor{open: true, name: "files", command: "npx"}
		tt := ui.NewTesterWith(func(c *ui.Context) {
			i18n.Bind(c, testCatalog)
			mcpDialog(c, m)
		}, 900, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		tt.Advance(time.Second)
		save, ok := tt.AccessByRole(ui.RoleButton, testCatalog.In("en", "settings_save"))
		if !ok {
			t.Fatalf("no Save: %q", tt.Texts())
		}
		for range 2 {
			tt.ClickAt(save.Bounds.X+save.Bounds.W/2, save.Bounds.Y+save.Bounds.H/2)
			tt.Frame()
		}
		if len(done) != 1 {
			t.Fatalf("%d saves for one server", len(done))
		}
		m.mcp = mcpEditor{open: true, name: "other"}
		done[0](true)
		if !m.mcp.open {
			t.Error("an earlier server's save closed the editor opened since")
		}
		m.mcp = mcpEditor{open: true, name: "files", command: "npx"}
		tt.Frame()
		tt.ClickAt(save.Bounds.X+save.Bounds.W/2, save.Bounds.Y+save.Bounds.H/2)
		tt.Frame()
		if len(done) != 2 {
			t.Fatalf("%d saves after the second server's", len(done))
		}
		if done[1](false); !m.mcp.open || m.mcp.request != nil {
			t.Error("a failed save closed the editor or left it waiting")
		}
		completed := m.connector.saving()
		m.connector = connectorEditor{open: true}
		if completed(true); !m.connector.open {
			t.Error("an earlier connector's save closed the editor opened since")
		}
		completed = m.connector.saving()
		if completed(true); m.connector.open || m.connector.pending {
			t.Errorf("a saved connector left its editor %+v", m.connector)
		}
	})
	en := func(key string, a ...any) string { return testCatalog.In("en", key, a...) }
	testPage := func(m *page) (*ui.Tester, func()) {
		tt := ui.NewTesterWith(func(c *ui.Context) {
			i18n.Bind(c, testCatalog)
			m.render(c)
		}, 1000, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		settle := func() {
			tt.Frame()
			tt.Advance(time.Second)
		}
		settle()
		return tt, settle
	}
	// button is the last button of that name: a dialog's own comes after the
	// page's, and its frame's close after its footer's.
	button := func(t *testing.T, tt *ui.Tester, name string) (found ui.AccessInfo) {
		t.Helper()
		ok := false
		for _, x := range tt.AccessAll() {
			if x.Role == ui.RoleButton && x.Name == name {
				found, ok = x, true
			}
		}
		if !ok {
			t.Fatalf("no %q button: %q", name, tt.Texts())
		}
		return found
	}
	press := func(tt *ui.Tester, b ui.AccessInfo) {
		tt.ClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
		tt.Frame()
		tt.Advance(time.Second)
	}
	manual := func() (*page, *[]string, *[]settings.MCPDraft, *[]func(bool)) {
		m := pageFor(settings.NewModel(preview.SettingsData()))
		m.Live = true
		var asked []string
		var drafts []settings.MCPDraft
		var done []func(bool)
		m.ConnectorActions = &settings.ConnectorActions{
			ConfigureMCP: func(id string) { asked = append(asked, id) },
			SaveMCP: func(draft settings.MCPDraft, completed func(bool)) {
				drafts, done = append(drafts, draft), append(done, completed)
			},
		}
		// The live row of a server added by hand: its definition and slot
		// names, with nothing about which slot holds a credential.
		m.Services = []preview.Service{{ID: "mcp:mcp-files", Name: "files", Revision: 4, Manual: true, Servers: []preview.Server{{
			Name: "server", Allowed: true, Command: "npx", Args: []string{"-y", "files"}, Env: []preview.EnvVar{{Key: "TOKEN"}, {Key: "REGION"}}}}}}
		return m, &asked, &drafts, &done
	}
	saved := func(m *page) settings.MCPConfiguration {
		return settings.MCPConfiguration{Service: m.Services[0], Saved: []string{"TOKEN"}}
	}
	// Configure reads the saved server before its editor can save: the draft
	// is the server's own, and a saved credential is kept unless typed over
	// or cleared, never replaced by an empty value.
	t.Run("configure loads the saved server", func(t *testing.T) {
		m, asked, drafts, done := manual()
		tt, settle := testPage(m)
		m.serviceAction(nil, &m.Services[0], "connector_configure")
		settle()
		if !slices.Equal(*asked, []string{"mcp:mcp-files"}) || !m.mcp.open || !m.mcp.loading {
			t.Fatalf("Configure asked for %v and opened %+v", *asked, m.mcp)
		}
		if save := button(t, tt, en("settings_save")); !save.Disabled {
			t.Error("Save is available before the configuration is read")
		}
		press(tt, button(t, tt, en("settings_save")))
		if len(*drafts) != 0 {
			t.Fatal("an unread server was saved")
		}
		m.Model.OpenMCPConfiguration(settings.MCPConfiguration{Service: preview.Service{ID: "mcp:other", Servers: []preview.Server{{Command: "other"}}}})
		if m.mcp.command != "npx" || !m.mcp.loading {
			t.Errorf("another service's configuration filled the editor: %+v", m.mcp)
		}
		m.Model.OpenMCPConfiguration(saved(m))
		settle()
		e := &m.mcp
		if e.loading || e.name != "files" || e.command != "npx" || !slices.Equal(e.args, []string{"-y", "files"}) || len(e.slots) != 2 || e.revision != 4 {
			t.Fatalf("the editor does not hold the saved server: %+v", *e)
		}
		// The first slot's value field: its placeholder says a credential is
		// saved under it.
		placeholder := func() string {
			value, _ := tt.AccessByRole(ui.RoleTextField, en("mcp_slot_value"))
			return value.Placeholder
		}
		if placeholder() != en("mcp_secret_saved") || !e.kept(0) || e.kept(1) {
			t.Errorf("saved slots: kept %v %v, placeholder %q", e.kept(0), e.kept(1), placeholder())
		}
		if tt.HasText(en("mcp_secret_saved")) {
			t.Errorf("the saved credential is noted beside its field: %q", tt.Texts())
		}
		if name, ok := tt.AccessByRole(ui.RoleTextField, en("settings_name")); !ok || !name.Disabled {
			t.Errorf("a saved server's name can be edited: %+v", name)
		}
		press(tt, button(t, tt, en("settings_save")))
		if len(*drafts) != 1 {
			t.Fatalf("%d saves", len(*drafts))
		}
		d := (*drafts)[0]
		if d.Service != "mcp:mcp-files" || d.Revision != 4 || d.Server.Command != "npx" || !slices.Equal(d.Keep, []string{"TOKEN"}) ||
			!slices.Equal(d.Server.Env, []preview.EnvVar{{Key: "TOKEN"}, {Key: "REGION"}}) {
			t.Errorf("the untouched server saves as %+v", d)
		}
		(*done)[0](false)
		settle()
		// A typed value replaces the saved credential; another kind of
		// transport does not carry it over.
		e.transport, e.url = 1, "https://example.test/mcp"
		settle()
		if e.kept(0) || placeholder() == en("mcp_secret_saved") || len(e.draft().Keep) != 0 {
			t.Error("a credential saved for a command is kept for a URL")
		}
		e.transport = 0
		settle()
		if !e.kept(0) || placeholder() != en("mcp_secret_saved") {
			t.Error("returning to a command does not keep its credential")
		}
		e.slots[0].Value = "typed"
		settle()
		if d := e.draft(); len(d.Keep) != 0 || d.Server.Env[0].Value != "typed" || placeholder() == en("mcp_secret_saved") {
			t.Errorf("a typed credential saves as %+v", d)
		}
		// A value is masked until shown: the last slot's here.
		e.slots[1].Value = "plain"
		settle()
		if tt.HasText("typed") || tt.HasText("plain") {
			t.Errorf("a credential shows while masked: %q", tt.Texts())
		}
		press(tt, button(t, tt, en("mcp_show_secret")))
		if value, ok := tt.AccessByRole(ui.RoleTextField, en("mcp_slot_value")); !e.marks[1].shown || e.marks[0].shown || !ok {
			t.Errorf("Show credential showed %+v: %+v", e.marks, value)
		}
		press(tt, button(t, tt, en("mcp_hide_secret")))
		if e.marks[1].shown {
			t.Error("Hide credential left it shown")
		}
		// The editor closed while it waited is not reopened by its answer.
		m.serviceAction(nil, &m.Services[0], "connector_configure")
		m.mcp.open = false
		m.Model.OpenMCPConfiguration(saved(m))
		if m.mcp.open {
			t.Error("a late configuration reopened the closed editor")
		}
		m.serviceAction(nil, &m.Services[0], "connector_configure")
		m.CloseManagedNodeDialogs()
		m.Model.OpenMCPConfiguration(saved(m))
		if m.mcp.open || m.mcp.loading || m.mcp.original != "" {
			t.Error("the previous Node's editor survived the Node change")
		}
	})
	// A failed read leaves the editor locked with Reload and the reason
	// behind Details; an uncertain save locks the draft and Save retries it.
	t.Run("reload, diagnostics and retry", func(t *testing.T) {
		m, asked, drafts, done := manual()
		tt, settle := testPage(m)
		m.serviceAction(nil, &m.Services[0], "connector_configure")
		m.Model.FailMCPConfiguration("mcp:mcp-files", "settings database is locked")
		settle()
		if save := button(t, tt, en("settings_save")); !save.Disabled || tt.HasText("settings database is locked") {
			t.Errorf("after a failed read Save disabled is %v: %q", save.Disabled, tt.Texts())
		}
		press(tt, button(t, tt, en("plugins_settings_diagnostics")))
		if !tt.HasText("settings database is locked") {
			t.Errorf("Details does not show the reason: %q", tt.Texts())
		}
		press(tt, button(t, tt, en("plugins_settings_diagnostics")))
		if tt.HasText("settings database is locked") {
			t.Error("Details does not hide the reason again")
		}
		press(tt, button(t, tt, en("plugins_settings_reload")))
		if len(*asked) != 2 || !m.mcp.loading {
			t.Fatalf("Reload asked %d times, loading %v", len(*asked), m.mcp.loading)
		}
		m.Model.OpenMCPConfiguration(saved(m))
		settle()
		if _, ok := tt.AccessByRole(ui.RoleButton, en("plugins_settings_reload")); ok || m.mcp.unread {
			t.Error("Reload stays after the configuration was read")
		}
		press(tt, button(t, tt, en("settings_save")))
		if len(*drafts) != 1 || !button(t, tt, en("settings_save")).Disabled {
			t.Fatalf("%d saves; Save is pressable while one runs", len(*drafts))
		}
		(*drafts)[0].Failed(true, "connection lost")
		(*done)[0](false)
		settle()
		retry := button(t, tt, en("plugins_retry"))
		if _, ok := tt.AccessByRole(ui.RoleButton, en("settings_save")); ok || retry.Disabled {
			t.Fatalf("after an uncertain save: %q", tt.Texts())
		}
		if command, ok := tt.AccessByRole(ui.RoleTextField, en("mcp_command")); !ok || !command.Disabled {
			t.Error("the draft of an uncertain save can change")
		}
		press(tt, retry)
		if len(*drafts) != 2 || (*drafts)[1].Server.Command != (*drafts)[0].Server.Command || !slices.Equal((*drafts)[1].Keep, (*drafts)[0].Keep) {
			t.Fatalf("Retry sent %+v", *drafts)
		}
		(*drafts)[1].Failed(false, "invalid command")
		(*done)[1](false)
		settle()
		if _, ok := tt.AccessByRole(ui.RoleButton, en("plugins_retry")); ok || m.mcp.uncertain || !m.mcp.open {
			t.Error("a failed retry left the editor uncertain or closed")
		}
		press(tt, button(t, tt, en("settings_save")))
		if (*done)[2](true); m.mcp.open {
			t.Error("a saved server left its editor open")
		}
	})
	// The page: the filter's No authentication, the row's right-click menu,
	// a busy row, and the details dialog's Close and Configure.
	t.Run("page", func(t *testing.T) {
		m, asked, _, _ := manual()
		busy := false
		var reconnected []string
		m.ConnectorActions.Busy = func(id string) bool { return busy && id == "mcp:mcp-files" }
		m.ConnectorActions.Reconnect = func(id string) { reconnected = append(reconnected, id) }
		m.ConnTypes = []preview.ConnectorType{{ID: "hub/hub", Package: "hub", Name: "Hub", Methods: []string{"connector_token"}, Enabled: true}}
		m.Services = append(m.Services,
			preview.Service{ID: "hub/hub", Type: "hub/hub", Name: "Hub", Method: "connector_token", Account: true, AccountOn: true, State: preview.Connected},
			preview.Service{ID: "mcp:pair", Name: "pair", Servers: []preview.Server{
				{Name: "first", Allowed: true, State: preview.ServerUp, Tools: 2, Updated: "2026-11-11 10:00:00"},
				{Name: "second", Allowed: true, State: preview.ServerDown, Error: "dial refused", Updated: "2026-11-11 10:05:00"}}})
		tt, settle := testPage(m)
		m.method = slices.Index(connectorMethods, "connector_no_auth")
		settle()
		if tt.HasText("Hub") || !tt.HasText("files") || !tt.HasText("pair") {
			t.Errorf("No authentication shows %q", tt.Texts())
		}
		m.method = slices.Index(connectorMethods, "connector_token")
		settle()
		if !tt.HasText("Hub") || tt.HasText("files") {
			t.Errorf("Token shows %q", tt.Texts())
		}
		m.method = 0
		settle()
		// Resting on a status shows its reason and when the server last
		// reported.
		box, ok := tt.Find(en("connector_mcp_failed"))
		if !ok {
			t.Fatalf("no failed status: %q", tt.Texts())
		}
		tt.Move(box.X+box.W/2, box.Y+box.H/2)
		tt.Frame()
		tt.Advance(2 * time.Second)
		updated := en("connector_mcp_updated", "time", "2026-11-11 10:05:00")
		if texts := strings.Join(tt.Texts(), "\n"); !strings.Contains(texts, "dial refused") || !strings.Contains(texts, updated) {
			t.Errorf("the status does not show its reason and %q: %q", updated, tt.Texts())
		}
		tt.Move(1, 1)
		settle()
		if err := tt.RightClick("files"); err != nil {
			t.Fatal(err)
		}
		want := []string{en("connector_configure"), en("connector_remove"), en("connector_mcp_reconnect"), en("connector_details")}
		if got := tt.Menu(); !slices.Equal(got, want) {
			t.Fatalf("the row's context menu is %q, want %q", got, want)
		}
		if err := tt.ChooseMenuItem(en("connector_mcp_reconnect")); err != nil {
			t.Fatal(err)
		}
		settle()
		if !slices.Equal(reconnected, []string{"mcp:mcp-files"}) {
			t.Errorf("the context menu's Reconnect asked for %v", reconnected)
		}
		// A command of the row under way holds its menu and its switch.
		busy = true
		settle()
		held := 0
		for _, x := range tt.AccessAll() {
			if x.Disabled && (x.Name == en("more") || x.Name == en("connector_server_allowed")+" files") {
				held++
			}
		}
		if held != 2 {
			t.Errorf("a busy row holds %d of its menu and switch", held)
		}
		busy = false
		// Details: a status for each of several servers, Close, and
		// Configure for a plugin's own servers.
		m.connDetails = "mcp:pair"
		settle()
		if !tt.HasText(en("connector_mcp_status")) || !tt.HasText(en("connector_mcp_tools", "n", 2)) {
			t.Errorf("details of two servers: %q", tt.Texts())
		}
		var footer ui.AccessInfo
		for _, x := range tt.AccessAll() {
			// The footer's Close is below the frame's.
			if x.Role == ui.RoleButton && x.Name == en("close") && x.Bounds.Y > footer.Bounds.Y {
				footer = x
			}
		}
		press(tt, footer)
		if m.connDetails != "" {
			t.Fatal("the details' Close button does not close them")
		}
		m.connDetails = "mcp:mcp-files"
		settle()
		if tt.HasText(en("connector_mcp_status")) {
			t.Error("one server's details repeat its status")
		}
		for _, x := range tt.AccessAll() {
			if x.Role == ui.RoleButton && x.Name == en("connector_configure") {
				t.Error("a server added by hand has Configure in its details")
			}
		}
		m.connDetails = "mcp:pair"
		settle()
		press(tt, button(t, tt, en("connector_configure")))
		if !slices.Equal(*asked, []string{"mcp:pair"}) || m.connDetails != "" || m.mcp.open {
			t.Fatalf("Configure asked for %v, details %q, editor open %v", *asked, m.connDetails, m.mcp.open)
		}
		m.Model.OpenMCPConfiguration(settings.MCPConfiguration{Declared: true, Saved: []string{"KEY"},
			Service: preview.Service{ID: "mcp:pair", Name: "first", Revision: 2, Servers: []preview.Server{{Name: "first", Transport: 1, URL: "https://example.test/mcp", Env: []preview.EnvVar{{Key: "KEY"}}}}}})
		settle()
		if e := m.mcp; !e.open || !e.declared || e.url != "https://example.test/mcp" || !e.kept(0) {
			t.Errorf("the plugin's server opened as %+v", e)
		}
	})
	// The connector's editor: an existing OAuth connection saves its fields
	// without authorizing again, the primary button waits for the required
	// fields and for a save under way, and a CLI without an account retries.
	t.Run("connector editor", func(t *testing.T) {
		m := pageFor(settings.NewModel(preview.SettingsData()))
		m.Live = true
		var drafts []settings.ConnectorDraft
		var done []func(bool)
		inspected := 0
		m.ConnectorActions = &settings.ConnectorActions{
			Save: func(draft settings.ConnectorDraft, completed func(bool)) {
				drafts, done = append(drafts, draft), append(done, completed)
			},
			InspectCLI: func(_ string, completed func([]string)) {
				inspected++
				if inspected > 1 {
					completed([]string{"octocat"})
				} else {
					completed(nil)
				}
			},
		}
		m.ConnTypes = []preview.ConnectorType{
			{ID: "hub/hub", Package: "hub", Name: "Hub", Methods: []string{"connector_oauth"}, Enabled: true, Fields: []preview.ConnectorField{{Name: "host", Title: "Host"}}},
			{ID: "cli/cli", Package: "cli", Name: "Tool", Methods: []string{"connector_cli"}, Enabled: true},
			{ID: "key/key", Package: "key", Name: "Keyed", Methods: []string{"connector_token"}, Enabled: true, Fields: []preview.ConnectorField{{Name: "token", Title: "Token", Secret: true}}},
		}
		m.Services = []preview.Service{{ID: "hub/hub", Type: "hub/hub", Name: "Hub", Method: "connector_oauth", Account: true, AccountOn: true, State: preview.Connected, Revision: 7, Fields: map[string]string{"host": "hub.test"}}}
		tt, settle := testPage(m)
		m.editService(&m.Services[0])
		settle()
		m.connector.fields["host"] = ""
		settle()
		if save := button(t, tt, en("settings_save")); !save.Disabled {
			t.Error("Save is available without the required field")
		}
		m.connector.fields["host"] = "hub.example"
		settle()
		press(tt, button(t, tt, en("settings_save")))
		if len(drafts) != 1 || !drafts[0].Update || drafts[0].Service != "hub/hub" || drafts[0].ExpectedRevision != 7 || drafts[0].Fields["host"] != "hub.example" {
			t.Fatalf("an OAuth connection's changed field saves as %+v", drafts)
		}
		// While it runs the button is held and cannot fire again.
		if save := button(t, tt, en("settings_save")); !save.Disabled {
			t.Error("Save is pressable while the save runs")
		}
		press(tt, button(t, tt, en("settings_save")))
		if len(drafts) != 1 {
			t.Fatalf("%d saves for one press", len(drafts))
		}
		done[0](false)
		settle()
		press(tt, button(t, tt, en("connector_reauthorize")))
		press(tt, button(t, tt, en("connector_reauthorize")))
		if len(drafts) != 2 || drafts[1].Update {
			t.Fatalf("Reauthorize sent %+v", drafts)
		}
		// A new token connector waits for its token.
		m.connector = connectorEditor{open: true, kind: &m.ConnTypes[2], fields: map[string]string{}, direct: true}
		settle()
		if save := button(t, tt, en("settings_save")); !save.Disabled {
			t.Error("Save is available without the token")
		}
		m.connector.fields["token"] = "secret"
		settle()
		if save := button(t, tt, en("settings_save")); save.Disabled {
			t.Error("Save is held with the token typed")
		}
		// No local account: nothing to bind until Retry finds one.
		m.connector = connectorEditor{open: true, kind: &m.ConnTypes[1], fields: map[string]string{}, direct: true}
		settle()
		if bind := button(t, tt, en("connector_bind")); !bind.Disabled || inspected != 1 {
			t.Fatalf("Bind disabled %v after %d inspections", bind.Disabled, inspected)
		}
		press(tt, button(t, tt, en("chat_retry")))
		settle()
		if bind := button(t, tt, en("connector_bind")); bind.Disabled || inspected != 2 || !tt.HasText("octocat") {
			t.Errorf("after Retry: Bind disabled %v, %d inspections: %q", bind.Disabled, inspected, tt.Texts())
		}
	})
	// A connector with several methods: a new connection starts on the CLI,
	// a saved one shows the same methods on its own, each method shows only
	// its own fields, and another method authorizes the connection again.
	t.Run("connector methods", func(t *testing.T) {
		m := pageFor(settings.NewModel(preview.SettingsData()))
		m.Live = true
		var drafts []settings.ConnectorDraft
		var done []func(bool)
		m.ConnectorActions = &settings.ConnectorActions{
			Save: func(draft settings.ConnectorDraft, completed func(bool)) {
				drafts, done = append(drafts, draft), append(done, completed)
			},
			InspectCLI: func(_ string, completed func([]string)) { completed([]string{"other", "octocat"}) },
		}
		m.ConnTypes = []preview.ConnectorType{{ID: "hub/hub", Package: "hub", Name: "Hub", Methods: []string{"connector_token", "connector_cli"}, Enabled: true,
			Fields: []preview.ConnectorField{{Name: "token", Title: "Token", Secret: true, Methods: []string{"connector_token"}}}}}
		m.Services = nil
		tt, settle := testPage(m)
		m.addCredentials(&preview.Service{Type: "hub/hub"})
		settle()
		if _, ok := tt.AccessByRole(ui.RoleTextField, "Token"); ok || m.connector.method != 1 {
			t.Fatalf("a new connection opens on method %d with the token field %v", m.connector.method, ok)
		}
		m.Services = []preview.Service{{ID: "hub/hub", Type: "hub/hub", Name: "Hub", Method: "connector_cli", Identity: "octocat", Account: true, AccountOn: true, State: preview.Connected, Revision: 3}}
		m.editService(&m.Services[0])
		settle()
		if _, ok := tt.AccessByRole(ui.RoleTextField, "Token"); ok || m.connector.method != 1 || m.connector.account != 1 {
			t.Fatalf("the CLI connection opens on method %d, account %d, token field %v", m.connector.method, m.connector.account, ok)
		}
		if _, ok := tt.AccessByRole(ui.RoleButton, en("settings_save")); ok {
			t.Error("the CLI connection offers Save beside Bind")
		}
		press(tt, button(t, tt, en("connector_bind")))
		if len(drafts) != 1 || drafts[0].Method != "connector_cli" || drafts[0].Service != "hub/hub" || drafts[0].CLIAccount != "octocat" || drafts[0].Update {
			t.Fatalf("Bind sent %+v", drafts)
		}
		done[0](false)
		m.connector.method = 0
		settle()
		if save := button(t, tt, en("settings_save")); !save.Disabled {
			t.Error("switching a CLI connection to a token keeps a token it never had")
		}
		m.connector.fields["token"] = "secret"
		settle()
		press(tt, button(t, tt, en("settings_save")))
		if len(drafts) != 2 || drafts[1].Method != "connector_token" || drafts[1].Service != "hub/hub" || drafts[1].ExpectedRevision != 3 || drafts[1].Fields["token"] != "secret" {
			t.Fatalf("switching to a token sent %+v", drafts)
		}
		done[1](false)
		m.connector.method = 1
		settle()
		m.connector.account = 0
		settle()
		press(tt, button(t, tt, en("connector_bind")))
		if len(drafts) != 3 || drafts[2].CLIAccount != "other" || len(drafts[2].Fields) != 0 {
			t.Fatal("CLI binding did not carry the selected account alone")
		}
		done[2](false)
		// A saved CLI connection with a field of its own saves the field
		// without binding again; Bind asks for the account apart.
		m.ConnTypes[0].Fields = append(m.ConnTypes[0].Fields, preview.ConnectorField{Name: "host", Title: "Host"})
		m.Services[0].Fields = map[string]string{"host": "hub.test"}
		m.editService(&m.Services[0])
		settle()
		if _, ok := tt.AccessByRole(ui.RoleTextField, "Host"); !ok || tt.HasText(en("connector_choose_account")) {
			t.Fatalf("the CLI connection's field editor: %q", tt.Texts())
		}
		m.connector.fields["host"] = "hub.example"
		settle()
		press(tt, button(t, tt, en("settings_save")))
		if len(drafts) != 4 || !drafts[3].Update || drafts[3].Method != "connector_cli" || drafts[3].CLIAccount != "" || drafts[3].Fields["host"] != "hub.example" {
			t.Fatalf("the CLI connection's field saved as %+v", drafts)
		}
		done[3](false)
		press(tt, button(t, tt, en("connector_bind")))
		settle()
		if !tt.HasText(en("connector_choose_account")) || !tt.HasText("octocat") {
			t.Errorf("Bind does not ask for the account: %q", tt.Texts())
		}
	})
}

// undoAfterRemove types "first" and "second" into the two rows of a dialog's
// list, removes the first and undoes in the row that moved up, which must
// keep its own text: the caller checks the list holds "second" alone. field
// and remove are the i18n keys of a row's input and remove button.
func undoAfterRemove(t *testing.T, draw func(*ui.Context), field, remove string, height int) {
	t.Helper()
	tt := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, testCatalog)
		draw(c)
	}, 900, height, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	tt.Advance(time.Second)
	named := func(role ui.Role, key string) (found []ui.Rect) {
		for _, x := range tt.AccessAll() {
			if x.Role == role && x.Name == testCatalog.In("en", key) {
				found = append(found, x.Bounds)
			}
		}
		return found
	}
	at := func(r ui.Rect) { tt.ClickAt(r.X+r.W/2, r.Y+r.H/2) }
	inputs := named(ui.RoleTextField, field)
	if len(inputs) != 2 {
		t.Fatalf("%d %s inputs: %q", len(inputs), field, tt.Texts())
	}
	at(inputs[0])
	tt.Type("first")
	at(inputs[1])
	tt.Type("second")
	if buttons := named(ui.RoleButton, remove); len(buttons) > 0 {
		at(buttons[0])
	} else {
		// A list row is a button that takes its title for its name, and its
		// remove action is the 28 DIP button at its end, 16 from the edge.
		row, ok := tt.AccessByRole(ui.RoleButton, "first")
		if !ok {
			t.Fatalf("no %s button and no row named by its text", remove)
		}
		tt.ClickAt(row.Bounds.X+row.Bounds.W-30, row.Bounds.Y+row.Bounds.H/2)
	}
	tt.Frame()
	inputs = named(ui.RoleTextField, field)
	if len(inputs) != 1 {
		t.Fatalf("%d %s inputs after removing one", len(inputs), field)
	}
	at(inputs[0])
	tt.Key(ui.Cmd, ui.KeyZ)
	tt.Frame()
}

// The CLI method's account list shows its skeleton row while the host
// looks at its accounts, then the accounts.
func TestAccountSkeleton(t *testing.T) {
	m := pageFor(settings.NewModel(preview.SettingsData()))
	m.connector = connectorEditor{open: true, kind: &m.ConnTypes[0], method: 2, fields: map[string]string{}}
	tt := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, testCatalog)
		connectorDialog(c, m)
	}, 900, 700, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	busy := func() bool {
		for _, x := range tt.AccessAll() {
			if x.Busy {
				return true
			}
		}
		return false
	}
	if !m.connector.accounts.Loading || !busy() || tt.HasText(m.ConnAccounts[0]) {
		t.Fatalf("no account skeleton: %q", tt.Texts())
	}
	tt.Advance(time.Second)
	tt.Frame()
	if busy() || !tt.HasText(m.ConnAccounts[0]) {
		t.Fatalf("accounts not shown: %q", tt.Texts())
	}
}
