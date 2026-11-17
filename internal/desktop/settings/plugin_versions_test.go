package settings

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

func TestOfficialPluginVersionHistory(t *testing.T) {
	pk := preview.Package{ID: "notes", Name: "Notes", Official: true, Installed: true, Pinned: true, Version: "1.2.0", MarketVersion: "2.0.0",
		Versions: []preview.PackageVersion{
			{Version: "2.0.0", Requires: "9.0.0", Notes: "Future release notes"},
			{Version: "1.3.0", Compatible: true, Notes: "Compatible release notes"},
			{Version: "1.2.0", Compatible: true, Notes: "Current release notes"},
			{Version: "1.1.0", Compatible: true, Notes: "Historical release notes"},
			{Version: "1.0.0", Compatible: true, Withdrawn: true, Notes: "Withdrawn release notes"},
			{Version: "0.9.0", Unsupported: "Unsupported plugin API v2"},
		}}
	s := New()
	var selectedID, selectedVersion string
	s.PackageActions = &PackageActions{PluginVersion: func(id, version string) { selectedID, selectedVersion = id, version }}
	tt := ui.NewTesterWith(func(c *ui.Context) { pluginVersions(c, s, &pk) }, 800, 1000, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	for _, text := range []string{"Future release notes", "Compatible release notes", "Current release notes", "Historical release notes", "Withdrawn release notes",
		i18n.In("en", "plugins_version_current"), i18n.In("en", "plugins_version_pinned"), i18n.In("en", "plugins_version_withdrawn"),
		i18n.In("en", "plugins_requires_sailry", "version", "9.0.0"), i18n.In("en", "plugins_version_unsupported", "reason", "Unsupported plugin API v2")} {
		if !tt.HasText(text) {
			t.Errorf("history lacks %q: %q", text, tt.Texts())
		}
	}
	action := i18n.In("en", "plugins_switch_version")
	for _, version := range []string{"2.0.0", "1.0.0", "0.9.0"} {
		if !findButton(t, tt, action+" "+version).Disabled {
			t.Errorf("version %s remains installable", version)
		}
	}
	if _, exists := tt.AccessByRole(ui.RoleButton, action+" 1.2.0"); exists {
		t.Error("the current version offers another switch")
	}
	if findButton(t, tt, action+" 1.1.0").Disabled {
		t.Fatal("pinning prevents an explicit historical switch")
	}
	clickButton(t, tt, action+" 1.1.0")
	if selectedID != "notes" || selectedVersion != "1.1.0" {
		t.Fatalf("historical switch selected %q at %q", selectedID, selectedVersion)
	}
	s.SetBusy(BusyPackage+pk.ID, true)
	tt.Frame()
	if button := findButton(t, tt, action+" 1.3.0"); !button.Disabled || !button.Busy {
		t.Error("a pending switch leaves another version active")
	}
	pk.Official = false
	tt.Frame()
	if _, exists := tt.AccessByRole(ui.RoleButton, action+" 1.1.0"); exists {
		t.Error("a third-party package offers an official historical switch")
	}
}

func TestPluginPinAndFollow(t *testing.T) {
	s := New()
	pk := preview.Package{ID: "notes", Name: "Notes", Version: "1.2.0", Official: true, Installed: true, Pinned: true, UpdateAvailable: true}
	s.Packages = []preview.Package{pk}
	var calls []bool
	s.PackageActions = &PackageActions{PinPlugin: func(id string, pinned bool) {
		if id != pk.ID {
			t.Errorf("pin targeted %q", id)
		}
		calls = append(calls, pinned)
	}}
	d := &Detail{Kind: DetailPlugin, ID: pk.ID}
	tt := ui.NewTesterWith(func(c *ui.Context) { pluginDetails(c, s, &pk, d) }, 800, 1000, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	if !tt.HasText(i18n.In("en", "plugins_fixed_version", "version", pk.Version)) || Update(&pk) {
		t.Fatal("a pinned package does not show its fixed version or still offers updates")
	}
	clickButton(t, tt, i18n.In("en", "plugins_resume_following"))
	if len(calls) != 1 || calls[0] {
		t.Fatalf("resume following sent %v", calls)
	}
	pk.Pinned = false
	tt.Frame()
	if !tt.HasText(i18n.In("en", "plugins_following_versions")) || !Update(&pk) {
		t.Fatal("following does not show its state or actionable Node update")
	}
	clickButton(t, tt, i18n.In("en", "plugins_pin_version"))
	if len(calls) != 2 || !calls[1] {
		t.Fatalf("pinning sent %v", calls)
	}
	pk.UpdateAvailable, pk.Latest = false, "9.0.0"
	if Update(&pk) {
		t.Error("catalog version metadata created an update without a Node recommendation")
	}
}
