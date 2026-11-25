package operations

import (
	"os"
	"path/filepath"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRuntimeRequirementsFenceExecutionCapture(t *testing.T) {
	t.Parallel()
	source, profile := t.TempDir(), t.TempDir()
	manifest := `{"$schema":"` + plugins.ManifestSchema + `","name":"runtime-fixture","version":"1.0.0","extensions":{"dev.sailry.platform":{"api_version":"v1","products":["harness"],"requires_modules":["files"]}}}`
	if err := os.WriteFile(filepath.Join(source, "plugin.json"), []byte(manifest), 0o600); err != nil {
		t.Fatal(err)
	}
	allowed := plugins.WithRuntime(t.Context(), plugins.Runtime{ProductID: "harness", Modules: []string{"files"}})
	installed, err := plugins.NewManager(profile).Install(allowed, source, "", "runtime-fixture")
	if err != nil {
		t.Fatal(err)
	}
	bundled, err := Bundled()
	if err != nil {
		t.Fatal(err)
	}
	index := slices.IndexFunc(bundled.Packages, func(value Package) bool { return value.Reference.Name == "files" })
	if index < 0 {
		t.Fatal("bundled files package is missing")
	}
	for _, reference := range []protocol.PluginReference{plugins.PackageReference(installed), bundled.Packages[index].Reference} {
		references := []protocol.PluginReference{reference}
		blocked := plugins.WithRuntime(t.Context(), plugins.Runtime{ProductID: "harness"})
		if _, err := CaptureCatalog(blocked, profile, references); err == nil {
			t.Fatalf("catalog admitted missing modules for %s", reference.Name)
		}
		if _, err := CaptureResources(blocked, profile, references); err == nil {
			t.Fatalf("resources admitted missing modules for %s", reference.Name)
		}
		if _, err := CaptureCatalog(allowed, profile, references); err != nil {
			t.Fatal(err)
		}
		if _, err := CaptureResources(allowed, profile, references); err != nil {
			t.Fatal(err)
		}
	}
}
