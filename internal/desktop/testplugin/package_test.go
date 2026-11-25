package testplugin

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPackages(t *testing.T) {
	fixtures := append([]Package{Basic("basic-fixture"), Notification(), Desktop(), Status(), Terminal(), Model(), Statistics()}, Connectors()...)
	for _, fixture := range fixtures {
		t.Run(fixture.Name, func(t *testing.T) {
			directory, err := fixture.Write(t.TempDir())
			if err != nil {
				t.Fatal(err)
			}
			manager := plugins.NewManager(t.TempDir())
			info, err := manager.Install(t.Context(), directory, "", fixture.Name)
			if err != nil {
				t.Fatal(err)
			}
			if info.Summary.Name != fixture.Name || info.Summary.Digest == "" || info.Extension == nil || len(info.Issues) != 0 || info.Summary.Compatibility.Kind != "compatible" {
				t.Fatalf("fixture is not a valid local package: %+v, issues %+v", info, info.Issues)
			}
			if (fixture.Name == DesktopName || fixture.Name == ModelName) && info.Settings == nil {
				t.Fatal("fixture using readSettings has no installed settings schema")
			}
			if desktop := info.Extension.Desktop; desktop != nil {
				for _, surface := range []struct {
					kind  string
					entry *string
				}{{"workspace", desktop.Entry}, {"composer", desktop.UiEntry}} {
					if surface.entry == nil {
						continue
					}
					bundle, err := manager.ReadDesktop(t.Context(), info, protocol.PluginDesktopSurface{Kind: surface.kind})
					if err != nil || bundle.Entry != *surface.entry || bundle.Files[bundle.Entry] == "" {
						t.Fatalf("%s fixture entry is unavailable: %+v, %v", surface.kind, bundle, err)
					}
				}
			}
			if page := info.Extension.SettingsPage; page != nil {
				bundle, err := manager.ReadDesktop(t.Context(), info, protocol.PluginDesktopSurface{Kind: "settings"})
				if err != nil || page.Entry == nil || bundle.Entry != *page.Entry || bundle.Files[bundle.Entry] == "" {
					t.Fatalf("Settings fixture entry is unavailable: %+v, %v", bundle, err)
				}
			}
		})
	}
}
