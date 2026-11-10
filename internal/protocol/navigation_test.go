package protocol

import "testing"

func TestNavigationValidation(t *testing.T) {
	t.Parallel()
	for _, candidate := range []struct {
		name string
		icon PluginDesktopIcon
		want bool
	}{
		{"catalog", PluginDesktopIcon{Kind: "Name", Data: "reicon:" + navigationIcons[0]}, true},
		{"unknown", PluginDesktopIcon{Kind: "Name", Data: "reicon:missing"}, false},
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
}
