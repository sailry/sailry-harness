package settings

import (
	"bytes"
	"fmt"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestManagedNodeInSettingsHeader(t *testing.T) {
	state := NewLive()
	body := func(c *ui.Context, _ *State) { ui.Text(c, "Native page body") }
	state.SetNativePages([]NativePage{
		{Section: Knowledge, Group: "settings_group_ai", Render: body},
		{Section: Terminal, Group: "settings_group_tools", Render: body},
		{Section: Hooks, Group: "settings_group_tools", Render: body},
	})
	state.ProviderActions.Hosts = func() []ProviderHost {
		return []ProviderHost{
			{Node: "local", Local: true, Connected: true},
			{Node: "remote", Name: "Remote", Connected: true},
		}
	}
	state.PermissionActions = &PermissionActions{}
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		Page(c, state, nil, Look{})
	}, 940, 700)
	for _, lang := range []string{"en", "zh-CN"} {
		tester.SetLocale(lang)
		for _, section := range []Section{General, Conversation, Knowledge, Terminal, Hooks, Permissions} {
			state.Open(section)
			tester.Frame()
			title, titled := tester.AccessByRole(ui.RoleText, i18n.In(lang, string(section)))
			description, described := tester.AccessByRole(ui.RoleText, i18n.In(lang, string(section)+"_description"))
			control, selectable := tester.AccessByRole(ui.RoleMenuButton, i18n.In(lang, "project_host"))
			if !titled || !described || !selectable {
				t.Fatalf("%s %s has no complete header: %q", lang, section, tester.Texts())
			}
			middle := control.Bounds.Y + control.Bounds.H/2
			if control.Bounds.X <= title.Bounds.X || middle < title.Bounds.Y || middle > description.Bounds.Y+description.Bounds.H {
				t.Errorf("%s %s: host control is outside the page header: %v", lang, section, control.Bounds)
			}
		}
	}
}

