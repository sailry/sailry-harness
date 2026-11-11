package settings

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

func TestKeysCapture(t *testing.T) {
	k := NewKeys()
	newChat := Registry[0]
	k.Record(newChat.ID)
	for _, c := range []struct {
		ch   Chord
		want string
	}{
		{Chord{0, ui.KeyN}, "shortcut_invalid"},
		{Chord{ui.Shift, ui.KeyN}, "shortcut_invalid"},
		{Chord{ui.Cmd, ui.KeyK}, "shortcut_conflict"},     // app.search
		{Chord{ui.Cmd, ui.KeyComma}, "shortcut_conflict"}, // fixed Settings
		{Chord{ui.Cmd | ui.Shift, ui.KeyN}, ""},
	} {
		if got := k.Capture(c.ch); got != c.want {
			t.Errorf("%+v: %q, want %q", c.ch, got, c.want)
		}
	}
	if s, ok := k.Bound(newChat); !ok || s.Mods != ui.Cmd|ui.Shift || k.Recording() {
		t.Fatalf("bound %+v %v recording %v", s, ok, k.Recording())
	}
	k.Record(newChat.ID)
	k.Clear()
	if _, ok := k.Bound(newChat); ok {
		t.Fatal("cleared shortcut still bound")
	}
	k.Record(newChat.ID)
	k.Reset()
	if s, ok := k.Bound(newChat); !ok || s.Mods != ui.Cmd || s.Key != ui.KeyN {
		t.Fatalf("reset %+v", s)
	}
	if got := k.Capture(Chord{ui.Cmd, ui.KeyJ}); got != "shortcut_unavailable" {
		t.Errorf("capture while idle: %q", got)
	}
}

func TestDraftChecks(t *testing.T) {
	t.Run("terminal variable", func(t *testing.T) {
		h := TerminalHost{Env: []preview.EnvVar{{Key: "EDITOR", Value: "vi"}}}
		for _, c := range []struct {
			index      int
			key, want  string
			wantLength int
		}{
			{-1, "1X", "terminal_variable_invalid", 1},
			{-1, "EDITOR", "terminal_variable_duplicate", 1},
			{0, " EDITOR ", "", 1},
			{-1, "PATH_2", "", 2},
		} {
			if got := h.SetVariable(c.index, c.key, "v"); got != c.want || len(h.Env) != c.wantLength {
				t.Errorf("%+v: %q %d", c, got, len(h.Env))
			}
		}
	})
	t.Run("hook consent reports the captured Node sandbox capability", func(t *testing.T) {
		m := &Model{HooksSandboxed: func() bool { return false }}
		if got := hookScope(m); got != "hooks_scope_workspace" {
			t.Fatalf("preview scope = %q", got)
		}
		m.Live = true
		if got := hookScope(m); got != "hooks_scope_windows" {
			t.Fatalf("Windows scope = %q", got)
		}
		m.HooksSandboxed = func() bool { return true }
		if got := hookScope(m); got != "hooks_scope_workspace" {
			t.Fatalf("sandbox scope = %q", got)
		}
	})
	e := hookEditor{script: preview.Hook{Command: "make"}, timeout: "x"}
	if e.Check() != "hooks_invalid" {
		t.Error("bad timeout accepted")
	}
	e.timeout = "60001"
	if e.Check() != "hooks_invalid" {
		t.Error("oversized hook timeout accepted")
	}
	e.timeout = "500"
	if e.Check() != "" || e.script.TimeoutMS != 500 {
		t.Errorf("good hook refused: %+v", e.script)
	}
	b := BackupTarget{Endpoint: "https://dav"}
	for _, c := range []struct {
		pass, confirm string
		set           bool
		want          string
	}{
		{"", "", false, "backup_passphrase_required"},
		{"short", "short", false, "backup_passphrase_invalid"},
		{"long enough", "other one", false, "backup_passphrase_mismatch"},
		{"long enough", "long enough", false, ""},
		{"", "", true, ""},
	} {
		b.Passphrase, b.Confirm, b.PassphraseSet = c.pass, c.confirm, c.set
		if got := b.Check(); got != c.want {
			t.Errorf("%+v: %q", c, got)
		}
	}
	b.Tab, b.Bucket = 1, ""
	if b.Check() != "backup_invalid" {
		t.Error("S3 without a bucket accepted")
	}
	// Trusting a host's key leaves the connection pending; canceling the
	// prompt ends it. A network save under way keeps its editor.
	t.Run("connection prompts", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		tt := ui.NewTesterWith(func(c *ui.Context) { connectionDialogs(c, m) }, 900, 700, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		answer := func(key string) (confirmed bool) {
			t.Helper()
			m.ConnectionPending = true
			m.HostTrust = &HostTrust{Host: "example.com:22", Algorithm: "ssh-ed25519", Fingerprint: "SHA256:abc", Confirm: func() { confirmed = true }}
			tt.Frame()
			tt.Advance(time.Second)
			button, ok := tt.AccessByRole(ui.RoleButton, i18n.In("en", key))
			if !ok {
				t.Fatalf("no %s: %q", key, tt.Texts())
			}
			tt.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
			tt.Frame()
			tt.Advance(time.Second)
			return confirmed
		}
		if !answer("ssh_trust") || m.HostTrust != nil || !m.ConnectionPending {
			t.Errorf("trusted: prompt %v, pending %v", m.HostTrust, m.ConnectionPending)
		}
		if answer("settings_cancel") || m.HostTrust != nil || m.ConnectionPending {
			t.Errorf("canceled: prompt %v, pending %v", m.HostTrust, m.ConnectionPending)
		}
		m.NetworkEditor = &NetworkEditor{Pending: true}
		tt.Frame()
		tt.Advance(time.Second)
		tt.Key(0, ui.KeyEscape)
		tt.Frame()
		if m.NetworkEditor == nil {
			t.Fatal("Escape dropped the network editor while it saved")
		}
		m.NetworkEditor.Pending = false
		tt.Frame()
		tt.Key(0, ui.KeyEscape)
		tt.Frame()
		if m.NetworkEditor != nil {
			t.Error("Escape kept an idle network editor")
		}
	})
}

