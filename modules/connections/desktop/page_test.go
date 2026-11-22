package desktop

import (
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

var testCatalog = i18n.New(Locales)

func TestConnectionRelayLabelsOnlyShowMeasuredLatency(t *testing.T) {
	var relay int
	var address string
	millis := uint64(41)
	choices := []settings.RelayChoice{
		{LabelKey: "pairing_relay_direct"},
		{Label: "China (Tencent Cloud)", Millis: &millis},
		{LabelKey: "pairing_relay_custom", Custom: true},
		{Label: "Pending relay", Pending: true},
	}
	tt := ui.NewTester(func(c *ui.Context) {
		i18n.Bind(c, testCatalog)
		ui.Column(c).Fill().Padding(24).Children(func() {
			connectionRelayFields(c, &relay, &address, nil, choices, false, true)
		})
	}, 420, 320)
	tt.SetLocale("en")
	tt.Frame()
	if texts := tt.Texts(); !slices.Contains(texts, "Direct only (LAN)") {
		t.Fatalf("closed selector does not show the plain name: %q", texts)
	}
	trigger, ok := tt.AccessByRole(ui.RoleMenuButton, testCatalog.In("en", "pairing_relay_server"))
	if !ok {
		t.Fatal("connection server selector is missing")
	}
	tt.ClickAt(trigger.Bounds.X+trigger.Bounds.W/2, trigger.Bounds.Y+trigger.Bounds.H/2)
	tt.Advance(250 * time.Millisecond)
	if _, ok := tt.AccessByRole(ui.RoleMenuItemCheckBox, "Direct only (LAN)"); !ok {
		t.Errorf("selected menu option does not show the plain name: %q", tt.Texts())
	}
	for _, label := range []string{"China (Tencent Cloud) · 41 ms", "Custom", "Pending relay"} {
		if _, ok := tt.AccessByRole(ui.RoleMenuItem, label); !ok {
			t.Errorf("missing menu option %q: %q", label, tt.Texts())
		}
	}
}

func TestConnectionsDeviceMonitor(t *testing.T) {
	for _, device := range []viewmodel.Device{
		{ID: "local", Name: "Local computer", Execution: true, Local: true, Online: true},
		{ID: "remote", Name: "Remote computer", Execution: true},
		{ID: "phone", Name: "Mobile controller"},
	} {
		t.Run(device.ID, func(t *testing.T) {
			state := settings.New()
			state.Devices = []viewmodel.Device{device}
			inspected := ""
			state.ConnectionActions = &settings.ConnectionActions{Inspect: func(value viewmodel.Device) { inspected = value.ID }}
			tt := ui.NewTester(func(c *ui.Context) {
				i18n.Bind(c, testCatalog)
				ui.Column(c).Fill().Padding(24).Children(func() { Page().Render(c, state) })
			}, 900, 1000)
			tt.SetLocale("en")
			tt.Frame()
			info, ok := tt.AccessByRole(ui.RoleMenuButton, testCatalog.In("en", "more"))
			if !ok {
				t.Fatal("device has no More menu")
			}
			tt.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
			tt.Advance(250 * time.Millisecond)
			monitor, offered := tt.AccessByRole(ui.RoleMenuItem, testCatalog.In("en", "workspace_host"))
			if offered != device.Execution {
				t.Fatalf("monitor offered for execution=%v: %q", device.Execution, tt.Texts())
			}
			if !device.Execution {
				return
			}
			tt.ClickAt(monitor.Bounds.X+monitor.Bounds.W/2, monitor.Bounds.Y+monitor.Bounds.H/2)
			tt.Advance(250 * time.Millisecond)
			if inspected != device.ID {
				t.Fatalf("monitor targeted %q, want %q", inspected, device.ID)
			}
		})
	}
}

// Trusting a host's key leaves the connection pending; canceling the
// prompt ends it. A network save under way keeps its editor.
func TestConnectionPrompts(t *testing.T) {
	state := settings.New()
	m := state.Model
	tt := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, testCatalog)
		Page().Render(c, state)
	}, 900, 700, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	answer := func(key string) (confirmed bool) {
		t.Helper()
		m.ConnectionPending = true
		m.HostTrust = &settings.HostTrust{Host: "example.com:22", Algorithm: "ssh-ed25519", Fingerprint: "SHA256:abc", Confirm: func() { confirmed = true }}
		tt.Frame()
		tt.Advance(time.Second)
		button, ok := tt.AccessByRole(ui.RoleButton, testCatalog.In("en", key))
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
	m.NetworkEditor = &settings.NetworkEditor{Pending: true}
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
}