func TestConversationCustomPrompt(t *testing.T) {
	state := NewLive()
	state.Open(Conversation)
	editor := &PersonalInstructions{Known: true}
	state.PersonalInstructions = editor
	var submitted string
	state.EditPersonalInstructions = func() { submitted = editor.Draft }
	state.CodeIndex = stored(true)
	state.SavePersistentCodeIndex = func(enabled bool) { state.PersistentCodeIndex = new(enabled) }
	state.PersistentCodeIndex = new(false)
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		SettleEdits(c, state.Model)
		Page(c, state, nil, Look{})
	}, 920, 800)
	tester.SetLocale("en")
	tester.Frame()
	toggle, ok := tester.AccessByRole(ui.RoleSwitch, i18n.In("en", "settings_code_index_persistent"))
	if !ok || toggle.Disabled {
		t.Fatal("full code index switch unavailable")
	}
	if _, ok := tester.AccessByRole(ui.RoleMenuButton, "Project"); ok {
		t.Fatal("full code index still uses a project selector")
	}
	tester.ClickAt(toggle.Bounds.X+10, toggle.Bounds.Y+10)
	tester.Frame()
	if !*state.PersistentCodeIndex {
		t.Fatal("full code index switch did not save")
	}
	if _, ok := tester.AccessByRole(ui.RoleButton, i18n.In("en", "settings_save")); ok {
		t.Fatal("custom prompt still has a Save button")
	}
	field, ok := tester.AccessByRole(ui.RoleTextField, i18n.In("en", "settings_custom_prompt"))
	if !ok || field.Disabled {
		t.Fatal("custom prompt field unavailable")
	}
	tester.ClickAt(field.Bounds.X+10, field.Bounds.Y+10)
	tester.Type("Use the project formatter.")
	tester.Frame()
	tester.Frame()
	// Typing keeps the draft; leaving the editor saves it.
	if submitted != "" {
		t.Fatalf("submitted %q while editing", submitted)
	}
	tester.ClickAt(toggle.Bounds.X+10, toggle.Bounds.Y+10)
	tester.Frame()
	tester.Frame()
	if submitted != "Use the project formatter." {
		t.Fatalf("submitted %q", submitted)
	}
}

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
	t.Run("a language choice has no toast", func(t *testing.T) {
		m := New().Model
		var toasts []string
		tt := ui.NewTesterWith(func(c *ui.Context) {
			general(c, m)
			ui.ToastViewportBase(c, func(_ ui.Element, shown []ui.Toast) {
				toasts = toasts[:0]
				for _, toast := range shown {
					toasts = append(toasts, toast.Title)
				}
			})
		}, 900, 1600, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		click := func(role ui.Role, name string) {
			t.Helper()
			for _, x := range tt.AccessAll() {
				if (x.Role == role || role == ui.RoleMenuItem && (x.Role == ui.RoleMenuItemRadio || x.Role == ui.RoleMenuItemCheckBox)) && x.Name == name {
					tt.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
					tt.Frame()
					tt.Advance(time.Second)
					return
				}
			}
			t.Fatalf("no %q among %q", name, tt.Texts())
		}
		click(ui.RoleMenuButton, i18n.In("en", "settings_language"))
		click(ui.RoleMenuItem, i18n.In("en", Languages[2]))
		tt.Frame()
		if m.Language != 2 || len(toasts) != 0 {
			t.Fatalf("language %d, toasts %q", m.Language, toasts)
		}
	})
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

// Plugin classification, configuration checks and market actions.
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
		m.library.open, m.library.info = true, m.Library[0].ID
		tt.Frame()
		tt.Advance(time.Second)
		clickButton(t, tt, i18n.In("en", "settings_cancel"))
		if m.library.info != "" {
			t.Error("Cancel left the library model's details open")
		}
	})
	// An update that asks for more than the installed version has says so,
	// in the review and on the details' newest version.
	t.Run("update permissions", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		s := &State{Model: m}
		var pk *preview.Package
		for i := range m.Packages {
			if m.Packages[i].Installed && Update(&m.Packages[i]) {
				pk = &m.Packages[i]
				break
			}
		}
		if pk == nil {
			t.Fatal("the preview has no package with an update")
		}
		pk.AddedPermissions = []string{"market_permission_artifacts_write"}
		want := i18n.In("en", "market_details_added_permissions") + ": " + i18n.In("en", "market_permission_artifacts_write")
		m.updates = pluginUpdates{open: true, chosen: map[string]bool{}}
		d := &Detail{Kind: DetailPlugin, ID: pk.ID, Tab: 3}
		tt := ui.NewTesterWith(func(c *ui.Context) {
			if m.updates.open {
				pluginUpdatesDialog(c, m)
			} else {
				pluginDetails(c, s, pk, d)
			}
		}, 900, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		if !tt.HasText(want) {
			t.Errorf("the review lacks %q: %q", want, tt.Texts())
		}
		m.updates.open = false
		tt.Frame()
		tt.Advance(time.Second)
		if !tt.HasText(want) {
			t.Errorf("the details lack %q: %q", want, tt.Texts())
		}
		pk.AddedPermissions = nil
		tt.Frame()
		if tt.HasText(i18n.In("en", "market_details_added_permissions")) {
			t.Error("an update without new permissions names some")
		}
	})
	// Dictation opens the library for speech models, as the other model
	// pages do for theirs; the library's details install the version they
	// show with the prefixes typed, and a managed model's details update
	// and, after the confirmation, delete it.
	t.Run("model library", func(t *testing.T) {
		en := func(key string) string { return i18n.In("en", key) }
		m := NewModel(preview.SettingsData())
		m.Live = true
		var calls []string
		m.ModelActions = &ModelActions{
			LibraryInstall: func(id string, choice *LibraryChoice) {
				if choice == nil {
					calls = append(calls, "install "+id)
				} else {
					calls = append(calls, "install "+id+" "+choice.QueryPrefix+"|"+choice.DocumentPrefix)
				}
			},
			LibraryDetails: func(id string) { calls = append(calls, "details "+id) },
			LibraryVersion: func(id string, version int) { calls = append(calls, fmt.Sprint("version ", id, " ", version)) },
			LibraryMore:    func() { calls = append(calls, "more"); m.LibraryMore = false },
			Update:         func(kind, id string) { calls = append(calls, "update "+kind+" "+id) },
			Delete:         func(kind, id string) { calls = append(calls, "delete "+kind+" "+id) },
			Cancel:         func(kind, id string) { calls = append(calls, "cancel "+kind+" "+id) },
		}
		tt := ui.NewTesterWith(func(c *ui.Context) {
			m.library.open = false
			dictation(c, m)
			libraryDialogs(c, m)
		}, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		m.SpeechTab = 1
		tt.Frame()
		if !m.library.open || m.library.kind != ModelsSpeech {
			t.Fatalf("Available opened %+v", m.library)
		}
		if _, ok := tt.AccessByRole(ui.RoleButton, en("model_library_manage")); ok {
			t.Error("the page still offers Manage models")
		}
		m.Library = []preview.LibraryModel{{ID: "hub/a", Name: "Embedder", Repo: "org/a", Kind: ModelsEmbedding, Source: "model_library_huggingface",
			Size: "1.0 MiB", License: "mit", Versions: []string{"main", "v1"}, Resolved: true, QueryPrefix: "query: "}}
		m.LibraryLoad, m.LibraryMore = preview.Fetch{Loaded: true}, true
		// The rows arrive, the page lays them out, and the page not
		// filling its height reads the next page once.
		for range 3 {
			tt.Frame()
			tt.Advance(time.Second)
		}
		m.LibraryMore = false
		tt.Frame()
		if !tt.HasText("Embedder") || !tt.HasText("org/a · 1.0 MiB · mit") {
			t.Fatalf("Available shows no hub models: %q", tt.Texts())
		}
		var details ui.AccessInfo
		for _, x := range tt.AccessAll() {
			if x.Role == ui.RoleButton && x.Name == en("knowledge_model_details") && x.Bounds.Y > details.Bounds.Y {
				details = x // the hub's row is last
			}
		}
		tt.ClickAt(details.Bounds.X+details.Bounds.W/2, details.Bounds.Y+details.Bounds.H/2)
		tt.Frame()
		tt.Advance(time.Second)
		for _, name := range []string{"model_library_query_prefix", "model_library_document_prefix"} {
			if _, ok := tt.AccessByRole(ui.RoleTextField, en(name)); !ok {
				t.Errorf("an embedding model's details have no %s", name)
			}
		}
		if !tt.HasText(en("model_library_version")) || !tt.HasText("main") {
			t.Errorf("the details show no version: %q", tt.Texts())
		}
		for _, x := range tt.AccessAll() {
			// The row's icon button shares the name; the footer's is the text one.
			if x.Role == ui.RoleButton && x.Name == en("knowledge_install") && x.Bounds.W > 48 {
				tt.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
			}
		}
		tt.Frame()
		if want := []string{"more", "details hub/a", "install hub/a query: |"}; !slices.Equal(calls, want) {
			t.Fatalf("library calls = %q, want %q", calls, want)
		}
		// An unresolved version cannot be installed from the details.
		m.Library[0].Resolved = false
		tt.Frame()
		for _, x := range tt.AccessAll() {
			if x.Role == ui.RoleButton && x.Name == en("knowledge_install") && x.Bounds.W > 48 && !x.Disabled {
				t.Error("an unresolved model's Install takes a press")
			}
		}
		m.library = library{}
		calls = nil
		m.info = &modelInfo{"Speech", ModelsSpeech, preview.LocalModel{ID: "x", Repo: "org/speech", Status: preview.ModelInstalled, Removable: true, Version: "0123abc", License: "mit", Size: "2.0 MiB"}}
		tt.Frame()
		tt.Advance(time.Second)
		for _, want := range []string{"0123abc", "mit", "2.0 MiB", en("knowledge_installed")} {
			if !tt.HasText(want) {
				t.Errorf("a managed model's details lack %q: %q", want, tt.Texts())
			}
		}
		clickButton(t, tt, en("settings_update"))
		clickButton(t, tt, en("knowledge_delete"))
		if m.dropping == nil || len(calls) != 1 {
			t.Fatalf("Delete did not ask first: %q", calls)
		}
		tt.Advance(time.Second)
		clickButton(t, tt, en("knowledge_delete"))
		tt.Advance(time.Second)
		if want := []string{"update " + ModelsSpeech + " x", "delete " + ModelsSpeech + " x"}; !slices.Equal(calls, want) || m.dropping != nil {
			t.Fatalf("managed calls = %q, want %q", calls, want)
		}
		// The recommended model cannot be removed; a download can be canceled.
		calls = nil
		m.Speech[0].Status, m.Speech[0].Percent = preview.ModelDownloading, 40
		m.info = &modelInfo{"Recommended", ModelsSpeech, m.Speech[0]}
		tt.Frame()
		tt.Advance(time.Second)
		for _, x := range tt.AccessAll() {
			if x.Role == ui.RoleButton && (x.Name == en("knowledge_delete") || x.Name == en("settings_update")) {
				t.Errorf("a download offers %s", x.Name)
			}
		}
		clickButton(t, tt, en("settings_cancel"))
		if !slices.Equal(calls, []string{"cancel " + ModelsSpeech + " "}) {
			t.Fatalf("download calls = %q", calls)
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
		m.PackageActions = &PackageActions{CheckUpdates: func(viewmodel.PackageKind) { start(BusyPluginUpdates) }}
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
			{Plugins, "plugins_check_updates", BusyPluginUpdates},
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
		// The last backup's outcome is on the page: when it ended, what it
		// left out, and the problem of a failed one.
		shows := func(want ...string) {
			t.Helper()
			texts := strings.Join(tt.Texts(), "\n")
			for _, text := range want {
				if !strings.Contains(texts, text) {
					t.Errorf("the page does not show %q: %q", text, tt.Texts())
				}
			}
		}
		if strings.Contains(strings.Join(tt.Texts(), "\n"), i18n.In("en", "backup_last")) {
			t.Error("a Node that never backed up shows a last backup")
		}
		m.Backup.Saved.Last = BackupLast{At: time.Date(2026, 3, 7, 12, 0, 0, 0, time.Local), Skipped: 2}
		tt.Frame()
		shows(i18n.In("en", "backup_last"), "2026-03-07 12:00", i18n.In("en", "backup_last_skipped", "count", 2), i18n.In("en", "backup_last_done"))
		m.Backup.Saved.Last = BackupLast{At: time.Date(2026, 3, 8, 12, 0, 0, 0, time.Local), Problem: "backup_denied", Reason: "backup folder is not writable"}
		tt.Frame()
		shows("2026-03-08 12:00", i18n.In("en", "backup_denied"))
		// Keeping fewer backups deletes stored ones: it is confirmed once,
		// naming the consequence. Keeping all is saved at once.
		var kept []int
		m.BackupActions.Keep = func(keep int) { kept = append(kept, keep); m.Backup.Saved.Keep = keep }
		choose := func(label string) {
			t.Helper()
			for _, x := range tt.AccessAll() {
				if x.Role == ui.RoleMenuButton && x.Name == i18n.In("en", "backup_keep") {
					tt.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
					tt.Frame()
					tt.Advance(time.Second)
				}
			}
			for _, x := range tt.AccessAll() {
				if (x.Role == ui.RoleMenuItem || x.Role == ui.RoleMenuItemRadio || x.Role == ui.RoleMenuItemCheckBox) && x.Name == label {
					tt.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
					tt.Frame()
					tt.Advance(time.Second)
					return
				}
			}
			t.Fatalf("no retention %q: %q", label, tt.Texts())
		}
		choose(i18n.In("en", "backup_keep_last", "count", 7))
		if len(kept) != 0 {
			t.Fatalf("a retention that deletes backups was saved unconfirmed: %v", kept)
		}
		shows(i18n.In("en", "backup_keep_confirm", "count", 7), i18n.In("en", "backup_keep_consequence"))
		clickButton(t, tt, i18n.In("en", "settings_cancel"))
		if len(kept) != 0 {
			t.Fatalf("a cancelled retention was saved: %v", kept)
		}
		choose(i18n.In("en", "backup_keep_last", "count", 7))
		clickButton(t, tt, i18n.In("en", "settings_save"))
		if !slices.Equal(kept, []int{7}) {
			t.Fatalf("the confirmed retention saved %v", kept)
		}
		choose(i18n.In("en", "backup_keep_all"))
		if !slices.Equal(kept, []int{7, 0}) {
			t.Fatalf("keeping every backup saved %v", kept)
		}
		// A count the Node holds that the page does not offer is listed.
		m.Backup.Saved.Keep = 5
		tt.Frame()
		shows(i18n.In("en", "backup_keep_last", "count", 5))
		// A storage save the Node has not confirmed keeps the dialog's draft
		// as it was sent: Save reads Retry and sends it again. A save that
		// failed is Save again.
		var saves []BackupTarget
		var answer func(bool)
		m.BackupActions.Save = func(target BackupTarget, done func(bool)) { saves, answer = append(saves, target), done }
		clickButton(t, tt, i18n.In("en", "backup_configure"))
		clickButton(t, tt, i18n.In("en", "settings_save"))
		if len(saves) != 1 {
			t.Fatalf("the storage dialog saved %d times: %q", len(saves), tt.Texts())
		}
		m.BackupActions.Retry = true
		answer(false)
		tt.Frame()
		if _, ok := tt.AccessByRole(ui.RoleButton, i18n.In("en", "settings_save")); ok || findButton(t, tt, i18n.In("en", "plugins_retry")).Disabled {
			t.Fatalf("after an unconfirmed storage save: %q", tt.Texts())
		}
		if !findButton(t, tt, i18n.In("en", "backup_choose_directory")).Disabled {
			t.Error("the draft of an unconfirmed storage save can change")
		}
		clickButton(t, tt, i18n.In("en", "plugins_retry"))
		if len(saves) != 2 || saves[1] != saves[0] {
			t.Fatalf("Retry sent %+v", saves)
		}
		m.BackupActions.Retry = false
		answer(false)
		tt.Frame()
		if _, ok := tt.AccessByRole(ui.RoleButton, i18n.In("en", "plugins_retry")); ok || findButton(t, tt, i18n.In("en", "backup_choose_directory")).Disabled {
			t.Fatalf("after a storage save that failed: %q", tt.Texts())
		}
		clickButton(t, tt, i18n.In("en", "settings_save"))
		answer(true)
		tt.Frame()
		if len(saves) != 3 || m.backups.editing {
			t.Fatalf("a confirmed storage save left the dialog open after %d saves", len(saves))
		}
		// Cancelling the dialog of an unconfirmed save gives its request up
		// once; a confirmed or untouched dialog gives nothing up.
		forgotten := 0
		m.BackupActions.Forget = func() { forgotten++; m.BackupActions.Retry = false }
		clickButton(t, tt, i18n.In("en", "backup_configure"))
		clickButton(t, tt, i18n.In("en", "settings_cancel"))
		tt.Frame()
		if forgotten != 0 {
			t.Fatal("a dialog that saved nothing gave a request up")
		}
		clickButton(t, tt, i18n.In("en", "backup_configure"))
		clickButton(t, tt, i18n.In("en", "settings_save"))
		m.BackupActions.Retry = true
		answer(false)
		tt.Frame()
		clickButton(t, tt, i18n.In("en", "settings_cancel"))
		tt.Frame()
		tt.Frame()
		if forgotten != 1 || m.backups.editing {
			t.Fatalf("cancelling an unconfirmed storage save gave its request up %d times", forgotten)
		}
		clickButton(t, tt, i18n.In("en", "backup_configure"))
		if _, ok := tt.AccessByRole(ui.RoleButton, i18n.In("en", "plugins_retry")); ok || findButton(t, tt, i18n.In("en", "backup_choose_directory")).Disabled {
			t.Fatalf("a reopened storage dialog still retries: %q", tt.Texts())
		}
		clickButton(t, tt, i18n.In("en", "settings_cancel"))
		tt.Frame()
		// Deleting an archived session names its consequence once; one
		// without a project is told that its private workspace's files go
		// too, and one of a project is not.
		var deleted []string
		m.BackupActions.PrepareDelete = func(id string) func() { return func() { deleted = append(deleted, id) } }
		for _, x := range []viewmodel.Archived{{ID: "project", Title: "Project notes"}, {ID: "chat", Title: "Chat notes", Private: true}} {
			m.Archived, m.backups.archives = []viewmodel.Archived{x}, true
			tt.Frame()
			clickButton(t, tt, i18n.In("en", "backup_delete"))
			tt.Frame()
			key := "session_delete_named"
			if x.Private {
				key = "session_delete_private_named"
			}
			want := i18n.In("en", key, "name", x.Title)
			if texts := tt.Texts(); !slices.Contains(texts, i18n.In("en", "session_delete")) || !slices.Contains(texts, want) {
				t.Fatalf("deleting %q asks %q, want %q", x.Title, texts, want)
			}
			m.backups.archives = false
			tt.Frame()
			clickButton(t, tt, i18n.In("en", "backup_delete"))
			tt.Frame()
		}
		if !slices.Equal(deleted, []string{"project", "chat"}) {
			t.Fatalf("the confirmed deletions: %v", deleted)
		}
		// A restored profile is offered in place of the one in use; the
		// restart happens only when it is confirmed.
		var restarted []string
		m.BackupActions.Restart = func(target string) { restarted = append(restarted, target) }
		m.backups.restart, m.backups.restarting = "/profiles/restored", true
		tt.Frame()
		shows(i18n.In("en", "backup_restart_restored")+"?", i18n.In("en", "backup_restart_consequence"))
		clickButton(t, tt, i18n.In("en", "settings_cancel"))
		if len(restarted) != 0 || m.backups.restarting {
			t.Fatalf("a declined restart ran: %v", restarted)
		}
		m.backups.restarting = true
		tt.Frame()
		clickButton(t, tt, i18n.In("en", "updates_restart"))
		if !slices.Equal(restarted, []string{"/profiles/restored"}) || m.backups.restarting {
			t.Fatalf("the confirmed restart ran %v", restarted)
		}
	})
	// Action rows keep the button semantics and hover style.
	t.Run("action row", func(t *testing.T) {
		var clicked bool
		tt := ui.NewTesterWith(func(c *ui.Context) {
			ui.Column(c).Width(400).Children(func() { ListRow{OnClick: func() { clicked = true }}.Render(c, "action", "Action") })
		}, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Move(2, 890)
		tt.Frame()
		tt.Advance(time.Second)
		idle := bytes.Clone(tt.Image().Pix)
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

// TestExperimentalMemory asserts the memory map shows under its heading
// only while the runtime projects classes into it.
func TestExperimentalMemory(t *testing.T) {
	m := &Model{}
	tt := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		experimental(c, m)
	}, 900, 700, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
	tt.SetLocale("en")
	tt.Frame()
	if tt.HasText(i18n.In("en", "settings_ram")) {
		t.Fatal("the memory map shows with no classes")
	}
	m.Memory = []kit.TreemapGroup{{ID: "ram_heap", Label: "Heap", Detail: "64.0 MiB", Items: []kit.TreemapItem{{ID: "ram_objects", Label: "Live objects", Detail: "60.0 MiB", Value: 60 << 20}}}}
	for range 3 {
		tt.Frame()
	}
	for _, text := range []string{i18n.In("en", "settings_ram"), "Live objects", "64.0 MiB"} {
		if !tt.HasText(text) {
			t.Fatalf("missing %q: %q", text, tt.Texts())
		}
	}
}

// TestPackageConfigurationTabs asserts a package's settings show by tab,
// a tab's fields only while its Enabled switch is on, and an effort setting
// is chosen in its model's picker, among that model's levels, with no row of
// its own.
func TestPackageConfigurationTabs(t *testing.T) {
	m := &Model{Packages: []preview.Package{{ID: "trader", Name: "Trader"}}}
	m.OpenPackageConfiguration("trader", "", []preview.Setting{
		{Name: "player_name", Title: "Your name", Kind: "string"},
		{Name: "player_1_enabled", Title: "Enabled", Kind: "bool", Value: "false", Required: true},
		{Name: "player_1_model", Title: "Model", Kind: "model", Options: []string{"m1", "m2"}, Value: "m1"},
		{Name: "player_1_effort", Title: "Reasoning", Kind: "effort", Model: "player_1_model", Value: "none", Efforts: map[string][]string{"m1": {"low", "high"}}},
	}, viewmodel.SettingsTab{ID: "one", Title: "You", Fields: []string{"player_name"}},
		viewmodel.SettingsTab{ID: "two", Title: "Rival", Fields: []string{"player_1_enabled", "player_1_model", "player_1_effort"}, Enabled: "player_1_enabled"})
	tt := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		PackageConfigurationDialog(c, m)
	}, 900, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
	tt.SetLocale("en")
	frame := func() { tt.Frame(); tt.Frame() }
	frame()
	if !tt.HasText("You") || !tt.HasText("Rival") || !tt.HasText("Your name") || tt.HasText("Model") {
		t.Fatalf("the first tab does not show alone: %q", tt.Texts())
	}
	if tt.HasText(i18n.In("en", "plugins_settings_required")) {
		t.Fatal("a required hint shows before Save")
	}
	m.config.tab = 1
	frame()
	if !tt.HasText("Enabled") || tt.HasText("Model") {
		t.Fatalf("a disabled tab shows more than its switch: %q", tt.Texts())
	}
	m.config.fields[1].Value = "true"
	frame()
	if !tt.HasText("Model") || tt.HasText("Reasoning") {
		t.Fatalf("want the model's picker alone, no effort row: %q", tt.Texts())
	}
	// "none" is not a level m1 takes: the setting follows the model.
	if m.config.fields[3].Value != "" {
		t.Fatalf("an effort the model lacks stays: %q", m.config.fields[3].Value)
	}
	click := func(role ui.Role, name string) {
		t.Helper()
		target, ok := tt.AccessByRole(role, name)
		if !ok {
			t.Fatalf("no %q: %q", name, tt.Texts())
		}
		tt.ClickAt(target.Bounds.X+target.Bounds.W/2, target.Bounds.Y+target.Bounds.H/2)
		frame()
	}
	click(ui.RoleButton, "Model")
	click(ui.RoleOption, "m1")
	if !tt.HasText(i18n.In("en", "plugins_settings_follow_model")) {
		t.Fatalf("the picker offers no Follow model beside m1's levels: %q", tt.Texts())
	}
	click(ui.RoleOption, "high")
	if m.config.fields[2].Value != "m1" || m.config.fields[3].Value != "high" {
		t.Fatalf("the picker chose %q at %q, want m1 at high", m.config.fields[2].Value, m.config.fields[3].Value)
	}
}

// A model named by its repository shows the model over its owner, never the
// repository twice; known facts follow.
func TestModelText(t *testing.T) {
	for _, c := range []struct{ name, repo, size, title, detail string }{
		{"org/model-GGUF", "org/model-GGUF", "", "model-GGUF", "org"},
		{"", "org/model", "2.0 GB", "model", "org · 2.0 GB"},
		{"Whisper Small", "openai/whisper-small", "967 MB", "Whisper Small", "openai/whisper-small · 967 MB"},
		{"local", "local", "", "local", ""},
	} {
		if title, detail := modelText(c.name, c.repo, c.size); title != c.title || detail != c.detail {
			t.Errorf("modelText(%q, %q) = %q, %q", c.name, c.repo, title, detail)
		}
	}
}