// The provider editor's steps and checks.
func TestProviderEditor(t *testing.T) {
	m := NewModel(preview.SettingsData())
	e := &m.provider
	e.add(m)
	e.choose(&m.Brands[0]) // Anthropic: several methods
	if e.step != stepMethod {
		t.Fatalf("several methods went to step %d", e.step)
	}
	e.configure(m.Brands[0].Methods[1]) // API key
	if got := e.Check(); got != "provider_key_required" {
		t.Errorf("no key: %q", got)
	}
	e.key = "sk-preview"
	if got := e.Check(); got != "provider_model_required" {
		t.Errorf("no model: %q", got)
	}
	e.models = []modelDraft{{id: "a", context: "128000"}, {id: "a", context: "128000"}}
	if got := e.Check(); got != "provider_model_duplicate" {
		t.Errorf("duplicate: %q", got)
	}
	e.models[1].id = "b"
	n := len(m.Channels)
	if got := e.save(m); got != "" || len(m.Channels) != n+1 {
		t.Fatalf("save: %q, %d channels", got, len(m.Channels))
	}
	e.back()
	if e.step != stepMethod {
		t.Errorf("back went to %d", e.step)
	}
	e.choose(&m.Brands[2]) // Google: one method
	if e.step != stepConfigure {
		t.Errorf("one method went to %d", e.step)
	}
	e.back()
	if e.step != stepBrand {
		t.Errorf("back from a single method went to %d", e.step)
	}
	if ProviderStatus(&m.Channels[2]) != "provider_status_missing_key" || ProviderStatus(&m.Channels[5]) != "provider_status_signed_out" {
		t.Error("statuses")
	}
	t.Run("removed model row", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		m.provider = providerEditor{open: true, step: stepConfigure, name: "Provider", keySet: true,
			models: []modelDraft{{open: true}, {open: true}}}
		undoAfterRemove(t, func(c *ui.Context) { providerEditorDialog(c, m) }, "provider_model_id", "settings_delete")
		if len(m.provider.models) != 1 || m.provider.models[0].id != "second" {
			t.Errorf("undo in the row that moved up left models %+v", m.provider.models)
		}
	})
	// Removing an effort listed before the default leaves the default.
	t.Run("removed effort", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		m.provider = providerEditor{open: true, step: stepConfigure, name: "Provider", keySet: true,
			models: []modelDraft{{open: true, id: "model", efforts: []string{"low", "medium", "high"}, def: 1}}}
		tt := ui.NewTesterWith(func(c *ui.Context) { providerEditorDialog(c, m) }, 900, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		tt.Advance(time.Second)
		for _, x := range tt.AccessAll() {
			if x.Role == ui.RoleButton && x.Name == i18n.In("en", "settings_delete") {
				tt.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
				break
			}
		}
		tt.Frame()
		d := m.provider.models[0]
		if !slices.Equal(d.efforts, []string{"medium", "high"}) || d.efforts[d.def] != "medium" {
			t.Errorf("efforts %v, default %d", d.efforts, d.def)
		}
	})
	t.Run("context_limits", func(t *testing.T) {
		for _, value := range []string{"", "0", "-1", "4295095296", "not a limit"} {
			editor := providerEditor{name: "Provider", keySet: true, models: []modelDraft{{id: "model", context: value}}}
			if got := editor.Check(); got != "provider_limits_invalid" {
				t.Fatalf("context %q: %q", value, got)
			}
		}
	})
}

