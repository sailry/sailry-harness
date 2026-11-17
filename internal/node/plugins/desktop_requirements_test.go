package plugins

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDesktopBundleRetainsPlatformRequirements(t *testing.T) {
	t.Parallel()
	const entry = "dev.sailry.platform/entry.js"
	entries := []packageEntry{
		{name: "dev.sailry.platform", directory: true},
		{name: entry, data: []byte("export function mount() {}")},
		{name: "plugin.json", data: []byte(`{"$schema":"` + ManifestSchema + `","name":"desktop-requirements-fixture","version":"1.0.0","extensions":{"dev.sailry.platform":{"api_version":"v1","min_sailry_version":"0.0.1","desktop":{"entry":"` + entry + `","resources":["` + entry + `"]}}}}`)},
	}
	manager := NewManager(t.TempDir())
	info, err := manager.publish(t.Context(), entries, nil, "desktop-requirements-fixture", protocol.PluginOrigin{Kind: "directory"})
	if err != nil {
		t.Fatal(err)
	}
	for _, legacy := range []bool{false, true} {
		if legacy {
			info.Requirements = nil
		}
		bundle, err := manager.ReadDesktop(t.Context(), info, protocol.PluginDesktopSurface{Kind: "workspace"})
		if err != nil || bundle.Requirements == nil || bundle.Requirements.APIVersion != "v1" || bundle.Requirements.MinSailryVersion == nil || *bundle.Requirements.MinSailryVersion != "0.0.1" {
			t.Fatalf("desktop requirements (legacy=%v): %+v %v", legacy, bundle.Requirements, err)
		}
	}
}
