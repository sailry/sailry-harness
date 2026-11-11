package settings

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
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
		tt := ui.NewTesterWith(func(c *ui.Context) { mcpDialog(c, m) }, 900, 700, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		tt.Advance(time.Second)
		named := func(role ui.Role, name string) (found []ui.Rect) {
			for _, x := range tt.AccessAll() {
				if x.Role == role && x.Name == name {
					found = append(found, x.Bounds)
				}
			}
			return found
		}
		at := func(r ui.Rect) { tt.ClickAt(r.X+r.W/2, r.Y+r.H/2) }
		argument := i18n.In("en", "mcp_argument")
		inputs := named(ui.RoleTextField, argument)
		if len(inputs) != 2 {
			t.Fatalf("%d argument inputs: %q", len(inputs), tt.Texts())
		}
		at(inputs[0])
		tt.Type("first")
		at(inputs[1])
		tt.Type("second")
		at(named(ui.RoleButton, i18n.In("en", "mcp_remove_argument"))[0])
		tt.Frame()
		if !slices.Equal(m.mcp.args, []string{"second"}) {
			t.Fatalf("arguments %q", m.mcp.args)
		}
		at(named(ui.RoleTextField, argument)[0])
		tt.Key(ui.Cmd, ui.KeyZ)
		tt.Frame()
		if got := m.mcp.args[0]; got != "second" {
			t.Errorf("undo in the row that moved up gave %q", got)
		}
	})
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
