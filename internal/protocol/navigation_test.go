package protocol

import (
	"strings"
	"testing"
)

func TestNavigationValidation(t *testing.T) {
	t.Parallel()
	for _, candidate := range []struct {
		name string
		icon PluginDesktopIcon
		want bool
	}{
		{"catalog", PluginDesktopIcon{Kind: "Name", Data: "reicon:" + navigationIcons[0]}, true},
		{"unknown", PluginDesktopIcon{Kind: "Name", Data: "reicon:missing"}, false},
		{"phosphor", PluginDesktopIcon{Kind: "Name", Data: "phosphor:database"}, true},
		{"phosphor_first", PluginDesktopIcon{Kind: "Name", Data: "phosphor:" + phosphorIcons[0]}, true},
		{"phosphor_last", PluginDesktopIcon{Kind: "Name", Data: "phosphor:" + phosphorIcons[len(phosphorIcons)-1]}, true},
		{"unknown_phosphor", PluginDesktopIcon{Kind: "Name", Data: "phosphor:missing"}, false},
		{"phosphor_weight", PluginDesktopIcon{Kind: "Name", Data: "phosphor:database-fill"}, false},
		{"shape", PluginDesktopIcon{Kind: "Svg", Data: PluginDesktopIconSvgPayload{Svg: `<svg viewBox="0 0 24 24"><path d="M4 12h16"/></svg>`}}, true},
		{"script", PluginDesktopIcon{Kind: "Svg", Data: PluginDesktopIconSvgPayload{Svg: `<svg viewBox="0 0 24 24"><script/></svg>`}}, false},
		{"external", PluginDesktopIcon{Kind: "Svg", Data: PluginDesktopIconSvgPayload{Svg: `<svg viewBox="0 0 24 24"><path fill="url(https://example.invalid)"/></svg>`}}, false},
		{"invalid_extent", PluginDesktopIcon{Kind: "Svg", Data: PluginDesktopIconSvgPayload{Svg: `<svg viewBox="0 0 NaN 24"><path/></svg>`}}, false},
		{"trailing_text", PluginDesktopIcon{Kind: "Svg", Data: PluginDesktopIconSvgPayload{Svg: `<svg viewBox="0 0 24 24"><path/></svg>text`}}, false},
	} {
		t.Run(candidate.name, func(t *testing.T) {
			navigation := PluginDesktopNavigation{Label: "Relay", Icon: &candidate.icon}
			if navigation.Valid() != candidate.want {
				t.Fatal("navigation validation changed the captured catalog rule")
			}
		})
	}
	t.Run("desktop bundles", func(t *testing.T) {
		for _, scenario := range []struct {
			name   string
			change func(*PluginDesktopBundle)
			valid  bool
		}{
			{"complete", func(*PluginDesktopBundle) {}, true},
			{"missing entry", func(bundle *PluginDesktopBundle) { bundle.Entry = "missing.js" }, false},
			{"case collision", func(bundle *PluginDesktopBundle) { bundle.Files["DEV/main.js"] = "export default 2" }, false},
			{"reserved device", func(bundle *PluginDesktopBundle) { bundle.Files["dev/CON.js"] = "private" }, false},
			{"reserved device extension", func(bundle *PluginDesktopBundle) { bundle.Files["dev/lpt¹.txt"] = "private" }, false},
			{"parent segment", func(bundle *PluginDesktopBundle) { bundle.Files["dev/../other.js"] = "private" }, false},
			{"absolute", func(bundle *PluginDesktopBundle) { bundle.Files["/other.js"] = "private" }, false},
			{"win32 alias", func(bundle *PluginDesktopBundle) { bundle.Files["dev/name .txt"] = "valid" }, true},
			{"trailing dot", func(bundle *PluginDesktopBundle) { bundle.Files["dev/name."] = "private" }, false},
			{"text limit", func(bundle *PluginDesktopBundle) {
				bundle.Files["dev/main.js"] = strings.Repeat("x", PluginDesktopMaxBytes+1)
			}, false},
			{"image path", func(bundle *PluginDesktopBundle) { bundle.Images["dev/image.jpg"] = "AAAA" }, false},
			{"empty image", func(bundle *PluginDesktopBundle) { bundle.Images["dev/image.png"] = "" }, false},
			{"image limit", func(bundle *PluginDesktopBundle) {
				bundle.Images["dev/image.png"] = strings.Repeat("A", PluginDesktopMaxImageBytes+4)
			}, false},
			{"png text", func(bundle *PluginDesktopBundle) { bundle.Files["dev/image.png"] = "AAAA" }, false},
			{"encoded image", func(bundle *PluginDesktopBundle) { bundle.Images["dev/image.png"] = "AAAA" }, true},
			{"file limit", func(bundle *PluginDesktopBundle) {
				for range PluginDesktopMaxFiles {
					bundle.Files["dev/"+strings.Repeat("a", len(bundle.Files))+".js"] = "export default 1"
				}
			}, false},
		} {
			t.Run(scenario.name, func(t *testing.T) {
				bundle := DefaultPluginDesktopBundle()
				bundle.Entry = "dev/main.js"
				bundle.Files[bundle.Entry] = "export default 1"
				scenario.change(&bundle)
				if err := ValidatePluginDesktopBundle(bundle); (err == nil) != scenario.valid {
					t.Fatalf("desktop resource contract: %v", err)
				}
			})
		}
	})
	t.Run("desktop declarations", func(t *testing.T) {
		for _, scenario := range []struct {
			name   string
			change func(*PluginDesktopManifest)
			valid  bool
		}{
			{"headless declarations", func(*PluginDesktopManifest) {}, true},
			{"missing contribution", func(manifest *PluginDesktopManifest) { manifest.UiOverlay = true }, false},
			{"contribution entry", func(manifest *PluginDesktopManifest) {
				entry := "dev/controller.js"
				manifest.UiEntry, manifest.Resources = &entry, []string{entry}
			}, true},
			{"undeclared entry", func(manifest *PluginDesktopManifest) {
				entry := "dev/controller.js"
				manifest.Entry = &entry
			}, false},
			{"wrong navigation surface", func(manifest *PluginDesktopManifest) {
				manifest.NavigationOptions.Surface.Kind = "composer"
			}, false},
			{"assistant duplicate", func(manifest *PluginDesktopManifest) {
				manifest.Conversations = []PluginConversationDeclaration{{ID: "same"}, {ID: "same"}}
			}, false},
		} {
			t.Run(scenario.name, func(t *testing.T) {
				manifest := DefaultPluginDesktopManifest()
				scenario.change(&manifest)
				if err := ValidatePluginDesktopManifest(manifest); (err == nil) != scenario.valid {
					t.Fatalf("desktop declaration contract: %v", err)
				}
			})
		}
	})
}
