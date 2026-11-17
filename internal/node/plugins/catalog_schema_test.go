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
