package plugins

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func runtimeCatalogFixture(t *testing.T, item map[string]any) (*Manager, func()) {
	t.Helper()
	var body atomic.Value
	publish := func() {
		t.Helper()
		data, err := json.Marshal(map[string]any{"version": 1, "source_commit": strings.Repeat("a", 40), "packages": []any{item}})
		if err != nil {
			t.Fatal(err)
		}
		body.Store(data)
	}
	publish()
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		_, _ = writer.Write(body.Load().([]byte))
	}))
	t.Cleanup(server.Close)
	manager := NewManager(t.TempDir())
	manager.OfficialURL = server.URL
	return manager, publish
}

func TestOfficialCatalogRuntimeRequirementsSelectEveryVersion(t *testing.T) {
	t.Parallel()
	const name = "catalog-runtime-fixture"
	item := map[string]any{
		"id": name,
		"manifest": map[string]any{"$schema": ManifestSchema, "name": name, "version": "3.0.0", "extensions": map[string]any{
			"dev.sailry.platform": map[string]any{"api_version": "v1", "min_sailry_version": "0.2.14"},
		}},
		"versions": []any{
			map[string]any{"version": "3.0.0", "commit": strings.Repeat("c", 40), "api_version": "v1", "min_sailry_version": "0.2.14"},
			map[string]any{"version": "2.0.0", "commit": strings.Repeat("b", 40), "api_version": "v1", "min_sailry_version": "0.2.14"},
			map[string]any{"version": "1.0.0", "commit": strings.Repeat("a", 40), "api_version": "v1", "min_sailry_version": "0.0.1"},
		},
		"version_requirements": map[string]any{
			"3.0.0": map[string]any{"products": []string{"code"}, "requires_modules": []string{"files"}},
			"2.0.0": map[string]any{"requires_modules": []string{"git"}},
			"1.0.0": map[string]any{},
		},
	}
	manager, _ := runtimeCatalogFixture(t, item)
	for _, scenario := range []struct {
		runtime Runtime
		version string
	}{
		{Runtime{ProductID: "harness", Modules: []string{"files"}}, "1.0.0"},
		{Runtime{ProductID: "harness", Modules: []string{"files", "git"}}, "2.0.0"},
		{Runtime{ProductID: "code", Modules: []string{"files"}}, "3.0.0"},
		{Runtime{ProductID: "code"}, "1.0.0"},
	} {
		ctx := WithRuntime(t.Context(), scenario.runtime)
		entry, err := manager.officialEntry(ctx, name, true)
		if err != nil || entry.CompatibleVersion == nil || entry.CompatibleVersion.Version != scenario.version {
			t.Fatalf("runtime=%+v entry=%+v err=%v", scenario.runtime, entry, err)
		}
		if entry.Version == nil || *entry.Version != "3.0.0" || len(entry.Versions) != 3 {
			t.Fatalf("runtime selection replaced latest/history: %+v", entry)
		}
		var restored protocol.PluginCatalogEntry
		data, err := json.Marshal(entry)
		if err != nil || json.Unmarshal(data, &restored) != nil {
			t.Fatalf("catalog round trip: %v", err)
		}
		selected := SelectCompatibleVersion(restored, nil, scenario.runtime)
		if selected == nil || selected.Version != scenario.version {
			t.Fatalf("projected catalog lost runtime requirements: %+v", restored)
		}
		if scenario.runtime.ProductID == "harness" {
			if _, err := officialVersion(restored, versionText("3.0.0"), scenario.runtime); err == nil {
				t.Fatal("explicit latest selection ignored sidecar product requirements")
			}
		}
	}
}

