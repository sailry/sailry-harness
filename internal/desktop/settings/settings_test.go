package settings

import (
	"bytes"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
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
			providers(c, s)
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
