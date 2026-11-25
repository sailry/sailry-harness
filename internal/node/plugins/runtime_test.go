package plugins

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRuntimeRequirements(t *testing.T) {
	t.Parallel()
	for _, scenario := range []struct {
		name, fields string
		invalid      bool
	}{
		{"legacy", ``, false},
		{"requirements", `,"products":["harness","code"],"requires_modules":["files","git"]`, false},
		{"empty modules", `,"requires_modules":[]`, false},
		{"empty products", `,"products":[]`, true},
		{"null products", `,"products":null`, true},
		{"null modules", `,"requires_modules":null`, true},
		{"wrong module type", `,"requires_modules":"files"`, true},
		{"duplicate products", `,"products":["harness","harness"]`, true},
		{"duplicate modules", `,"requires_modules":["files","files"]`, true},
		{"invalid product", `,"products":["Harness"]`, true},
		{"invalid module", `,"requires_modules":["files/host"]`, true},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			manifest, err := ValidateManifest([]byte(`{"$schema":"` + ManifestSchema + `","name":"runtime-fixture","version":"1.0.0","extensions":{"dev.sailry.platform":{"api_version":"v1"` + scenario.fields + `}}}`))
			if err != nil || manifest.Requirements == nil || manifest.Requirements.Invalid != scenario.invalid {
				t.Fatalf("requirements=%+v err=%v", manifest.Requirements, err)
			}
			info := protocol.PluginInfo{Requirements: manifest.Requirements, Extension: manifest.Extension}
			if (InfoCompatibility(info).Kind == "compatible") == scenario.invalid {
				t.Fatalf("invalid declaration became compatible: %+v", info)
			}
		})
	}
}

func TestRuntimeCompatibilityAndHistoricalRecords(t *testing.T) {
	t.Parallel()
	requirements := &protocol.PluginRequirements{APIVersion: "v1", Products: []string{"harness"}, RequiresModules: []string{"files", "git"}}
	info := protocol.DefaultPluginInfo()
	info.Requirements = requirements
	for _, scenario := range []struct {
		runtime Runtime
		kind    string
	}{
		{Runtime{}, "compatible"},
		{Runtime{ProductID: "harness", Modules: []string{"files", "git"}}, "compatible"},
		{Runtime{ProductID: "code", Modules: []string{"files", "git"}}, "unsupported"},
		{Runtime{ProductID: "harness", Modules: []string{"files"}}, "unsupported"},
		{Runtime{ProductID: "harness"}, "unsupported"},
	} {
		if actual := InfoCompatibility(info, scenario.runtime); actual.Kind != scenario.kind {
			t.Fatalf("runtime=%+v compatibility=%+v", scenario.runtime, actual)
		}
	}
	stored := protocol.PluginInfo{Extension: &protocol.PluginExtension{ApiVersion: "v1", Products: requirements.Products, RequiresModules: requirements.RequiresModules}}
	if InfoCompatibility(stored, Runtime{ProductID: "code"}).Kind != "unsupported" {
		t.Fatal("older stored extension lost its product requirements")
	}
	legacy := protocol.PluginInfo{Requirements: &protocol.PluginRequirements{APIVersion: "v1"}}
	if InfoCompatibility(legacy, Runtime{ProductID: "code"}).Kind != "compatible" {
		t.Fatal("legacy requirements gained a product restriction")
	}
	info.Extension = nil
	encoded, err := json.Marshal(info)
	if err != nil {
		t.Fatal(err)
	}
	var decoded protocol.PluginInfo
	if err := json.Unmarshal(encoded, &decoded); err != nil || !reflect.DeepEqual(decoded.Requirements, requirements) {
		t.Fatalf("stored requirements lost: %+v %v", decoded.Requirements, err)
	}
	modules := []string{"files"}
	ctx := WithRuntime(t.Context(), Runtime{ProductID: "harness", Modules: modules})
	modules[0] = "git"
	read := RuntimeFromContext(ctx)
	read.Modules[0] = "ssh"
	if RuntimeFromContext(ctx).Modules[0] != "files" {
		t.Fatal("runtime module ownership escaped its context")
	}
}

func TestRuntimeCatalogVersionSelection(t *testing.T) {
	t.Parallel()
	legacy, harness, code := catalogVersionFixture("1.0.0"), catalogVersionFixture("2.0.0"), catalogVersionFixture("3.0.0")
	harness.Products, harness.RequiresModules = []string{"harness"}, []string{"files"}
	code.Products, code.RequiresModules = []string{"code"}, []string{"git"}
	entry := protocol.DefaultPluginCatalogEntry()
	entry.Versions = []protocol.PluginCatalogVersion{legacy, harness, code}
	for _, scenario := range []struct {
		runtime Runtime
		version string
	}{
		{Runtime{ProductID: "harness", Modules: []string{"files", "git"}}, "2.0.0"},
		{Runtime{ProductID: "code", Modules: []string{"git"}}, "3.0.0"},
		{Runtime{ProductID: "code"}, "1.0.0"},
	} {
		selected := SelectCompatibleVersion(entry, nil, scenario.runtime)
		if selected == nil || selected.Version != scenario.version {
			t.Fatalf("runtime=%+v selected=%+v", scenario.runtime, selected)
		}
	}
}

func TestRuntimeInstallationAndCapture(t *testing.T) {
	t.Parallel()
	source, profile := t.TempDir(), t.TempDir()
	data := `{"$schema":"` + ManifestSchema + `","name":"runtime-fixture","version":"1.0.0","extensions":{"dev.sailry.platform":{"api_version":"v1","products":["harness"],"requires_modules":["files"]}}}`
	if err := os.WriteFile(filepath.Join(source, "plugin.json"), []byte(data), 0o600); err != nil {
		t.Fatal(err)
	}
	manager := NewManager(profile)
	allowed := WithRuntime(t.Context(), Runtime{ProductID: "harness", Modules: []string{"files"}})
	blocked := WithRuntime(t.Context(), Runtime{ProductID: "code", Modules: []string{"files"}})
	if inspected, err := manager.Inspect(blocked, source, "", "runtime-fixture"); err != nil || inspected.Summary.Compatibility.Kind != "unsupported" {
		t.Fatalf("inspection=%+v err=%v", inspected.Summary, err)
	}
	if _, err := manager.Install(blocked, source, "", "runtime-fixture"); err == nil {
		t.Fatal("incompatible product installed")
	}
	installed, err := manager.Install(allowed, source, "", "runtime-fixture")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := manager.CapturedInfo(blocked, PackageReference(installed)); err == nil {
		t.Fatal("captured content bypassed product matching")
	}
	if _, err := manager.CapturedInfo(WithRuntime(t.Context(), Runtime{ProductID: "harness"}), PackageReference(installed)); err == nil {
		t.Fatal("captured content bypassed module matching")
	}
	if _, err := manager.CapturedInfo(allowed, PackageReference(installed)); err != nil {
		t.Fatal(err)
	}
}