func TestOfficialCatalogRuntimeRequirementsAreFailClosed(t *testing.T) {
	t.Parallel()
	for _, scenario := range []struct {
		name, sidecar, manifestMinimum, recordMinimum string
		valid, omitLatest                             bool
	}{
		{name: "complete map", sidecar: `{"1.0.0":{},"0.9.0":{}}`, valid: true},
		{name: "legacy without map", manifestMinimum: "0.2.13", recordMinimum: "0.2.13", valid: true},
		{name: "missing capability map"},
		{name: "latest projection cannot lower record minimum", manifestMinimum: "0.2.13"},
		{name: "missing historical entry", sidecar: `{"1.0.0":{}}`},
		{name: "missing latest entry", sidecar: `{"0.9.0":{}}`},
		{name: "missing implicit latest entry", sidecar: `{"0.9.0":{}}`, omitLatest: true},
		{name: "null map", sidecar: `null`},
		{name: "array map", sidecar: `[]`},
		{name: "empty map", sidecar: `{}`},
		{name: "null entry", sidecar: `{"1.0.0":null,"0.9.0":{}}`},
		{name: "array entry", sidecar: `{"1.0.0":[],"0.9.0":{}}`},
		{name: "null products", sidecar: `{"1.0.0":{"products":null},"0.9.0":{}}`},
		{name: "empty products", sidecar: `{"1.0.0":{"products":[]},"0.9.0":{}}`},
		{name: "null modules", sidecar: `{"1.0.0":{"requires_modules":null},"0.9.0":{}}`},
		{name: "wrong modules type", sidecar: `{"1.0.0":{"requires_modules":"git"},"0.9.0":{}}`},
		{name: "duplicate modules", sidecar: `{"1.0.0":{"requires_modules":["git","git"]},"0.9.0":{}}`},
		{name: "invalid product", sidecar: `{"1.0.0":{"products":["Harness"]},"0.9.0":{}}`},
		{name: "unknown field", sidecar: `{"1.0.0":{"modules":["git"]},"0.9.0":{}}`},
		{name: "invalid version key", sidecar: `{"1.0.0":{},"0.9.0":{},"latest":{}}`},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			t.Parallel()
			minimum := scenario.manifestMinimum
			if minimum == "" {
				minimum = "0.2.14"
			}
			recordMinimum := scenario.recordMinimum
			if recordMinimum == "" {
				recordMinimum = "0.2.14"
			}
			versions := []any{map[string]any{"version": "0.9.0", "commit": strings.Repeat("a", 40)}}
			if !scenario.omitLatest {
				versions = append(versions, map[string]any{"version": "1.0.0", "commit": strings.Repeat("b", 40), "api_version": "v1", "min_sailry_version": recordMinimum})
			}
			item := map[string]any{
				"id": "catalog-runtime-fixture",
				"manifest": map[string]any{"$schema": ManifestSchema, "name": "catalog-runtime-fixture", "version": "1.0.0", "extensions": map[string]any{
					"dev.sailry.platform": map[string]any{"api_version": "v1", "min_sailry_version": minimum},
				}},
				"versions": versions,
			}
			if scenario.sidecar != "" {
				item["version_requirements"] = json.RawMessage(scenario.sidecar)
			}
			manager, _ := runtimeCatalogFixture(t, item)
			_, err := manager.OfficialEntries(t.Context(), true)
			if (err == nil) != scenario.valid {
				t.Fatalf("valid=%v err=%v", scenario.valid, err)
			}
		})
	}
}

func TestOfficialCatalogMergesManifestRuntimeRequirements(t *testing.T) {
	t.Parallel()
	item := map[string]any{
		"id": "catalog-runtime-fixture",
		"manifest": map[string]any{"$schema": ManifestSchema, "name": "catalog-runtime-fixture", "version": "1.0.0", "extensions": map[string]any{
			"dev.sailry.platform": map[string]any{"api_version": "v1", "min_sailry_version": "0.2.14", "products": []string{"harness", "code"}, "requires_modules": []string{"files"}},
		}},
		"versions":             []any{map[string]any{"version": "1.0.0", "commit": strings.Repeat("a", 40)}},
		"version_requirements": map[string]any{"1.0.0": map[string]any{"products": []string{"harness"}, "requires_modules": []string{"git"}}},
	}
	manager, publish := runtimeCatalogFixture(t, item)
	for _, scenario := range []struct {
		runtime Runtime
		allowed bool
	}{
		{Runtime{ProductID: "harness", Modules: []string{"files", "git"}}, true},
		{Runtime{ProductID: "harness", Modules: []string{"git"}}, false},
		{Runtime{ProductID: "harness", Modules: []string{"files"}}, false},
		{Runtime{ProductID: "code", Modules: []string{"files", "git"}}, false},
	} {
		entry, err := manager.officialEntry(WithRuntime(t.Context(), scenario.runtime), "catalog-runtime-fixture", true)
		if err != nil || (entry.CompatibleVersion != nil) != scenario.allowed {
			t.Fatalf("runtime=%+v entry=%+v err=%v", scenario.runtime, entry, err)
		}
		version := entry.Versions[0]
		if !reflect.DeepEqual(version.Products, []string{"harness"}) || !reflect.DeepEqual(version.RequiresModules, []string{"files", "git"}) {
			t.Fatalf("manifest/sidecar constraints were replaced: %+v", version)
		}
	}
	item["version_requirements"] = map[string]any{"1.0.0": map[string]any{"products": []string{"another-product"}}}
	publish()
	if _, err := manager.OfficialEntries(t.Context(), true); err == nil {
		t.Fatal("conflicting product requirements became unrestricted")
	}
}

