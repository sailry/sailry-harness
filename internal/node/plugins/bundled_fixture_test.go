package plugins

import (
	"encoding/json"
	"strings"
	"testing"
	"testing/fstest"
)

const bundledFixtureName = "bundled-fixture"

func bundledFixture(t testing.TB) fstest.MapFS {
	t.Helper()
	const entry = "dev.sailry.platform/host/main.js"
	const resource = "dev.sailry.platform/host/value.js"
	manifest := []byte(`{"$schema":"` + ManifestSchema + `","name":"` + bundledFixtureName + `","version":"1.0.0","extensions":{"dev.sailry.platform":{"api_version":"v1","host":{"entry":"` + entry + `","resources":["` + entry + `","` + resource + `"],"handlers":["read"],"read_only":["read"]}}}}`)
	catalog, err := json.Marshal(map[string]any{
		"version": 1, "source_commit": strings.Repeat("a", 40),
		"packages": []any{map[string]any{
			"id": bundledFixtureName, "manifest": json.RawMessage(manifest),
			"versions": []any{map[string]any{"version": "1.0.0", "commit": strings.Repeat("a", 40), "api_version": "v1"}},
		}},
	})
	if err != nil {
		t.Fatal(err)
	}
	root := "plugins/" + bundledFixtureName + "/"
	return fstest.MapFS{
		"plugins/catalog.json":           {Data: catalog},
		root + "plugin.json":             {Data: manifest},
		root + entry:                     {Data: []byte(`import {value} from "./value.js"; export async function read(){return value;}`)},
		root + resource:                  {Data: []byte(`export const value="fixture";`)},
		root + "skills/inspect/SKILL.md": {Data: []byte("---\nname: inspect\ndescription: Inspect a fixture\n---\nRead the fixture.\n")},
	}
}