// Connector statuses, menus and the MCP check.
func TestConnectors(t *testing.T) {
	m := NewModel(preview.SettingsData())
	gh, linear, notion, fs := m.service("github"), m.service("linear"), m.service("notion"), m.service("mcp-filesystem")
	if st := StatusOf(gh); st.Tools != 38 || st.Problem != "" {
		t.Errorf("github %+v", st)
	}
	if !StatusOf(linear).Connecting || StatusOf(notion).Problem != "connector_not_connected" || !StatusOf(fs).Failed {
		t.Error("statuses")
	}
	if got := serviceItems(notion); got[0] != "connector_connect" {
		t.Errorf("notion items %v", got)
	}
	if got := serviceItems(fs); got[0] != "connector_configure" || got[len(got)-1] != "connector_details" {
		t.Errorf("manual items %v", got)
	}
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
		m := NewModel(preview.SettingsData())
		m.mcp = mcpEditor{open: true, name: "files", command: "npx", args: []string{"", ""}}
		undoAfterRemove(t, func(c *ui.Context) { mcpDialog(c, m) }, "mcp_argument", "mcp_remove_argument")
		if !slices.Equal(m.mcp.args, []string{"second"}) {
			t.Errorf("undo in the row that moved up left arguments %q", m.mcp.args)
		}
	})
	t.Run("removed variable row", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		m.mcp = mcpEditor{open: true, name: "files", command: "npx", slots: []preview.EnvVar{{}, {}}}
		undoAfterRemove(t, func(c *ui.Context) { mcpDialog(c, m) }, "mcp_slot_value", "mcp_remove_slot")
		if len(m.mcp.slots) != 1 || m.mcp.slots[0].Value != "second" {
			t.Errorf("undo in the row that moved up left variables %+v", m.mcp.slots)
		}
	})
	// A save is asked for once while it runs, and its completion closes the
	// editor that asked, not one opened since.
	t.Run("late save", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		m.Live = true
		var done []func(bool)
		m.ConnectorActions = &ConnectorActions{SaveMCP: func(_ MCPDraft, completed func(bool)) { done = append(done, completed) }}
		m.mcp = mcpEditor{open: true, name: "files", command: "npx"}
		tt := ui.NewTesterWith(func(c *ui.Context) { mcpDialog(c, m) }, 900, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		tt.Advance(time.Second)
		save, ok := tt.AccessByRole(ui.RoleButton, i18n.In("en", "settings_save"))
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
}

// undoAfterRemove types "first" and "second" into the two rows of a dialog's
// list, removes the first and undoes in the row that moved up, which must
// keep its own text: the caller checks the list holds "second" alone. field
// and remove are the i18n keys of a row's input and remove button.
func undoAfterRemove(t *testing.T, draw func(*ui.Context), field, remove string) {
	t.Helper()
	tt := ui.NewTesterWith(draw, 900, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	tt.Advance(time.Second)
	named := func(role ui.Role, key string) (found []ui.Rect) {
		for _, x := range tt.AccessAll() {
			if x.Role == role && x.Name == i18n.In("en", key) {
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

// Team and role checks.
func TestTeams(t *testing.T) {
	m := NewModel(preview.SettingsData())
	if got := m.saveTeam(preview.Team{Name: " "}); got != "team_invalid" {
		t.Errorf("blank team: %q", got)
	}
	if got := m.saveTeam(preview.Team{Name: "study"}); got != "team_name_taken" {
		t.Errorf("taken name: %q", got)
	}
	n := len(m.Teams)
	if got := m.saveTeam(preview.Team{Name: "Docs"}); got != "" || len(m.Teams) != n+1 {
		t.Errorf("new team: %q", got)
	}
	if got := m.saveRole(preview.Role{Name: "X", Key: "Bad Key"}); got != "teams_role_invalid" {
		t.Errorf("bad key: %q", got)
	}
	if got := m.saveRole(preview.Role{Name: "X", Key: "tester"}); got != "role_duplicate" {
		t.Errorf("duplicate key: %q", got)
	}
	// A team's deletion closes the editor it was deleted from, not one
	// opened since for another team.
	t.Run("late removal", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		m.Live = true
		var done func(kit.Outcome)
		m.TeamActions = &TeamActions{RemoveTeam: func(_ preview.Team, completed func(kit.Outcome)) { done = completed }}
		removed, other := m.Teams[0], m.Teams[1]
		m.teamEdit, m.teamRemoving = teamEditor{open: true, team: removed}, removed.ID
		tt := ui.NewTesterWith(func(c *ui.Context) { teams(c, m) }, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		tt.Advance(time.Second)
		var confirms []ui.Rect
		for _, x := range tt.AccessAll() {
			if x.Role == ui.RoleButton && x.Name == i18n.In("en", "settings_delete") {
				confirms = append(confirms, x.Bounds)
			}
		}
		if len(confirms) == 0 {
			t.Fatalf("no confirmation: %q", tt.Texts())
		}
		at := confirms[len(confirms)-1]
		tt.ClickAt(at.X+at.W/2, at.Y+at.H/2)
		tt.Frame()
		if done == nil {
			t.Fatal("the confirmation removed nothing")
		}
		m.teamEdit, m.teamRemoving = teamEditor{open: true, team: other}, other.ID
		if done(kit.Done); !m.teamEdit.open || m.teamRemoving != other.ID {
			t.Errorf("another team's editor %v, confirmation %q", m.teamEdit.open, m.teamRemoving)
		}
		m.teamEdit = teamEditor{open: true, team: removed}
		if done(kit.Done); m.teamEdit.open {
			t.Error("the deleted team's editor stayed open")
		}
	})
}

// Plugin classification, configuration checks, market actions and skill
// sources.
func TestExtensions(t *testing.T) {
	m := NewModel(preview.SettingsData())
	if got := len(Classified(m.Packages, preview.KindPlugin)); got != 5 {
		t.Errorf("plugins %d", got)
	}
	if len(Classified(m.Packages, preview.KindSkill)) != 1 || len(Classified(m.Packages, preview.KindMCP)) != 1 {
		t.Error("skill and MCP packages")
	}
	m.openConfig(m.pkg("figma"))
	if got := m.config.Check(); got != "plugins_settings_required" {
		t.Errorf("required: %q", got)
	}
	m.config.fields[0].Value, m.config.fields[1].Value = "T1", "many"
	if got := m.config.Check(); got != "plugins_settings_number" {
		t.Errorf("number: %q", got)
	}
	m.openConfig(m.pkg("git"))
	m.config.secrets["token"] = 1
	if got := m.config.Check(); got != "plugins_settings_secret_required" {
		t.Errorf("secret: %q", got)
	}
	if MarketAction(false, false) != "market_install" || MarketAction(true, true) != "market_update" || MarketAction(true, false) != "market_installed" {
		t.Error("market actions")
	}
	e := skillInstall{source: 1, link: "https://example.com/x"}
	if e.Check() != "skill_source_link_invalid" {
		t.Error("skills.sh link passed")
	}
	s := &State{Model: m}
	s.ShowDetails(DetailPlugin, "git")
	s.Open(Skills)
	if s.Details != nil {
		t.Error("opening a section kept the details")
	}
	// An install or download that completes after its dialog closed leaves
	// the dialog opened since, and its draft, alone.
	t.Run("late install", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		var done []func(bool)
		m.PackageActions = &PackageActions{
			Install:  func(_, _ string, completed func(bool)) { done = append(done, completed) },
			Download: func(_, _, _ string, completed func(bool)) { done = append(done, completed) },
		}
		s := &State{Model: m}
		tt := ui.NewTesterWith(func(c *ui.Context) { plugins(c, s) }, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		press := func(key string, last bool) {
			t.Helper()
			var found []ui.Rect
			for _, x := range tt.AccessAll() {
				if x.Role == ui.RoleButton && x.Name == i18n.In("en", key) {
					found = append(found, x.Bounds)
				}
			}
			if len(found) == 0 {
				t.Fatalf("no %s button: %q", key, tt.Texts())
			}
			r := found[0]
			if last {
				r = found[len(found)-1]
			}
			tt.ClickAt(r.X+r.W/2, r.Y+r.H/2)
			tt.Frame()
			tt.Advance(time.Second)
		}
		press("plugins_install", false)
		m.install.pkg = "first.zip"
		press("plugins_install", true)
		if len(done) != 1 || !m.install.pending {
			t.Fatalf("%d installs, dialog %+v", len(done), m.install)
		}
		press("settings_cancel", false)
		press("plugins_install", false)
		m.install.pkg = "second.zip"
		if done[0](true); !m.install.open || m.install.pkg != "second.zip" {
			t.Errorf("the first install closed the second's dialog: %+v", m.install)
		}
		press("plugins_install", true)
		if done[1](true); m.install.open {
			t.Error("its own install left the dialog open")
		}
		tt.Frame()
		tt.Advance(time.Second)
		press("plugins_download", false)
		m.download.repository = "https://example.com/first"
		press("plugins_install", true)
		if len(done) != 3 || !m.download.pending {
			t.Fatalf("%d requests, download %+v", len(done), m.download)
		}
		press("settings_cancel", false)
		press("plugins_download", false)
		if done[2](true); !m.download.open {
			t.Error("the first download closed the second's dialog")
		}
	})
}

// The CLI method's account list shows its skeleton row while the host
// looks at its accounts, then the accounts.
func TestAccountSkeleton(t *testing.T) {
	m := NewModel(preview.SettingsData())
	m.connector = connectorEditor{open: true, kind: &m.ConnTypes[0], method: 2, fields: map[string]string{}}
	tt := ui.NewTesterWith(func(c *ui.Context) { connectorDialog(c, m) }, 900, 700, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
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

// A permission row draws one way into the system's settings, and an action
// that nothing on this device can take is disabled.
func TestPermissionRowActions(t *testing.T) {
	m := NewModel(preview.SettingsData())
	m.Permissions = []preview.Permission{
		{Resource: "permission_microphone", Status: "permission_denied", Settings: true},
		{Resource: "permission_accessibility", Status: "permission_unavailable"},
		{Resource: "permission_screen", Status: "permission_granted", Settings: true},
	}
	var requested []string
	m.PermissionActions = &PermissionActions{Request: func(resource string) { requested = append(requested, resource) }}
	tt := ui.NewTester(func(c *ui.Context) { permissions(c, m) }, 900, 700)
	tt.SetLocale("en")
	tt.Frame()
	opens, request := i18n.In("en", "permission_settings"), i18n.In("en", "permission_request")
	var settings, requests []ui.AccessInfo
	for _, x := range tt.AccessAll() {
		if x.Role == ui.RoleButton && x.Name == opens {
			settings = append(settings, x)
		}
		if x.Role == ui.RoleButton && x.Name == request {
			requests = append(requests, x)
		}
	}
	// The refused microphone's action and the granted row's shortcut.
	if len(settings) != 2 || len(requests) != 1 || !requests[0].Disabled {
		t.Fatalf("settings buttons %d, request buttons %+v", len(settings), requests)
	}
	tt.ClickAt(requests[0].Bounds.X+requests[0].Bounds.W/2, requests[0].Bounds.Y+requests[0].Bounds.H/2)
	if len(requested) != 0 {
		t.Fatalf("an unavailable row requested %v", requested)
	}
}
