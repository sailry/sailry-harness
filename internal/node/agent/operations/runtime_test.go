package operations

import (
	"os"
	"path/filepath"
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
	references := []protocol.PluginReference{plugins.PackageReference(installed)}
	blocked := plugins.WithRuntime(t.Context(), plugins.Runtime{ProductID: "harness"})
	if _, err := CaptureCatalog(blocked, profile, references); err == nil {
		t.Fatal("catalog admitted missing modules")
	}
	if _, err := CaptureResources(blocked, profile, references); err == nil {
		t.Fatal("resources admitted missing modules")
	}
	if _, err := CaptureCatalog(allowed, profile, references); err != nil {
		t.Fatal(err)
	}
	if _, err := CaptureResources(allowed, profile, references); err != nil {
		t.Fatal(err)
	}
}
