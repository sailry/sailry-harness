package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A package's brand that the collection has no mark for shows the mark the
// package declares, on the brand and on each of its sign-ins.
func TestPackageBrandMark(t *testing.T) {
	brand := "markless"
	entry := protocol.ConversationPresetsEntry{
		Source:  protocol.ConversationPresetsSource{Kind: "package"},
		Package: &protocol.PluginReference{Name: "markless"},
	}
	entry.Preset.ID, entry.Preset.Name, entry.Preset.Brand, entry.Preset.Offered = "markless-global", "Markless International", &brand, true
	entry.Preset.Auth.Kind = "plugin"
	entry.Preset.BrandDisplay = &protocol.PluginDesktopNavigation{Label: "Markless", Icon: &protocol.PluginDesktopIcon{
		Kind: "Svg", Data: protocol.PluginDesktopIconSvgPayload{Svg: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><circle cx="4" cy="4" r="4" fill="currentColor"/></svg>`},
	}}
	key := presetBrand(entry)
	if icons.Brand(key) != nil || icons.Brand(entry.Preset.ID) != nil {
		t.Fatal("the collection has a mark for the test brand")
	}
	r := &Runtime{App: &shell.App{Settings: settings.New()}}
	r.applyPresets(protocol.ConversationPresetsListing{Entries: []protocol.ConversationPresetsEntry{entry}})
	if icons.Brand(key) == nil || icons.BrandColor(key) == nil {
		t.Fatal("the brand does not show the mark its package declares")
	}
	if icons.Brand(entry.Preset.ID) != nil {
		t.Fatal("the declared mark was given to another identifier")
	}
	brands := r.App.Settings.Brands
	if len(brands) != 1 || brands[0].ID != key || brands[0].Name != "Markless" || len(brands[0].Methods) != 1 {
		t.Fatalf("brands = %+v", brands)
	}
	// The collection's own mark is not replaced by a declared one.
	before := icons.Brand("openai")
	icons.Declare("openai", icons.Brand(key))
	if icons.Brand("openai") != before {
		t.Fatal("a declared mark replaced the collection's")
	}
}