func TestConnectionRows(t *testing.T) {
	local, remote, mobile := strings.Repeat("325e9373", 8), strings.Repeat("c48e0d55", 8), strings.Repeat("5e6f7a8b", 8)
	state := settings.New()
	m := state.Model
	m.Devices = []viewmodel.Device{
		{ID: local, Name: "MacBook Pro", Platform: "macos", Execution: true, Online: true, Local: true, OS: "27.2", Version: "0.1.6"},
		{ID: remote, Name: "build-server", Platform: "linux", Execution: true, Online: true, Route: "p2p", RTT: 18, OS: "24.04", Version: "0.1.6"},
		{ID: strings.Repeat("a1b2c3d4", 8), Name: "Studio PC", Platform: "windows", Execution: true},
		{ID: mobile, Name: "iPhone", Platform: "ios", Online: true, Route: "relay", RTT: 64, Version: "0.1.6"},
	}
	tt := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, testCatalog)
		ui.Scroll(c).Fill().Children(func() { ui.Column(c).Padding(24, 24).Children(func() { Page().Render(c, state) }) })
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
	// A row's status is CrychicUI's ConnectionStatus, named by its
	// label: this computer, the route of a device that is reachable, and
	// offline, each once here.
	for _, key := range []string{"composer_host_local", "connection_route_p2p", "connection_route_relay", "connections_offline"} {
		label := testCatalog.In("zh-CN", key)
		status, ok := tt.AccessByRole(ui.RoleStatus, label)
		if text, shown := tt.Find(label); !ok || !shown || text.Y < status.Bounds.Y || text.Y+text.H > status.Bounds.Y+status.Bounds.H {
			t.Fatalf("no status %q holding its label: %+v, %q", label, status, tt.Texts())
		}
	}
	// For review: $SAILRY_SNAPSHOTS/settings-connections-status-<mode>.png at 2x.
	if os.Getenv("SAILRY_SNAPSHOTS") != "" {
		for _, mode := range []theme.Mode{theme.Light, theme.Dark} {
			sheet := ui.NewTesterWith(func(c *ui.Context) {
				i18n.Bind(c, testCatalog)
				theme.Apply(c, "", mode)
				ui.Column(c).Fill().Padding(24).Background(theme.Of(c).Background).Children(func() { Page().Render(c, state) })
			}, 760, 620, ui.TesterOptions{Dark: mode == theme.Dark, Preferences: ui.Preferences{ReduceTransparency: true}})
			sheet.SetLocale("en")
			sheet.SetScale(2)
			sheet.Frame()
			sheet.Advance(time.Second)
			sheet.Frame()
			name := map[theme.Mode]string{theme.Light: "light", theme.Dark: "dark"}[mode]
			snapshot(t, "settings-connections-status-"+name, sheet)
		}
	}
	snapshot(t, "connections-rows-zh-CN", tt)
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
	snapshot(t, "connections-rows-tooltip-zh-CN", tt)
	// Every row's native menu copies the whole ID; a Host's keeps Edit.
	for name, want := range map[string]struct {
		id   string
		menu []string
	}{
		"MacBook Pro": {local, []string{testCatalog.In("zh-CN", "connection_action_edit"), "复制 ID"}},
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
	if _, ok := tt.Access(testCatalog.In("zh-CN", "live_revoke")); ok {
		t.Fatal("an idle row shows a removal")
	}
	m.SetConnectionStatus(settings.ConnectionStatus{Revoking: map[string]bool{remote: true}})
	tt.Frame()
	if _, ok := tt.Access(testCatalog.In("zh-CN", "live_revoke")); !ok {
		t.Fatal("a row whose pairing is being removed does not show it")
	}
}

