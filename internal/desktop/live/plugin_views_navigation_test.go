package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestHasPluginSettingsContent(t *testing.T) {
	info := protocol.DefaultPluginInfo()
	if hasPluginSettingsContent(info) {
		t.Fatal("plugin without an extension has a settings page")
	}

	extension := protocol.DefaultPluginExtension()
	page := protocol.DefaultPluginDesktopSettings()
	extension.SettingsPage = &page
	info.Extension = &extension
	if hasPluginSettingsContent(info) {
		t.Fatal("navigation-only settings page was included")
	}

	page.EnabledControl = true
	if !hasPluginSettingsContent(info) {
		t.Fatal("enabled control was omitted")
	}
	page.EnabledControl = false
	entry := "dev.sailry.platform/desktop/settings.js"
	page.Entry = &entry
	if !hasPluginSettingsContent(info) {
		t.Fatal("custom settings page was omitted")
	}
	page.Entry = nil
	info.Settings = new(protocol.PluginSettingsSchema)
	if !hasPluginSettingsContent(info) {
		t.Fatal("schema settings page was omitted")
	}
}
