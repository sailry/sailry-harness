package settings

import (
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
	// declares: its Kubai emblem in its tint, which moves only while the
	// pointer is over it, or its own SVG on a tile. Only a package that
	// declares none, or a name nothing draws, has the puzzle piece.
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
		// where is the box of the emblem: the leading 44 DIPs of the card's
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
		// mark is the pixels of the emblem.
		mark := func(title string) string {
			t.Helper()
			settle()
			at := where(title)
			img := tt.Image()
			scale := float32(img.Bounds().Dx()) / 900
			area := image.Rect(int(at.X*scale), int(at.Y*scale), int((at.X+at.W)*scale), int((at.Y+at.H)*scale))
			var pixels []byte
			for y := area.Min.Y; y < area.Max.Y; y++ {
				pixels = append(pixels, img.Pix[img.PixOffset(area.Min.X, y):img.PixOffset(area.Max.X, y)]...)
			}
			return string(pixels)
		}
		subject := &m.MarketPlugins[0]
		for i := range m.MarketPlugins {
			if m.MarketPlugins[i].ID == "files" {
				subject = &m.MarketPlugins[i]
			}
		}
		installed := m.installedPackage(subject.ID)
		if subject.Icon != "reicon:folders/folder" || installed == nil {
			t.Fatalf("the Files package declares %q, installed %v", subject.Icon, installed != nil)
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
				t.Errorf("%s: the emblem at rest changed between frames", step.name)
			}
			step.of.Icon = ""
			placeholder := mark(step.of.Name)
			if placeholder == drawn {
				t.Errorf("%s: the declared icon draws as the placeholder", step.name)
			}
			step.of.Icon = "reicon:missing/icon"
			if mark(step.of.Name) != placeholder {
				t.Errorf("%s: an unknown icon name does not fall back to the placeholder", step.name)
			}
			step.of.Icon = "kubai:rocket"
			if other := mark(step.of.Name); other == placeholder || other == drawn {
				t.Errorf("%s: a library icon named by its id is not drawn", step.name)
			}
			step.of.Icon = declared
			if index > 0 {
				continue
			}
			// The pointer over the card plays the motion, which asks for
			// frames only until it ends; with less motion it asks for none.
			at := where(subject.Name)
			at.X += 120
			tt.Move(at.X, at.Y+at.H/2)
			tt.Frame()
			if !tt.FramePending() {
				t.Error("the hovered emblem does not move")
			}
			if hovered := mark(subject.Name); hovered == drawn {
				t.Error("the emblem's hover pose is its rest pose")
			}
			save("market-icons-hover-zh-CN-dark")
			tt.Move(2, 2)
			if mark(subject.Name) != drawn {
				t.Error("the emblem did not return to rest")
			}
			tt.SetPreferences(ui.Preferences{ReduceTransparency: true, ReduceMotion: true})
			tt.Frame()
			tt.Move(at.X, at.Y+at.H/2)
			tt.Frame()
			if tt.FramePending() {
				t.Error("the emblem moves while the desktop asks for less motion")
			}
			tt.Move(2, 2)
			tt.SetPreferences(ui.Preferences{ReduceTransparency: true})
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
