package plugins

import (
	"encoding/json"
	"io/fs"
	"os"
	"testing"

	sailry "github.com/sailry/sailry-harness"
)

func TestBundledCatalogMatchesPublishedSchema(t *testing.T) {
	data, err := os.ReadFile("../../../schemas/plugins/v1/catalog.schema.json")
	if err != nil {
		t.Fatal(err)
	}
	declaration, err := decodeValue(data)
	if err != nil {
		t.Fatal(err)
	}
	validator, err := compileSchema(declaration)
	if err != nil {
		t.Fatal(err)
	}
	data, err = fs.ReadFile(sailry.Plugins, "plugins/catalog.json")
	if err != nil {
		t.Fatal(err)
	}
	var catalog any
	if err := json.Unmarshal(data, &catalog); err != nil {
		t.Fatal(err)
	}
	if err := validator.Validate(catalog); err != nil {
		t.Fatalf("official release history violates its published contract: %v", err)
	}
}

func TestOfficialCatalogRuntimeRequirementsSchema(t *testing.T) {
	t.Parallel()
	data, err := os.ReadFile("../../../schemas/plugins/v1/catalog.schema.json")
	if err != nil {
		t.Fatal(err)
	}
	declaration, err := decodeValue(data)
	if err != nil {
		t.Fatal(err)
	}
	validator, err := compileSchema(declaration)
	if err != nil {
		t.Fatal(err)
	}
	for _, scenario := range []struct {
		name, sidecar, versionFields, manifestFields string
		valid                                        bool
	}{
		{name: "legacy", valid: true},
		{name: "sidecar", sidecar: `,"version_requirements":{"1.0.0":{"products":["harness"],"requires_modules":["git"]}}`, valid: true},
		{name: "explicit unrestricted", sidecar: `,"version_requirements":{"1.0.0":{}}`, valid: true},
		{name: "runtime fields in version", versionFields: `,"products":["harness"]`},
		{name: "runtime fields in manifest", manifestFields: `,"extensions":{"dev.sailry.platform":{"api_version":"v1","requires_modules":["git"]}}`},
		{name: "null entry", sidecar: `,"version_requirements":{"1.0.0":null}`},
		{name: "empty products", sidecar: `,"version_requirements":{"1.0.0":{"products":[]}}`},
		{name: "invalid module", sidecar: `,"version_requirements":{"1.0.0":{"requires_modules":["Git"]}}`},
		{name: "unknown field", sidecar: `,"version_requirements":{"1.0.0":{"modules":["git"]}}`},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			var catalog any
			data := []byte(`{"version":1,"packages":[{"id":"fixture","manifest":{"name":"fixture"` + scenario.manifestFields + `},"versions":[{"version":"1.0.0","commit":"0123456789012345678901234567890123456789"` + scenario.versionFields + `}]` + scenario.sidecar + `}]}`)
			if err := json.Unmarshal(data, &catalog); err != nil {
				t.Fatal(err)
			}
			if err := validator.Validate(catalog); (err == nil) != scenario.valid {
				t.Fatalf("valid=%v err=%v", scenario.valid, err)
			}
		})
	}
}
