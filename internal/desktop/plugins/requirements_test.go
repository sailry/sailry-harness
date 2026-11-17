package plugins

import (
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestMountChecksLocalCompatibility(t *testing.T) {
	minimum := "999.0.0"
	for _, requirements := range []*protocol.PluginRequirements{
		{APIVersion: "v999"},
		{APIVersion: "v1", MinSailryVersion: &minimum},
		{APIVersion: "v1", Invalid: true},
	} {
		runtime := &Runtime{}
		bundle := protocol.DefaultPluginDesktopBundle()
		bundle.Package.Name, bundle.Package.Digest = "remote-fixture", strings.Repeat("a", 64)
		bundle.Entry = "dev.sailry.platform/desktop/main.js"
		bundle.Files[bundle.Entry] = `throw new Error("incompatible bundle was evaluated");`
		bundle.Requirements = requirements
		ctx := protocol.DefaultPluginContext()
		ctx.Package = bundle.Package
		if err := protocol.ValidatePluginDesktopBundle(bundle); err != nil {
			t.Fatal(err)
		}
		if _, err := runtime.Mount(bundle, ctx); err == nil {
			t.Fatalf("local loader accepted remote requirements: %+v", requirements)
		}
		if len(runtime.mounts) != 0 {
			t.Fatal("an incompatible bundle entered the runtime")
		}
	}
}
