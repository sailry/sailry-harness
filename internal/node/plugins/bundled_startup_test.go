package plugins

import (
	"encoding/json"
	"io/fs"
	"testing"

	sailry "github.com/sailry/sailry-harness"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestBundledStartupCompatibility(t *testing.T) {
	manager := NewManager(t.TempDir())
	for _, entry := range BundledDefaults() {
		t.Run(entry.Name, func(t *testing.T) {
			info, err := manager.InspectBundled(t.Context(), entry.Name)
			if err != nil {
				t.Fatal(err)
			}
			if compatibility := InfoCompatibility(info); compatibility.Kind != "compatible" {
				data, err := fs.ReadFile(sailry.Plugins, "plugins/"+entry.Name+"/plugin.json")
				if err != nil {
					t.Fatal(err)
				}
				var manifest struct {
					Extensions map[string]json.RawMessage `json:"extensions"`
				}
				if err := json.Unmarshal(data, &manifest); err != nil {
					t.Fatal(err)
				}
				var extension protocol.PluginExtension
				err = json.Unmarshal(manifest.Extensions["dev.sailry.platform"], &extension)
				if err == nil {
					err = validateExtension(extension)
				}
				t.Fatalf("bundled package is incompatible: %s requirements=%+v issues=%+v declaration=%v", entry.Name, info.Requirements, info.Issues, err)
			}
		})
	}
}
