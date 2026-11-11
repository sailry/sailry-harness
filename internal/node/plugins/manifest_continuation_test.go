package plugins

import (
	"encoding/json"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestContinuationMarkers covers the named markers a package declares beside
// its default continuation marker.
func TestContinuationMarkers(t *testing.T) {
	t.Parallel()
	many := map[string]protocol.PluginDesktopNavigation{}
	for index := range 9 {
		many[fmt.Sprintf("marker-%d", index)] = protocol.PluginDesktopNavigation{Label: "Marker"}
	}
	for _, current := range []struct {
		name    string
		markers map[string]protocol.PluginDesktopNavigation
		valid   bool
	}{
		{"none", nil, true},
		{"named marker", map[string]protocol.PluginDesktopNavigation{"verify": {Label: "Verify work", Locales: map[string]string{"zh-CN": "核验"}}}, true},
		{"invalid id", map[string]protocol.PluginDesktopNavigation{"Verify": {Label: "Verify work"}}, false},
		{"empty label", map[string]protocol.PluginDesktopNavigation{"verify": {}}, false},
		{"too many", many, false},
	} {
		t.Run(current.name, func(t *testing.T) {
			t.Parallel()
			var extension protocol.PluginExtension
			if err := json.Unmarshal([]byte(`{"api_version":"v1","continuation_marker":{"label":"Continue work"}}`), &extension); err != nil {
				t.Fatal(err)
			}
			extension.ContinuationMarkers = current.markers
			if err := validateExtension(extension); (err == nil) != current.valid {
				t.Fatalf("continuation markers: %v", err)
			}
		})
	}
	t.Run("manifest field", func(t *testing.T) {
		t.Parallel()
		var extension protocol.PluginExtension
		if err := json.Unmarshal([]byte(`{"api_version":"v1","continuation_markers":{"verify":{"label":"Verify work"}}}`), &extension); err != nil {
			t.Fatal(err)
		}
		if extension.ContinuationMarkers["verify"].Label != "Verify work" {
			t.Fatalf("decoded markers = %+v", extension.ContinuationMarkers)
		}
	})
}