func TestOfficialCatalogHintsCannotBypassRealRuntimeRequirements(t *testing.T) {
	t.Parallel()
	const name = "runtime-install-fixture"
	directory := t.TempDir()
	commit := repositoryVersionFixture(t, directory, name)
	revision, manifest := commit("1.0.0", `{"api_version":"v1","min_sailry_version":"0.2.14","products":["harness"],"requires_modules":["git"]}`)
	var projected map[string]any
	if err := json.Unmarshal(manifest, &projected); err != nil {
		t.Fatal(err)
	}
	extension := projected["extensions"].(map[string]any)["dev.sailry.platform"].(map[string]any)
	delete(extension, "products")
	delete(extension, "requires_modules")
	item := map[string]any{
		"id": name, "manifest": projected,
		"versions":             []any{map[string]any{"version": "1.0.0", "commit": revision, "api_version": "v1", "min_sailry_version": "0.2.14"}},
		"version_requirements": map[string]any{"1.0.0": map[string]any{}},
	}
	manager, publish := runtimeCatalogFixture(t, item)
	source, err := ParseRepositorySource(protocol.PluginSkillsSource{Repository: directory})
	if err != nil {
		t.Fatal(err)
	}
	manager.officialRepository = source.Repository
	allowed := WithRuntime(t.Context(), Runtime{ProductID: "harness", Modules: []string{"git"}})
	blocked := WithRuntime(t.Context(), Runtime{ProductID: "harness", Modules: []string{"files"}})
	if entry, err := manager.officialEntry(blocked, name, true); err != nil || entry.CompatibleVersion == nil {
		t.Fatalf("inaccurate hint fixture was not selectable: %+v %v", entry, err)
	}
	for _, ctx := range []Runtime{{ProductID: "harness", Modules: []string{"files"}}, {ProductID: "code", Modules: []string{"git"}}} {
		if _, err := manager.InspectOfficial(WithRuntime(t.Context(), ctx), name, nil, true); err == nil {
			t.Fatalf("real manifest was not rechecked for %+v", ctx)
		}
	}
	selection, err := manager.InspectOfficial(allowed, name, nil, true)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := manager.InstallOfficial(blocked, selection); err == nil {
		t.Fatal("changed runtime installed a package from its inspection cache")
	}
	if _, err := os.Stat(filepath.Join(manager.profile, "plugins/packages", selection.Info.Summary.Digest)); !os.IsNotExist(err) {
		t.Fatalf("incompatible content was published: %v", err)
	}
	item["version_requirements"] = map[string]any{"1.0.0": map[string]any{"requires_modules": []string{"worktrees"}}}
	publish()
	if _, err := manager.InstallOfficial(allowed, selection); err == nil {
		t.Fatal("installation ignored newly published per-version requirements")
	}
	item["version_requirements"] = map[string]any{"1.0.0": map[string]any{}}
	publish()
	installed, err := manager.InstallOfficial(allowed, selection)
	if err != nil || !slices.Contains(installed.Requirements.RequiresModules, "git") {
		t.Fatalf("compatible installation lost actual manifest requirements: %+v %v", installed, err)
	}
	if _, err := manager.CapturedInfo(blocked, PackageReference(installed)); err == nil {
		t.Fatal("captured installed content trusted its catalog hint")
	}
}
