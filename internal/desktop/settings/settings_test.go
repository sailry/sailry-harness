package settings

import (
	"bytes"
	"encoding/json"
	"image"
	"image/png"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	sailry "github.com/sailry/sailry-harness"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
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
	t.Run("connection rows", func(t *testing.T) {
		local, remote, mobile := strings.Repeat("325e9373", 8), strings.Repeat("c48e0d55", 8), strings.Repeat("5e6f7a8b", 8)
		m := NewModel(preview.SettingsData())
		m.Devices = []viewmodel.Device{
			{ID: local, Name: "MacBook Pro", Platform: "macos", Execution: true, Online: true, Local: true, OS: "27.2", Version: "0.1.6"},
			{ID: remote, Name: "build-server", Platform: "linux", Execution: true, Online: true, Route: "p2p", RTT: 18, OS: "24.04", Version: "0.1.6"},
			{ID: strings.Repeat("a1b2c3d4", 8), Name: "Studio PC", Platform: "windows", Execution: true},
			{ID: mobile, Name: "iPhone", Platform: "ios", Online: true, Route: "relay", RTT: 64, Version: "0.1.6"},
		}
		tt := ui.NewTesterWith(func(c *ui.Context) {
			ui.Scroll(c).Fill().Children(func() { ui.Column(c).Padding(24, 24).Children(func() { connections(c, m) }) })
		}, 900, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("zh-CN")
		tt.Frame()
		for _, line := range []string{"macOS 27.2 · Sailry 0.1.6 ·", "Linux 24.04 · Sailry 0.1.6 ·", "Windows ·", "iOS · Sailry 0.1.6 ·", "325e9373", "c48e0d55", "5e6f7a8b"} {
			if !tt.HasText(line) {
				t.Fatalf("no %q: %q", line, tt.Texts())
			}
		}
		if tt.HasText(local) || tt.HasText(remote) || tt.HasText(mobile) {
			t.Fatalf("a row shows a whole Node ID: %q", tt.Texts())
		}
		shots := filepath.Join("..", "..", "..", ".runtime", "ui-shots")
		if err := os.MkdirAll(shots, 0o755); err != nil {
			t.Fatal(err)
		}
		file, err := os.Create(filepath.Join(shots, "connections-rows-zh-CN.png"))
		if err != nil {
			t.Fatal(err)
		}
		defer file.Close()
		if err := png.Encode(file, tt.Image()); err != nil {
			t.Fatal(err)
		}
		// The short ID's tooltip is the whole ID.
		short, _ := tt.Find("325e9373")
		tt.Move(short.X+short.W/2, short.Y+short.H/2)
		tt.Advance(2 * time.Second)
		tt.Frame()
		tt.Advance(time.Second)
		tt.Frame()
		if !tt.HasText(local) {
			t.Fatalf("hovering the short ID does not show the whole ID: %q", tt.Texts())
		}
		tip, err := os.Create(filepath.Join(shots, "connections-rows-tooltip-zh-CN.png"))
		if err != nil {
			t.Fatal(err)
		}
		defer tip.Close()
		if err := png.Encode(tip, tt.Image()); err != nil {
			t.Fatal(err)
		}
		// Every row's native menu copies the whole ID; a Host's keeps Edit.
		for name, want := range map[string]struct {
			id   string
			menu []string
		}{
			"MacBook Pro": {local, []string{i18n.In("zh-CN", "connection_action_edit"), "复制 ID"}},
			"iPhone":      {mobile, []string{"复制 ID"}},
		} {
			if err := tt.RightClick(name); err != nil {
				t.Fatal(err)
			}
			if menu := tt.Menu(); !slices.Equal(menu, want.menu) {
				t.Fatalf("%s menu: %q", name, menu)
			}
			if err := tt.ChooseMenuItem("复制 ID"); err != nil || tt.Clipboard() != want.id {
				t.Fatalf("%s copied %q: %v", name, tt.Clipboard(), err)
			}
		}
		// A pairing being removed shows it in place of its menu.
		if _, ok := tt.Access(i18n.In("zh-CN", "live_revoke")); ok {
			t.Fatal("an idle row shows a removal")
		}
		m.SetConnectionStatus(ConnectionStatus{Revoking: map[string]bool{remote: true}})
		tt.Frame()
		if _, ok := tt.Access(i18n.In("zh-CN", "live_revoke")); !ok {
			t.Fatal("a row whose pairing is being removed does not show it")
		}
	})
	// The Add host dialog offers the methods this computer can act on, sends
	// one step at a time, and checks an uncertain installation instead of
	// sending another.
	t.Run("add host", func(t *testing.T) {
		system := hostOS
		t.Cleanup(func() { hostOS = system })
		hostOS = "linux"
		en := func(key string) string { return i18n.In("en", key) }
		m := NewModel(preview.SettingsData())
		var added []ConnectionDraft
		checks := 0
		m.ConnectionActions = &ConnectionActions{
			Add:         func(draft ConnectionDraft, _ func(bool)) { added, m.ConnectionPending = append(added, draft), true },
			CheckStatus: func(func(bool)) { checks, m.ConnectionPending = checks+1, true },
		}
		tt := ui.NewTesterWith(func(c *ui.Context) { connections(c, m) }, 900, 1000, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		click := func(text string) {
			t.Helper()
			tt.Frame()
			if err := tt.Click(text); err != nil {
				t.Fatalf("%s: %v in %q", text, err, tt.Texts())
			}
			tt.Frame()
			tt.Advance(time.Second)
			tt.Frame()
		}
		shot := func(name string) {
			t.Helper()
			shots := filepath.Join("..", "..", "..", ".runtime", "ui-shots")
			if err := os.MkdirAll(shots, 0o755); err != nil {
				t.Fatal(err)
			}
			file, err := os.Create(filepath.Join(shots, name+".png"))
			if err != nil {
				t.Fatal(err)
			}
			defer file.Close()
			if err := png.Encode(file, tt.Image()); err != nil {
				t.Fatal(err)
			}
		}
		alpha, beta := ProjectChoice{ID: "a", Name: "Alpha"}, ProjectChoice{ID: "b", Name: "Beta"}
		m.openAddHost()
		m.SetConnectionStatus(ConnectionStatus{Projects: []ProjectChoice{alpha, beta}})
		tt.Frame()
		tt.Advance(time.Second)
		tt.Frame()
		if tt.HasText(en("host_wsl")) || !tt.HasText(en("ssh_agent_short")) {
			t.Fatalf("methods off Windows: %q", tt.Texts())
		}
		// The agent has no secret; a shared connection names its projects.
		click(en("ssh_agent_short"))
		if Method(m.pairing.method) != ByAgent || tt.HasText(en("ssh_passphrase")) {
			t.Fatalf("agent method %d: %q", m.pairing.method, tt.Texts())
		}
		click(en("connection_projects"))
		shot("connections-add-agent-en")
		click(en("host_install"))
		if len(added) != 0 {
			t.Fatalf("installed sharing with no project: %+v", added)
		}
		click(beta.Name)
		click(en("host_install"))
		if len(added) != 1 || added[0].Method != ByAgent || added[0].Scope != ScopeProjects || added[0].Projects != beta.ID {
			t.Fatalf("agent install: %+v", added)
		}
		// The step under way cannot start again, and locks its draft.
		click(en("host_install"))
		click(en("ssh_private_key"))
		if len(added) != 1 || Method(m.pairing.method) != ByAgent {
			t.Fatalf("pending dialog acted: %d installs, method %d", len(added), m.pairing.method)
		}
		// An uncertain installation is checked, never sent again.
		m.ConnectionPending = false
		m.SetConnectionStatus(ConnectionStatus{Recovery: true, Projects: []ProjectChoice{alpha, beta}})
		tt.Frame()
		if tt.HasText(en("host_install")) || !tt.HasText(en("host_check_status")) {
			t.Fatalf("uncertain installation: %q", tt.Texts())
		}
		shot("connections-add-check-en")
		click(en("ssh_private_key"))
		click(en("host_check_status"))
		click(en("host_check_status"))
		if checks != 1 || len(added) != 1 || Method(m.pairing.method) != ByAgent {
			t.Fatalf("%d checks, %d installs, method %d", checks, len(added), m.pairing.method)
		}
		// A secret left blank keeps the one saved for the same method.
		m.ConnectionPending = false
		m.SetConnectionStatus(ConnectionStatus{Saved: ByPrivateKey})
		click(en("ssh_private_key"))
		if !tt.HasText(en("ssh_keep_credential")) {
			t.Fatalf("no hint for the saved credential: %q", tt.Texts())
		}
		shot("connections-add-key-en")
		click(en("ssh_key_file"))
		if tt.HasText(en("ssh_keep_credential")) {
			t.Fatal("another method offers to keep the saved credential")
		}
		// Windows offers WSL instead of the agent, with the distributions
		// the Node found.
		hostOS = "windows"
		m.openAddHost()
		m.SetConnectionStatus(ConnectionStatus{DistributionsLoaded: true})
		tt.Frame()
		tt.Advance(time.Second)
		tt.Frame()
		if !tt.HasText(en("host_wsl")) || tt.HasText(en("ssh_agent_short")) {
			t.Fatalf("methods on Windows: %q", tt.Texts())
		}
		click(en("host_wsl"))
		if !tt.HasText(en("host_wsl_empty")) || tt.HasText(en("connection_scope")) {
			t.Fatalf("WSL without distributions: %q", tt.Texts())
		}
		click(en("host_install"))
		if len(added) != 1 {
			t.Fatalf("installed without a distribution: %+v", added)
		}
		m.SetConnectionStatus(ConnectionStatus{DistributionsLoaded: true, Distributions: []string{"Ubuntu", "Debian"}})
		tt.Frame()
		shot("connections-add-wsl-en")
		click(en("host_install"))
		if len(added) != 2 || added[1].Method != ByWSL || added[1].Distribution != "Ubuntu" {
			t.Fatalf("WSL install: %+v", added)
		}
	})
}

// The provider editor's steps and checks.
func TestProviderEditor(t *testing.T) {
	m := NewModel(preview.SettingsData())
	e := &m.provider
	e.add(m)
	e.choose(&m.Brands[0], apiMethods(&m.Brands[0])) // Anthropic: several API methods
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
	e.choose(&m.Brands[2], apiMethods(&m.Brands[2])) // Google: one method
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
	// A sign-in opens its form alone: Back returns to the brand step, not to
	// the brand's API methods.
	e.choose(&m.Brands[0], m.Brands[0].Methods[:1])
	if e.step != stepConfigure || !e.login() || e.usesKey() {
		t.Errorf("sign-in went to step %d, login %v, key %v", e.step, e.login(), e.usesKey())
	}
	if e.back(); e.step != stepBrand {
		t.Errorf("back from a sign-in went to %d", e.step)
	}
	// A method's routes are one entry: the form switches its protocol,
	// product and region, keeping what was typed and following the route's
	// own defaults.
	t.Run("routes", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		hosted := preview.Method{ID: "hosted", Name: "Hosted", Access: "provider_access_key", Routes: []preview.Route{
			{ID: "hosted/0", API: "chat_completions", Endpoint: "https://cn.example/v1", Region: "china", Product: "metered"},
			{ID: "hosted/1", API: "chat_completions", Endpoint: "https://global.example/v1", Region: "global", Product: "metered"},
			{ID: "hosted/2", API: "chat_completions", Endpoint: "https://cn.example/coding", Region: "china", Product: "coding", ProductName: "Hosted Code", Models: []string{"coder"}},
			{ID: "hosted/3", API: "chat_completions", Endpoint: "https://global.example/coding", Region: "global", Product: "coding", ProductName: "Hosted Code", Models: []string{"coder"}},
		}}
		compatible := preview.Method{ID: "compatible", Name: "Compatible", Access: "provider_access_endpoint", Keyed: true, Routes: []preview.Route{
			{ID: "compatible/0", API: "responses", Custom: true}, {ID: "compatible/1", API: "chat_completions", Custom: true},
		}}
		brand := &preview.Brand{ID: "fixture", Name: "Fixture", Methods: []preview.Method{hosted, compatible}}
		e := &m.provider
		e.add(m)
		e.choose(brand, apiMethods(brand))
		if e.step != stepMethod || len(e.methods) != 2 {
			t.Fatalf("two methods went to step %d with %d rows", e.step, len(e.methods))
		}
		e.configure(compatible)
		if got := e.choices(axisProtocol); !slices.Equal(got, []int{0, 1}) || len(e.choices(axisProduct)) != 1 || len(e.choices(axisRegion)) != 1 {
			t.Fatalf("compatible protocols %v, products %v, regions %v", got, e.choices(axisProduct), e.choices(axisRegion))
		}
		e.name, e.endpoint, e.key = "Mine", "https://mine.example/v1", "sk-typed"
		e.models = []modelDraft{{id: "typed", context: "128000"}}
		e.switchRoute(1)
		if e.current().API != "chat_completions" || e.name != "Mine" || e.endpoint != "https://mine.example/v1" || e.key != "sk-typed" || len(e.models) != 1 || e.models[0].id != "typed" {
			t.Fatalf("switched protocol left route %+v, name %q, endpoint %q, key %q, models %+v", e.current(), e.name, e.endpoint, e.key, e.models)
		}
		if !e.usesKey() || e.Check() != "" {
			t.Errorf("a keyed custom endpoint: key %v, check %q", e.usesKey(), e.Check())
		}
		n := len(m.Channels)
		if got := e.save(m); got != "" || m.Channels[n].Method != "compatible/1" || m.Channels[n].API != "chat_completions" {
			t.Fatalf("save %q kept %+v", got, m.Channels[n])
		}
		// One protocol has no control; products and regions each have two.
		e.configure(hosted)
		if len(e.choices(axisProtocol)) != 1 || !slices.Equal(e.choices(axisProduct), []int{0, 2}) || !slices.Equal(e.choices(axisRegion), []int{0, 1}) {
			t.Fatalf("hosted protocols %v, products %v, regions %v", e.choices(axisProtocol), e.choices(axisProduct), e.choices(axisRegion))
		}
		e.switchRoute(1) // Global
		if e.endpoint != "https://global.example/v1" || len(e.models) != 0 {
			t.Fatalf("region left endpoint %q, models %+v", e.endpoint, e.models)
		}
		e.switchRoute(e.choices(axisProduct)[1]) // the coding product keeps the region
		if e.current().ID != "hosted/3" || e.endpoint != "https://global.example/coding" || len(e.models) != 1 || e.models[0].id != "coder" {
			t.Fatalf("product went to %+v, endpoint %q, models %+v", e.current(), e.endpoint, e.models)
		}
		e.switchRoute(e.choices(axisProduct)[0]) // back: the model the product came with goes
		if e.current().ID != "hosted/1" || len(e.models) != 0 {
			t.Fatalf("product back went to %+v, models %+v", e.current(), e.models)
		}
		// A model the user added keeps every model through a switch.
		e.switchRoute(3)
		e.models = append(e.models, modelDraft{id: "mine", context: "128000"})
		e.switchRoute(1)
		if len(e.models) != 2 {
			t.Fatalf("a switch dropped the user's models: %+v", e.models)
		}
		// An edited channel opens on its route, which stays.
		m.Brands = append(m.Brands, *brand)
		ch := preview.Channel{ID: "saved", Name: "Saved", Brand: "fixture", Method: "hosted/2", API: "chat_completions", KeySet: true}
		e.edit(m, &ch)
		if e.method.ID != "hosted" || e.current().ID != "hosted/2" || e.step != stepConfigure {
			t.Fatalf("edit opened %q on %+v at step %d", e.method.ID, e.current(), e.step)
		}
		if e.switchRoute(0); e.current().ID != "hosted/2" {
			t.Errorf("an edited channel changed its route to %+v", e.current())
		}
	})
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
	// The capability boxes show a model's tags; a change marks them the
	// user's own, and the save carries them.
	t.Run("capabilities", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		m.Live = true
		var saved []ProviderDraft
		m.ProviderActions.Save = func(d ProviderDraft, _ func(bool)) { saved = append(saved, d) }
		m.provider = providerEditor{open: true, step: stepConfigure, name: "Provider", keySet: true, models: draftsOf([]preview.ChannelModel{
			{ID: "hosted", Context: 128000, Tools: true, Generates: []string{viewmodel.GenerateImageTool, viewmodel.GenerateSpeech}},
			{ID: "plain", Context: 128000, Vision: true},
		})}
		for i := range m.provider.models {
			m.provider.models[i].open = true
		}
		tt := ui.NewTesterWith(func(c *ui.Context) { providerEditorDialog(c, m) }, 900, 1400, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		tt.Advance(time.Second)
		// boxes are a model's six, in the form's order.
		names := []string{"Understands Image", "Can Tools", "Can Reasoning", "Generates Image", "Generates Video", "Generates Speech"}
		boxes := func(model int) (checked [6]bool, at [6]ui.AccessInfo) {
			t.Helper()
			for i, name := range names {
				seen := 0
				for _, x := range tt.AccessAll() {
					if x.Role == ui.RoleCheckBox && x.Name == name {
						if seen == model {
							checked[i], at[i] = x.Checked, x
						}
						seen++
					}
				}
				if seen != 2 {
					t.Fatalf("%d boxes named %q", seen, name)
				}
			}
			return
		}
		click := func(model, box int) {
			t.Helper()
			_, at := boxes(model)
			tt.ClickAt(at[box].Bounds.X+at[box].Bounds.H/2, at[box].Bounds.Y+at[box].Bounds.H/2)
			tt.Frame()
		}
		if got, _ := boxes(0); got != [6]bool{false, true, false, true, false, true} {
			t.Fatalf("hosted model's boxes %v", got)
		}
		if got, _ := boxes(1); got != [6]bool{true, false, false, false, false, false} {
			t.Fatalf("plain model's boxes %v", got)
		}
		hosted, plain := &m.provider.models[0].model, &m.provider.models[1].model
		if hosted.CustomCapabilities || plain.CustomCapabilities {
			t.Fatal("unedited tags are the user's own")
		}
		// The one Image box keeps the hosted tool the model came with.
		click(0, 3)
		if !slices.Equal(hosted.Generates, []string{viewmodel.GenerateSpeech}) || !hosted.CustomCapabilities {
			t.Fatalf("hosted model without Image: %+v", hosted)
		}
		click(0, 3)
		if !slices.Equal(hosted.Generates, []string{viewmodel.GenerateSpeech, viewmodel.GenerateImageTool}) {
			t.Fatalf("hosted model with Image again generates %v", hosted.Generates)
		}
		click(0, 0)
		click(0, 2)
		// A model without the hosted tool generates by the endpoint.
		click(1, 3)
		click(1, 4)
		click(1, 0)
		if !slices.Equal(plain.Generates, []string{viewmodel.GenerateImage, viewmodel.GenerateVideo}) || plain.Vision || !plain.CustomCapabilities {
			t.Fatalf("plain model: %+v", plain)
		}
		if got, _ := boxes(1); got != [6]bool{false, false, false, true, true, false} {
			t.Fatalf("plain model's boxes after edits %v", got)
		}
		// Tags that are the user's own can be reset: the boxes show the tags
		// the model came with, which a save leaves to the catalog.
		resets := func() (found []ui.AccessInfo) {
			for _, x := range tt.AccessAll() {
				if x.Role == ui.RoleButton && x.Name == "Reset to model data" {
					found = append(found, x)
				}
			}
			return
		}
		at := resets()
		if len(at) != 2 {
			t.Fatalf("%d resets for two edited models", len(at))
		}
		tt.ClickAt(at[1].Bounds.X+at[1].Bounds.W/2, at[1].Bounds.Y+at[1].Bounds.H/2)
		tt.Frame()
		if got, _ := boxes(1); got != [6]bool{true, false, false, false, false, false} || plain.CustomCapabilities || len(plain.Generates) != 0 || len(resets()) != 1 {
			t.Fatalf("plain model after its reset: boxes %v, %+v, %d resets", got, plain, len(resets()))
		}
		click(1, 3)
		click(1, 4)
		click(1, 0)
		if problem := m.provider.save(m); problem != "" || len(saved) != 1 {
			t.Fatalf("save: %q, %d drafts", problem, len(saved))
		}
		first, second := saved[0].Channel.Models[0], saved[0].Channel.Models[1]
		if !first.CustomCapabilities || !first.Vision || !first.Tools || !first.Reasoning || !slices.Equal(first.Generates, []string{viewmodel.GenerateSpeech, viewmodel.GenerateImageTool}) {
			t.Errorf("saved hosted model %+v", first)
		}
		if !second.CustomCapabilities || second.Vision || second.Tools || !slices.Equal(second.Generates, []string{viewmodel.GenerateImage, viewmodel.GenerateVideo}) {
			t.Errorf("saved plain model %+v", second)
		}
	})
	// Each media row offers Follow conversation model and the models tagged
	// for its purpose, and marks a saved model that cannot run.
	t.Run("media rows", func(t *testing.T) {
		s := New()
		m := s.Model
		m.MediaOptions = []viewmodel.MediaOption{
			{Key: "a/see", Label: "A · see", Kinds: []string{"vision"}},
			{Key: "a/draw", Label: "A · draw", Kinds: []string{"image"}},
			{Key: "a/all", Label: "A · all", Kinds: []string{"vision", "image", "video", "speech"}},
			{Key: "b/gone", Label: "B · gone"},
		}
		m.Media = stored([4]string{"", "", "", "b/gone"})
		m.MediaUnavailable = [4]bool{false, false, false, true}
		var toasts []string
		tt := ui.NewTesterWith(func(c *ui.Context) {
			mediaModels(c, m)
			ui.ToastViewportBase(c, func(_ ui.Element, shown []ui.Toast) {
				toasts = toasts[:0]
				for _, toast := range shown {
					toasts = append(toasts, toast.Title)
				}
			})
		}, 900, 1600, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		named := func(role ui.Role, name string) (found []ui.AccessInfo) {
			for _, x := range tt.AccessAll() {
				if x.Role == role && x.Name == name {
					found = append(found, x)
				}
			}
			return
		}
		// offered opens a row's select and returns its options.
		offered := func(row string) (labels []string) {
			t.Helper()
			trigger := named(ui.RoleMenuButton, row)
			if len(trigger) != 1 {
				t.Fatalf("%d selects named %q", len(trigger), row)
			}
			tt.ClickAt(trigger[0].Bounds.X+trigger[0].Bounds.W/2, trigger[0].Bounds.Y+trigger[0].Bounds.H/2)
			tt.Frame()
			tt.Advance(time.Second)
			for _, x := range tt.AccessAll() {
				if x.Role == ui.RoleMenuItem || x.Role == ui.RoleMenuItemRadio || x.Role == ui.RoleMenuItemCheckBox {
					labels = append(labels, x.Name)
				}
			}
			return
		}
		choose := func(label string) {
			t.Helper()
			for _, x := range tt.AccessAll() {
				if (x.Role == ui.RoleMenuItem || x.Role == ui.RoleMenuItemRadio || x.Role == ui.RoleMenuItemCheckBox) && x.Name == label {
					tt.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
					tt.Frame()
					tt.Advance(time.Second)
					return
				}
			}
			t.Fatalf("no option %q", label)
		}
		follow := i18n.In("en", "media_follow")
		for row, want := range map[string][]string{
			"Vision": {follow, "A · see", "A · all"},
			"Image":  {follow, "A · draw", "A · all"},
			"Video":  {follow, "A · all"},
			"Speech": {follow, "A · all", "B · gone"},
		} {
			if got := offered(row); !slices.Equal(got, want) {
				t.Errorf("%s offers %q, want %q", row, got, want)
			}
			tt.Key(0, ui.KeyEscape)
			tt.Frame()
			tt.Advance(time.Second)
		}
		unavailable := i18n.In("en", "media_model_unavailable")
		marked := func() (count int) {
			for _, x := range tt.AccessAll() {
				if x.Name == unavailable || x.Value == unavailable {
					count++
				}
			}
			return
		}
		if marked() != 1 {
			t.Errorf("%d rows marked unavailable, want the Speech row alone", marked())
		}
		offered("Image")
		choose("A · draw")
		if m.Media.Draft != [4]string{"", "a/draw", "", "b/gone"} || m.Media.Saved != m.Media.Draft {
			t.Errorf("chosen image model: draft %q, saved %q", m.Media.Draft, m.Media.Saved)
		}
		if len(toasts) != 0 {
			t.Errorf("a saved choice toasted %q", toasts)
		}
		// Another choice leaves the row: the unavailable mark is the saved model's.
		offered("Speech")
		choose(follow)
		if marked() != 0 || m.Media.Draft[3] != "" {
			t.Errorf("after Follow conversation model: %d marks, draft %q", marked(), m.Media.Draft)
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
		m := NewModel(preview.SettingsData())
		m.Live = true
		var reconnected []string
		m.ConnectorActions = &ConnectorActions{Reconnect: func(id string) { reconnected = append(reconnected, id) }}
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
			status ConnStatus
			label  string
			items  []string
		}{
			{ConnStatus{Problem: "connector_not_connected", Tools: -1}, "connector_not_connected", []string{"connector_add_credentials", "connector_mcp_reconnect", "connector_details"}},
			{ConnStatus{Problem: "connector_not_connected", Tools: -1}, "connector_not_connected", []string{"connector_connect", "connector_details"}},
			{ConnStatus{Failed: true, Reason: "dial refused", Tools: -1}, "connector_mcp_failed", []string{"connector_add_credentials", "connector_mcp_reconnect", "connector_details"}},
			{ConnStatus{Problem: "connector_unavailable", Reason: "computer helper is missing", Tools: -1}, "connector_unavailable", []string{"connector_details"}},
			{ConnStatus{Problem: "connector_package_disabled", Tools: -1}, "connector_package_disabled", []string{"connector_details"}},
			{ConnStatus{Problem: "connector_configuration_required", Tools: -1}, "connector_configuration_required", []string{"connector_details"}},
			{ConnStatus{Connecting: true, Tools: -1}, "connector_mcp_connecting", []string{"connector_details"}},
			{ConnStatus{Tools: 3}, "", []string{"connector_mcp_reconnect", "connector_details"}},
			{ConnStatus{Problem: "connector_not_connected", Tools: -1}, "connector_not_connected", nil},
		}
		for i, x := range want {
			s := &m.Services[i]
			if got := StatusOf(s); got != x.status {
				t.Errorf("%s status %+v, want %+v", s.ID, got, x.status)
			}
			if got := m.serviceItems(s); !slices.Equal(got, x.items) {
				t.Errorf("%s items %v, want %v", s.ID, got, x.items)
			}
		}
		tt := ui.NewTesterWith(func(c *ui.Context) { connectors(c, m) }, 1000, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		tt.Advance(time.Second)
		texts := tt.Texts()
		count := func(key string) (n int) {
			for _, text := range texts {
				if text == i18n.In("en", key) {
					n++
				}
			}
			return n
		}
		// The failed row's dot is named as its text is.
		for key, n := range map[string]int{"connector_not_connected": 3, "connector_mcp_failed": 2, "connector_unavailable": 1, "connector_package_disabled": 1, "connector_configuration_required": 1, "connector_mcp_connecting": 1} {
			if got := count(key); got != n {
				t.Errorf("%d rows show %q, want %d: %q", got, i18n.In("en", key), n, texts)
			}
		}
		if !slices.Contains(texts, i18n.In("en", "connector_mcp_tools", "n", 3)) {
			t.Errorf("the connected row does not show its tools: %q", texts)
		}
		menus := func() (found []ui.AccessInfo) {
			for _, x := range tt.AccessAll() {
				if x.Name == i18n.In("en", "more") {
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
			box, ok := tt.Find(i18n.In("en", label))
			if !ok {
				t.Fatalf("no %q status", label)
			}
			tt.Move(box.X+box.W/2, box.Y+box.H/2)
			tt.Frame()
			tt.Advance(2 * time.Second)
			if !tt.HasText(reason) {
				t.Errorf("resting on %q does not show %q: %q", i18n.In("en", label), reason, tt.Texts())
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
				if x.Name == i18n.In("en", key) {
					target = &x
				}
			}
			var items []string
			for _, k := range want[row].items {
				items = append(items, i18n.In("en", k))
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
	en := func(key string, a ...any) string { return i18n.In("en", key, a...) }
	page := func(m *Model) (*ui.Tester, func()) {
		tt := ui.NewTesterWith(func(c *ui.Context) { connectors(c, m) }, 1000, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
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
	manual := func() (*Model, *[]string, *[]MCPDraft, *[]func(bool)) {
		m := NewModel(preview.SettingsData())
		m.Live = true
		var asked []string
		var drafts []MCPDraft
		var done []func(bool)
		m.ConnectorActions = &ConnectorActions{
			ConfigureMCP: func(id string) { asked = append(asked, id) },
			SaveMCP: func(draft MCPDraft, completed func(bool)) {
				drafts, done = append(drafts, draft), append(done, completed)
			},
		}
		// The live row of a server added by hand: its definition and slot
		// names, with nothing about which slot holds a credential.
		m.Services = []preview.Service{{ID: "mcp:mcp-files", Name: "files", Revision: 4, Manual: true, Servers: []preview.Server{{
			Name: "server", Allowed: true, Command: "npx", Args: []string{"-y", "files"}, Env: []preview.EnvVar{{Key: "TOKEN"}, {Key: "REGION"}}}}}}
		return m, &asked, &drafts, &done
	}
	saved := func(m *Model) MCPConfiguration {
		return MCPConfiguration{Service: m.Services[0], Saved: []string{"TOKEN"}}
	}
	// Configure reads the saved server before its editor can save: the draft
	// is the server's own, and a saved credential is kept unless typed over
	// or cleared, never replaced by an empty value.
	t.Run("configure loads the saved server", func(t *testing.T) {
		m, asked, drafts, done := manual()
		tt, settle := page(m)
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
		m.OpenMCPConfiguration(MCPConfiguration{Service: preview.Service{ID: "mcp:other", Servers: []preview.Server{{Command: "other"}}}})
		if m.mcp.command != "npx" || !m.mcp.loading {
			t.Errorf("another service's configuration filled the editor: %+v", m.mcp)
		}
		m.OpenMCPConfiguration(saved(m))
		settle()
		e := &m.mcp
		if e.loading || e.name != "files" || e.command != "npx" || !slices.Equal(e.args, []string{"-y", "files"}) || len(e.slots) != 2 || e.revision != 4 {
			t.Fatalf("the editor does not hold the saved server: %+v", *e)
		}
		if !tt.HasText(en("mcp_secret_saved")) || !e.kept(0) || e.kept(1) {
			t.Errorf("saved slots: kept %v %v: %q", e.kept(0), e.kept(1), tt.Texts())
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
		// Clear and Keep toggle the saved credential; a typed value replaces
		// it; another kind of transport does not carry it over.
		press(tt, button(t, tt, en("mcp_clear_secret")))
		if e.kept(0) || !tt.HasText(en("mcp_secret_cleared")) || len(e.draft().Keep) != 0 {
			t.Errorf("Clear left the credential kept: %q", tt.Texts())
		}
		press(tt, button(t, tt, en("mcp_keep_secret")))
		if !e.kept(0) || !tt.HasText(en("mcp_secret_saved")) {
			t.Errorf("Keep did not keep the credential: %q", tt.Texts())
		}
		e.transport, e.url = 1, "https://example.test/mcp"
		settle()
		if e.kept(0) || !tt.HasText(en("mcp_secret_cleared")) {
			t.Error("a credential saved for a command is kept for a URL")
		}
		if _, ok := tt.AccessByRole(ui.RoleButton, en("mcp_keep_secret")); ok {
			t.Error("a credential of another transport can be kept")
		}
		e.transport = 0
		e.slots[0].Value = "typed"
		settle()
		if d := e.draft(); len(d.Keep) != 0 || d.Server.Env[0].Value != "typed" || tt.HasText(en("mcp_secret_saved")) {
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
		m.OpenMCPConfiguration(saved(m))
		if m.mcp.open {
			t.Error("a late configuration reopened the closed editor")
		}
	})
	// A failed read leaves the editor locked with Reload and the reason
	// behind Details; an uncertain save locks the draft and Save retries it.
	t.Run("reload, diagnostics and retry", func(t *testing.T) {
		m, asked, drafts, done := manual()
		tt, settle := page(m)
		m.serviceAction(nil, &m.Services[0], "connector_configure")
		m.FailMCPConfiguration("mcp:mcp-files", "settings database is locked")
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
		m.OpenMCPConfiguration(saved(m))
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
		tt, settle := page(m)
		m.ConnMethod = slices.Index(connectorMethods, "connector_no_auth")
		settle()
		if tt.HasText("Hub") || !tt.HasText("files") || !tt.HasText("pair") {
			t.Errorf("No authentication shows %q", tt.Texts())
		}
		m.ConnMethod = slices.Index(connectorMethods, "connector_token")
		settle()
		if !tt.HasText("Hub") || tt.HasText("files") {
			t.Errorf("Token shows %q", tt.Texts())
		}
		m.ConnMethod = 0
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
		m.OpenMCPConfiguration(MCPConfiguration{Declared: true, Saved: []string{"KEY"},
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
		m := NewModel(preview.SettingsData())
		m.Live = true
		var drafts []ConnectorDraft
		var done []func(bool)
		inspected := 0
		m.ConnectorActions = &ConnectorActions{
			Save: func(draft ConnectorDraft, completed func(bool)) {
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
		tt, settle := page(m)
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
	// A card, a row and the details header draw the icon their package
	// declares: a white glyph on the gradient of its tint, or a brand's mark
	// on a neutral tile, still whether or not the pointer is over it. Only a
	// package that declares none, or a name nothing draws, has the puzzle
	// piece.
	t.Run("declared icons", func(t *testing.T) {
		manifests, err := fs.Glob(sailry.Plugins, "plugins/*/plugin.json")
		if err != nil || len(manifests) < 40 {
			t.Fatalf("bundled manifests: %d, %v", len(manifests), err)
		}
		var rows []preview.Package
		for _, name := range manifests {
			data, err := fs.ReadFile(sailry.Plugins, name)
			if err != nil {
				t.Fatal(err)
			}
			type text struct {
				Label   string            `json:"label"`
				Locales map[string]string `json:"locales"`
				Icon    json.RawMessage   `json:"icon"`
			}
			var manifest struct {
				Name       string `json:"name"`
				Version    string `json:"version"`
				Extensions map[string]struct {
					Display, Description text
					Tint, Category       string
				} `json:"extensions"`
			}
			if err := json.Unmarshal(data, &manifest); err != nil {
				t.Fatal(name, err)
			}
			extension := manifest.Extensions["dev.sailry.platform"]
			row := preview.Package{ID: manifest.Name, Name: extension.Display.Locales["zh-CN"], Description: extension.Description.Locales["zh-CN"], Version: manifest.Version, Latest: manifest.Version,
				Tint: extension.Tint, Category: "market_category_" + extension.Category, Source: "market_details_source_official", Official: true, Versions: []string{manifest.Version}}
			var own struct {
				Svg string `json:"svg"`
			}
			if json.Unmarshal(extension.Display.Icon, &row.Icon) != nil && json.Unmarshal(extension.Display.Icon, &own) == nil {
				row.Icon = own.Svg
			}
			if row.Name == "" {
				row.Name = extension.Display.Label
			}
			if row.Description == "" {
				row.Description = extension.Description.Label
			}
			if row.Name == "" || row.Icon == "" {
				t.Fatalf("%s: name %q, icon %q", name, row.Name, row.Icon)
			}
			rows = append(rows, row)
		}
		m := NewModel(preview.SettingsData())
		m.MarketPlugins = rows
		m.Packages = nil
		for i, row := range rows {
			if i%3 != 0 {
				row.Installed, row.Enabled = true, true
				m.Packages = append(m.Packages, row)
			}
		}
		s := &State{Model: m}
		page, mode := 0, theme.Dark
		tt := ui.NewTesterWith(func(c *ui.Context) {
			theme.Apply(c, "", mode)
			p := theme.Of(c)
			ui.Column(c).Fill().Background(p.Background).TextColor(p.Foreground).Children(func() {
				switch page {
				case 0:
					MarketPage(c, s)
				case 1:
					ui.Scroll(c.Key("plugins")).Fill().Children(func() {
						ui.Column(c).FillWidth().MaxWidth(kit.PageWidth).Margin(0, ui.Auto).Padding(24).Children(func() { plugins(c, s) })
					})
				default:
					ui.Column(c.Key("details")).Width(520).Grow(1).Children(func() { DetailsPanel(c, s) })
				}
			})
		}, 900, 2500, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("zh-CN")
		shots := filepath.Join("..", "..", "..", ".runtime", "ui-shots")
		if err := os.MkdirAll(shots, 0o755); err != nil {
			t.Fatal(err)
		}
		settle := func() {
			t.Helper()
			tt.Frame()
			for range 400 {
				if !tt.FramePending() {
					return
				}
				tt.Advance(50 * time.Millisecond)
				tt.Frame()
			}
			t.Fatalf("the window still asks for frames with nothing moving: %q", tt.Texts())
		}
		save := func(name string) {
			t.Helper()
			file, err := os.Create(filepath.Join(shots, name+".png"))
			if err != nil {
				t.Fatal(err)
			}
			defer file.Close()
			if err := png.Encode(file, tt.Image()); err != nil {
				t.Fatal(err)
			}
		}
		// where is the box of the icon tile: the leading 44 DIPs of the card's
		// summary button, or the 64 before the details' title.
		where := func(title string) ui.Rect {
			t.Helper()
			if page == 2 {
				at, found := tt.Find(title)
				if !found {
					t.Fatalf("no %q: %q", title, tt.Texts())
				}
				return ui.Rect{X: at.X - 72, Y: at.Y + at.H/2 - 30, W: 64, H: 60}
			}
			card, found := tt.AccessByRole(ui.RoleButton, title)
			if !found {
				t.Fatalf("no %q card: %q", title, tt.Texts())
			}
			return ui.Rect{X: card.Bounds.X - 4, Y: card.Bounds.Y, W: 44, H: card.Bounds.H}
		}
		// area is a box of the window in the last frame's pixels.
		area := func(at ui.Rect) (*image.RGBA, image.Rectangle) {
			img := tt.Image()
			scale := float32(img.Bounds().Dx()) / 900
			return img, image.Rect(int(at.X*scale), int(at.Y*scale), int((at.X+at.W)*scale), int((at.Y+at.H)*scale))
		}
		// mark is the pixels of the icon tile.
		mark := func(title string) string {
			t.Helper()
			settle()
			img, box := area(where(title))
			var pixels []byte
			for y := box.Min.Y; y < box.Max.Y; y++ {
				pixels = append(pixels, img.Pix[img.PixOffset(box.Min.X, y):img.PixOffset(box.Max.X, y)]...)
			}
			return string(pixels)
		}
		// vivid counts the pixels of a box that have a hue.
		vivid := func(at ui.Rect) int {
			img, box := area(at)
			count := 0
			for y := box.Min.Y; y < box.Max.Y; y++ {
				for x := box.Min.X; x < box.Max.X; x++ {
					if pixel := img.RGBAAt(x, y); max(pixel.R, pixel.G, pixel.B)-min(pixel.R, pixel.G, pixel.B) > 80 {
						count++
					}
				}
			}
			return count
		}
		subject := &m.MarketPlugins[0]
		for i := range m.MarketPlugins {
			if m.MarketPlugins[i].ID == "files" {
				subject = &m.MarketPlugins[i]
			}
		}
		installed := m.installedPackage(subject.ID)
		if subject.Icon != "phosphor:folder" || installed == nil {
			t.Fatalf("the Files package declares %q, installed %v", subject.Icon, installed != nil)
		}
		// brand is a package whose icon is a brand's mark in its own colors.
		var brand *preview.Package
		for i := range m.MarketPlugins {
			if m.MarketPlugins[i].ID == "figma" {
				brand = &m.MarketPlugins[i]
			}
		}
		if brand == nil || brand.Icon != "ailogo:figma" {
			t.Fatalf("the Figma package declares %+v", brand)
		}
		for index, step := range []struct {
			name string
			of   *preview.Package
			open func()
		}{
			{"market", subject, func() { page = 0 }},
			{"plugins", installed, func() { page = 1 }},
			{"plugin-details", installed, func() {
				page = 2
				s.ShowDetails(DetailPlugin, subject.ID)
			}},
		} {
			step.open()
			declared := step.of.Icon
			drawn := mark(step.of.Name)
			for _, dark := range []bool{true, false} {
				mode = map[bool]theme.Mode{true: theme.Dark, false: theme.Light}[dark]
				tt.SetDark(dark)
				settle()
				save(step.name + "-icons-zh-CN-" + map[bool]string{true: "dark", false: "light"}[dark])
			}
			mode = theme.Dark
			tt.SetDark(true)
			if again := mark(step.of.Name); again != drawn {
				t.Errorf("%s: the icon at rest changed between frames", step.name)
			}
			step.of.Icon = ""
			placeholder := mark(step.of.Name)
			if placeholder == drawn {
				t.Errorf("%s: the declared icon draws as the placeholder", step.name)
			}
			for _, unknown := range []string{"reicon:missing/icon", "phosphor:missing", "ailogo:missing-brand", "kanban"} {
				step.of.Icon = unknown
				if mark(step.of.Name) != placeholder {
					t.Errorf("%s: the unknown icon %q does not fall back to the placeholder", step.name, unknown)
				}
			}
			step.of.Icon = "phosphor:rocket"
			glyph := mark(step.of.Name)
			if glyph == placeholder || glyph == drawn {
				t.Errorf("%s: a Phosphor icon named by its id is not drawn", step.name)
			}
			step.of.Icon = "reicon:ui/puzzle-piece"
			if mark(step.of.Name) != placeholder {
				t.Errorf("%s: a Reicon glyph is not white on the tint's gradient", step.name)
			}
			step.of.Icon = `<svg viewBox="0 0 24 24"><path d="M4 4h16v16H4z"/></svg>`
			if own := mark(step.of.Name); own == placeholder || own == glyph {
				t.Errorf("%s: a package's own SVG is not drawn", step.name)
			}
			// A brand's mark is drawn, in the brand's colors where the
			// collection has them.
			step.of.Icon = "ailogo:figma"
			logo := mark(step.of.Name)
			step.of.Icon = "ailogo:github"
			mono := mark(step.of.Name)
			if logo == placeholder || mono == placeholder || logo == mono {
				t.Errorf("%s: a brand's mark is not drawn", step.name)
			}
			step.of.Icon = declared
			if index > 0 {
				continue
			}
			// A market card's tile is the 36 DIPs square at the start of its
			// summary button: inner is the 20 DIPs at its middle, where the
			// icon is, and edge the strip under its top, clear of the icon.
			tile := func(title string) (inner, edge ui.Rect) {
				at := where(title)
				middle := at.Y + at.H/2
				return ui.Rect{X: at.X + 12, Y: middle - 10, W: 20, H: 20}, ui.Rect{X: at.X + 16, Y: middle - 16, W: 12, H: 3}
			}
			// The glyph is on its tint's gradient, in no color of its own.
			// The declared icon is back: draw it before reading the tile.
			settle()
			inner, edge := tile(subject.Name)
			if vivid(edge) == 0 {
				t.Error("a glyph's tile has no tint")
			}
			// The pointer over the card moves nothing in its icon: the page
			// comes to rest (settle) and the tile keeps its pixels.
			pose := func() string {
				img, box := area(inner)
				var pixels []byte
				for y := box.Min.Y; y < box.Max.Y; y++ {
					pixels = append(pixels, img.Pix[img.PixOffset(box.Min.X, y):img.PixOffset(box.Max.X, y)]...)
				}
				return string(pixels)
			}
			rest := pose()
			tt.Move(inner.X+140, inner.Y+10)
			settle()
			if pose() != rest {
				t.Error("the icon of a hovered card is not its icon at rest")
			}
			tt.Move(2, 2)
			if mark(subject.Name) != drawn {
				t.Error("the icon changed after the pointer left its card")
			}
			// A brand's package has its mark on a neutral tile: in the
			// brand's colors, or in the foreground color for a mark without.
			for _, sample := range []struct {
				icon  string
				vivid bool
			}{{"ailogo:figma", true}, {"ailogo:github", false}, {"ailogo:missing-brand", false}} {
				brand.Icon = sample.icon
				settle()
				inner, edge := tile(brand.Name)
				if unknown := sample.icon == "ailogo:missing-brand"; (vivid(edge) != 0) != unknown {
					t.Errorf("%s: the tile's tint is %d pixels", sample.icon, vivid(edge))
				} else if !unknown && (vivid(inner) >= 20) != sample.vivid {
					t.Errorf("%s: the mark has %d pixels in a color of its own", sample.icon, vivid(inner))
				}
			}
			brand.Icon = "ailogo:figma"
		}
	})
	// A details dialog's Close, pressed with the pointer, closes it.
	t.Run("details close", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		tt := ui.NewTesterWith(func(c *ui.Context) {
			dictation(c, m)
			libraryDialogs(c, m)
		}, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		clickButton(t, tt, i18n.In("en", "knowledge_model_details"))
		if m.info == nil {
			t.Fatal("a model's Details did not open its dialog")
		}
		tt.Advance(time.Second)
		clickButton(t, tt, i18n.In("en", "close"))
		if m.info != nil {
			t.Error("Close left the model's details open")
		}
		tt.Advance(time.Second)
		m.library.info = m.Library[0].ID
		tt.Frame()
		tt.Advance(time.Second)
		clickButton(t, tt, i18n.In("en", "close"))
		if m.library.info != "" {
			t.Error("Close left the library model's details open")
		}
	})
	// A button whose operation runs is busy and takes no second press; the
	// operation's end gives it back.
	t.Run("pending buttons", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		m.Live = true
		s := &State{Model: m}
		sent := map[string]int{}
		start := func(op string) { sent[op]++; m.SetBusy(op, true) }
		m.ProviderActions = ProviderActions{RefreshCatalog: func() { start(BusyCatalog) }}
		m.PackageActions = &PackageActions{CheckUpdates: func(kind viewmodel.PackageKind) {
			if kind == preview.KindSkill {
				start(BusySkillUpdates)
			} else {
				start(BusyPluginUpdates)
			}
		}}
		m.AppearanceActions = &AppearanceActions{CheckUpdates: func() { start(BusyThemes) }}
		m.PermissionActions = &PermissionActions{Check: func() { start(BusyPermissions) }}
		m.ResetTelemetry = func() { m.Telemetry.Pending = true; start(BusyTelemetry) }
		m.SendDiagnostics = func() { m.Telemetry.Pending = true; start(BusyDiagnostics) }
		var dark theme.Mode
		var name string
		var effects bool
		var surfaces theme.Surfaces
		data := preview.Load()
		for _, x := range []struct {
			section   Section
			label, op string
		}{
			{Providers, "provider_catalog_update", BusyCatalog},
			{Plugins, "plugins_check_updates", BusyPluginUpdates},
			{Skills, "plugins_check_updates", BusySkillUpdates},
			{Appearance, "plugins_check_updates", BusyThemes},
			{Permissions, "permission_check", BusyPermissions},
			{General, "telemetry_reset_action", BusyTelemetry},
			{General, "telemetry_send_action", BusyDiagnostics},
		} {
			s.Section = x.section
			tt := ui.NewTesterWith(func(c *ui.Context) {
				ui.Scroll(c).Fill().Children(func() { Page(c, s, data, Look{Theme: &name, Mode: &dark, Effects: &effects, Surfaces: &surfaces}) })
			}, 1100, 2400, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
			tt.SetLocale("en")
			tt.Frame()
			label := i18n.In("en", x.label)
			if b := pressButton(t, tt, label); !b.Busy || !b.Disabled || sent[x.op] != 1 {
				t.Errorf("%s: the pressed %s is busy %v, disabled %v after %d requests", x.section, label, b.Busy, b.Disabled, sent[x.op])
			}
			if pressButton(t, tt, label); sent[x.op] != 1 {
				t.Errorf("%s: the busy %s started %d operations", x.section, label, sent[x.op])
			}
			m.SetBusy(x.op, false)
			m.Telemetry.Pending = false
			tt.Frame()
			if b := findButton(t, tt, label); b.Busy || b.Disabled {
				t.Errorf("%s: the finished %s stayed busy %v, disabled %v", x.section, label, b.Busy, b.Disabled)
			}
		}
		page := func(section Section) *ui.Tester {
			s.Section = section
			tt := ui.NewTesterWith(func(c *ui.Context) {
				ui.Scroll(c).Fill().Children(func() { Page(c, s, data, Look{Theme: &name, Mode: &dark, Effects: &effects, Surfaces: &surfaces}) })
			}, 1100, 2400, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
			tt.SetLocale("en")
			tt.Frame()
			return tt
		}
		count := func(tt *ui.Tester, match func(ui.AccessInfo) bool) (n int) {
			for _, x := range tt.AccessAll() {
				if match(x) {
					n++
				}
			}
			return n
		}
		// A plugin's update under way puts the spinner in its card's action,
		// holds its switch and makes Update all busy; nothing is sent twice.
		var updated []string
		m.PackageActions.Update = func(ids []string) {
			for _, id := range ids {
				updated = append(updated, id)
				m.SetBusy(BusyPackage+id, true)
			}
		}
		m.checked = true
		tt := page(Plugins)
		update, all := i18n.In("en", "plugins_update_available"), i18n.In("en", "plugins_update_all")
		offered := func(x ui.AccessInfo) bool { return x.Role == ui.RoleButton && x.Name == update }
		before := count(tt, offered)
		if before == 0 {
			t.Fatalf("no plugin offers an update: %q", tt.Texts())
		}
		clickButton(t, tt, update)
		if len(updated) != 1 || count(tt, offered) != before-1 {
			t.Fatalf("a card's update sent %q and left %d of %d update buttons", updated, count(tt, offered), before)
		}
		var plugin string
		for _, x := range m.Packages {
			if x.ID == updated[0] {
				plugin = x.Name
			}
		}
		if count(tt, func(x ui.AccessInfo) bool { return x.Role == ui.RoleButton && x.Name == plugin && x.Busy && x.Disabled }) != 1 {
			t.Errorf("the updating %s shows no busy action", plugin)
		}
		if count(tt, func(x ui.AccessInfo) bool {
			return x.Name == i18n.In("en", "plugins_enabled")+" "+plugin && x.Disabled
		}) != 1 {
			t.Errorf("the updating %s kept its switch", plugin)
		}
		if b := pressButton(t, tt, all); !b.Busy || !b.Disabled || len(updated) != 1 {
			t.Errorf("Update all beside a running update is busy %v, disabled %v and sent %q", b.Busy, b.Disabled, updated)
		}
		m.SetBusy(BusyPackage+updated[0], false)
		tt.Frame()
		if b := findButton(t, tt, all); b.Busy || b.Disabled || count(tt, offered) != before {
			t.Errorf("the finished update left Update all busy %v, disabled %v, %d update buttons", b.Busy, b.Disabled, count(tt, offered))
		}
		// A backup that runs: its action is busy and takes no second press.
		m.Backup.Saved = BackupTarget{Tab: 2, Directory: "/backups", PassphraseSet: true}
		m.BackupActions = &BackupActions{Now: func() { start(BusyBackup + "now"); m.Backup.Pending = true }}
		tt = page(Backup)
		now := i18n.In("en", "backup_now")
		if b := pressButton(t, tt, now); !b.Busy || !b.Disabled || sent[BusyBackup+"now"] != 1 {
			t.Errorf("the pressed backup is busy %v, disabled %v after %d requests", b.Busy, b.Disabled, sent[BusyBackup+"now"])
		}
		if pressButton(t, tt, now); sent[BusyBackup+"now"] != 1 {
			t.Errorf("the busy backup started %d operations", sent[BusyBackup+"now"])
		}
		m.SetBusy(BusyBackup+"now", false)
		m.Backup.Pending = false
		tt.Frame()
		if b := findButton(t, tt, now); b.Busy || b.Disabled {
			t.Errorf("the finished backup stayed busy %v, disabled %v", b.Busy, b.Disabled)
		}
	})
	// A list row with no action is not a button and takes no hover style;
	// one with an action is, and does.
	t.Run("plain rows", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		role := m.Roles[0]
		m.teamEdit = teamEditor{open: true, def: true, team: preview.Team{Name: "Default", Members: []string{role.ID}}}
		var clicked bool
		tt := ui.NewTesterWith(func(c *ui.Context) {
			ui.Column(c).Width(400).Children(func() { listRow{onClick: func() { clicked = true }}.row(c, "action", "Action") })
			teamDialog(c, m)
		}, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		tt.Advance(time.Second)
		for _, x := range tt.AccessAll() {
			if x.Role == ui.RoleButton && x.Name == role.Name {
				t.Errorf("the Default team's member row is a button: %+v", x)
			}
		}
		box, ok := tt.Find(role.Name)
		if !ok {
			t.Fatalf("no member row: %q", tt.Texts())
		}
		tt.Move(2, 890)
		tt.Frame()
		idle := bytes.Clone(tt.Image().Pix)
		tt.Move(box.X+box.W+40, box.Y+box.H/2)
		tt.Frame()
		tt.Advance(time.Second)
		if !bytes.Equal(idle, tt.Image().Pix) {
			t.Error("a member row with no action changed under the pointer")
		}
		m.teamEdit = teamEditor{}
		tt.Move(2, 890)
		tt.Frame()
		tt.Advance(time.Second)
		idle = bytes.Clone(tt.Image().Pix)
		b := findButton(t, tt, "Action")
		tt.Move(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
		tt.Frame()
		tt.Advance(time.Second)
		if bytes.Equal(idle, tt.Image().Pix) {
			t.Error("a row with an action shows no hover style")
		}
		if clickButton(t, tt, "Action"); !clicked {
			t.Error("a row's action did not run")
		}
	})
}

// findButton is the lowest button of that name.
func findButton(t *testing.T, tt *ui.Tester, name string) (found ui.AccessInfo) {
	t.Helper()
	for _, x := range tt.AccessAll() {
		if x.Role == ui.RoleButton && x.Name == name && x.Bounds.Y >= found.Bounds.Y {
			found = x
		}
	}
	if found.Name == "" {
		t.Fatalf("no %q button: %q", name, tt.Texts())
	}
	return found
}

// clickButton presses that button with the pointer.
func clickButton(t *testing.T, tt *ui.Tester, name string) {
	t.Helper()
	b := findButton(t, tt, name)
	tt.ClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
	tt.Frame()
}

// pressButton presses that button with the pointer and returns it as the
// next frame shows it.
func pressButton(t *testing.T, tt *ui.Tester, name string) ui.AccessInfo {
	t.Helper()
	clickButton(t, tt, name)
	return findButton(t, tt, name)
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