// The Add host dialog offers the methods this computer can act on, sends
// one step at a time, and checks an uncertain installation instead of
// sending another.
func TestAddHostFromSettings(t *testing.T) {
	system := hostOS
	t.Cleanup(func() { hostOS = system })
	hostOS = "linux"
	en := func(key string) string { return testCatalog.In("en", key) }
	state := settings.New()
	state.SetNativePages([]settings.NativePage{Page()})
	state.Open(settings.Connections)
	m := state.Model
	var added []settings.ConnectionDraft
	checks := 0
	m.ConnectionActions = &settings.ConnectionActions{
		Add: func(draft settings.ConnectionDraft, _ func(bool)) {
			added, m.ConnectionPending = append(added, draft), true
		},
		CheckStatus: func(func(bool)) { checks, m.ConnectionPending = checks+1, true },
	}
	tt := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, testCatalog)
		settings.Page(c, state, nil, settings.Look{})
	}, 900, 1000, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
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
	shot := func(name string) { snapshot(t, name, tt) }
	alpha, beta := settings.ProjectChoice{ID: "a", Name: "Alpha"}, settings.ProjectChoice{ID: "b", Name: "Beta"}
	state.AddHost()
	m.SetConnectionStatus(settings.ConnectionStatus{Projects: []settings.ProjectChoice{alpha, beta}})
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	if tt.HasText(en("host_wsl")) || !tt.HasText(en("host_pairing_mode")) || tt.HasText(en("pairing_relay_server")) {
		t.Fatalf("pairing mode or platform options: %q", tt.Texts())
	}
	// Pairing follows the invited Host's route and needs no local network
	// selection. SSH installation chooses a network and authentication.
	click(en("host_ssh_mode"))
	if !tt.HasText(en("pairing_relay_server")) || !tt.HasText(en("ssh_authentication")) {
		t.Fatalf("SSH installation fields: %q", tt.Texts())
	}
	click(en("host_pairing_mode"))
	if tt.HasText(en("pairing_relay_server")) || !tt.HasText(en("pairing_pin_hint")) {
		t.Fatalf("returning to pairing mode: %q", tt.Texts())
	}
	click(en("host_ssh_mode"))
	click(en("ssh_agent_short"))
	// The agent has no secret; a shared connection names its projects.
	if m.ConnectionDraft().Method != settings.ByAgent || tt.HasText(en("ssh_passphrase")) {
		t.Fatalf("agent method %d: %q", m.ConnectionDraft().Method, tt.Texts())
	}
	if !tt.HasText(en("host_ssh_agent_hint")) {
		t.Fatalf("SSH Agent is not explained as local SSH authentication: %q", tt.Texts())
	}
	click(en("connection_projects"))
	shot("connections-add-agent-en")
	click(en("host_install"))
	if len(added) != 0 {
		t.Fatalf("installed sharing with no project: %+v", added)
	}
	click(beta.Name)
	click(en("host_install"))
	if len(added) != 1 || added[0].Method != settings.ByAgent || added[0].Scope != settings.ScopeProjects || added[0].Projects != beta.ID {
		t.Fatalf("agent install: %+v", added)
	}
	// The step under way cannot start again, and locks its draft.
	click(en("host_install"))
	click(en("ssh_private_key"))
	if len(added) != 1 || m.ConnectionDraft().Method != settings.ByAgent {
		t.Fatalf("pending dialog acted: %d installs, method %d", len(added), m.ConnectionDraft().Method)
	}
	// An installation under way lists its steps.
	m.ConnectionStage = "host_install_installing"
	tt.Frame()
	for _, key := range []string{"host_step_connect", "host_install_detecting", "host_install_installing", "host_step_pair"} {
		if !tt.HasText(en(key)) {
			t.Fatalf("no %q step under way: %q", key, tt.Texts())
		}
	}
	shot("connections-add-progress-en")
	// A failed one says why in the dialog, until the next attempt.
	m.ConnectionPending, m.ConnectionStage = false, ""
	m.ConnectionFailure = &settings.ConnectionFailure{Tone: kit.Negative, Title: "host_install_failed", Detail: "permission denied (publickey)"}
	tt.Frame()
	if !slices.Contains(tt.Texts(), en("host_install_failed")+"\npermission denied (publickey)") || tt.HasText(en("host_step_connect")) {
		t.Fatalf("the failure is not in the dialog: %q", tt.Texts())
	}
	shot("connections-add-failed-en")
	click(en("host_install"))
	if m.ConnectionFailure != nil || len(added) != 2 {
		t.Fatalf("another attempt kept the failure %+v, %d installs", m.ConnectionFailure, len(added))
	}
	added = added[:1]
	// An uncertain installation is checked, never sent again.
	m.ConnectionPending = false
	m.SetConnectionStatus(settings.ConnectionStatus{Recovery: true, Projects: []settings.ProjectChoice{alpha, beta}})
	tt.Frame()
	if tt.HasText(en("host_install")) || !tt.HasText(en("host_check_status")) {
		t.Fatalf("uncertain installation: %q", tt.Texts())
	}
	shot("connections-add-check-en")
	click(en("ssh_private_key"))
	click(en("host_check_status"))
	click(en("host_check_status"))
	if checks != 1 || len(added) != 1 || m.ConnectionDraft().Method != settings.ByAgent {
		t.Fatalf("%d checks, %d installs, method %d", checks, len(added), m.ConnectionDraft().Method)
	}
	// A secret left blank keeps the one saved for the same method.
	m.ConnectionPending = false
	m.SetConnectionStatus(settings.ConnectionStatus{Saved: settings.ByPrivateKey})
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
	state.AddHost()
	m.SetConnectionStatus(settings.ConnectionStatus{DistributionsLoaded: true})
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
	m.SetConnectionStatus(settings.ConnectionStatus{DistributionsLoaded: true, Distributions: []string{"Ubuntu", "Debian"}})
	tt.Frame()
	shot("connections-add-wsl-en")
	click(en("host_install"))
	if len(added) != 2 || added[1].Method != settings.ByWSL || added[1].Distribution != "Ubuntu" {
		t.Fatalf("WSL install: %+v", added)
	}
}

func snapshot(t *testing.T, name string, tt *ui.Tester) {
	t.Helper()
	dir := os.Getenv("SAILRY_SNAPSHOTS")
	if dir == "" {
		return
	}
	file, err := os.Create(filepath.Join(dir, name+".png"))
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	if err := png.Encode(file, tt.Image()); err != nil {
		t.Fatal(err)
	}
}
