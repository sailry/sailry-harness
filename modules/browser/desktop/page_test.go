package desktop

import (
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestBrowserSettingsPageInChinese(t *testing.T) {
	const locale = "zh-CN"
	assembly := shell.Assemble(Register)
	catalog := i18n.New(assembly.Locales...)
	s := settings.NewLive()
	s.SetNativePages(assembly.SettingsPages)
	s.Open(settings.Browser)
	s.Browser = settings.BrowserSettings{Supported: true, Enabled: true, Persistent: true}
	var changes []bool
	scans := 0
	s.BrowserActions = &settings.BrowserActions{SetPersistent: func(value bool) { changes = append(changes, value) }, ScanChrome: func() { scans++ }}
	view := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, catalog)
		theme.Apply(c, "", theme.Dark)
		settings.Page(c, s, nil, settings.Look{})
	}, 900, 520, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	view.SetLocale(locale)
	view.Frame()
	if !view.HasText("从 Chrome 导入") {
		t.Fatalf("the native browser settings page did not show its import control: %q", view.Texts())
	}
	retainTitle := catalog.In(locale, "browser_data_local")
	if retain, ok := view.AccessByRole(ui.RoleSwitch, retainTitle); !ok || retain.Disabled {
		t.Fatalf("the persistence switch is missing or disabled: %q", view.Texts())
	}
	for _, text := range view.Texts() {
		if strings.Contains(text, "暂不支持") {
			t.Fatalf("the page reports an unsupported system: %q", view.Texts())
		}
	}
	if os.Getenv("SAILRY_UI_SHOTS") != "" {
		shots := filepath.Join("..", "..", "..", ".runtime", "ui-shots")
		if err := os.MkdirAll(shots, 0o755); err != nil {
			t.Fatal(err)
		}
		file, err := os.Create(filepath.Join(shots, "browser-settings-zh-CN.png"))
		if err != nil {
			t.Fatal(err)
		}
		defer file.Close()
		if err := png.Encode(file, view.Image()); err != nil {
			t.Fatal(err)
		}
	}

	s.Browser.ExternalStatus = "connections_offline"
	view.Frame()
	if _, ok := view.AccessByRole(ui.RoleSwitch, catalog.In(locale, "browser_external_enabled")); ok || !view.HasText(catalog.In(locale, "connections_offline")) {
		t.Fatalf("the offline Node still offered external Browser settings: %q", view.Texts())
	}
	click := func(role ui.Role, title string) {
		t.Helper()
		button, ok := view.AccessByRole(role, title)
		if !ok || button.Disabled {
			t.Fatalf("no enabled %q control: %q", title, view.Texts())
		}
		view.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
		view.Frame()
	}
	click(ui.RoleSwitch, retainTitle)
	click(ui.RoleButton, catalog.In(locale, "browser_import"))
	if !slices.Equal(changes, []bool{false}) || scans != 1 {
		t.Fatalf("offline Node blocked the window's Browser actions: changes %v, scans %d", changes, scans)
	}
}
