package plugins

import (
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func testDesktopDeclarations(t *testing.T) {
	for _, current := range []struct {
		name      string
		extension string
		valid     bool
	}{
		{"workspace", `{"desktop":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"]}}`, true},
		{"empty desktop", `{"desktop":{}}`, false},
		{"settings-only entry", `{"desktop":{"resources":["dev.sailry.platform/settings.js"]},"settings_page":{"navigation":{"label":"Settings"},"entry":"dev.sailry.platform/settings.js"}}`, true},
		{"undeclared settings entry", `{"desktop":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"]},"settings_page":{"navigation":{"label":"Settings"},"entry":"dev.sailry.platform/settings.js"}}`, false},
		{"settings navigation without settings entry", `{"desktop":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],"navigation":{"label":"Settings","icon":"reicon:ui/settings"},"navigation_options":{"surface":"settings"}}}`, false},
		{"preview outside host scope", `{"scope":"desktop","actions":["files.read"],"desktop":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],"previews":["text/plain"]}}`, false},
		{"preview without file action", `{"desktop":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],"previews":["text/plain"]}}`, false},
		{"preview with file action", `{"actions":["files.read"],"desktop":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],"previews":["text/plain"]}}`, true},
		{"terminal without create contribution", `{"actions":["terminals.read","terminals.control"],"desktop":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],"renderers":[{"resource":"terminal","create":"new-terminal"}]}}`, false},
		{"browser missing control action", `{"actions":["browser.read"],"desktop":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],"renderers":[{"resource":"browser"}]}}`, false},
		{"browser declared actions", `{"actions":["browser.read","browser.control"],"desktop":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],"renderers":[{"resource":"browser"}]}}`, true},
	} {
		t.Run(current.name, func(t *testing.T) {
			var fields map[string]json.RawMessage
			if err := json.Unmarshal([]byte(current.extension), &fields); err != nil {
				t.Fatal(err)
			}
			fields["api_version"] = json.RawMessage(`"v1"`)
			body, err := json.Marshal(fields)
			if err != nil {
				t.Fatal(err)
			}
			var extension protocol.PluginExtension
			if err := json.Unmarshal(body, &extension); err != nil {
				t.Fatal(err)
			}
			if err := validateDesktopExtension(extension); (err == nil) != current.valid {
				t.Fatalf("desktop extension declaration: %v", err)
			}
		})
	}
}
