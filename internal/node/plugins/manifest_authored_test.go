package plugins

import (
	"encoding/json"
	"fmt"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestManifestIsAuthoredInput keeps a package manifest checked against
// exactly what this build declares. The same extension read back from a newer
// Node is a peer's report and decodes; here it is an author's input, and a
// member, enum value or variant this build does not declare is refused.
func TestManifestIsAuthoredInput(t *testing.T) {
	t.Parallel()
	manifest := func(extension string) []byte {
		return []byte(`{"$schema":"` + ManifestSchema + `","name":"sample","version":"0.1.0","extensions":{"dev.sailry.platform":` + extension + `}}`)
	}
	tool := `{"api_version":"v1","tools":[{"name":"inspect","description":"Inspect","handler":{"name":"run","parameters":{"type":"object"}%s}}]}`
	connector := `{"api_version":"v1","connectors":[{"id":"mail","display":{"label":"Mail"},"accounts":"single","origins":[],"auth":[{"kind":"future_auth","scopes":[]}],"inject":[]}]}`
	for _, current := range []struct {
		name      string
		extension string
		valid     bool
	}{
		{"declared members", `{"api_version":"v1","instructions":"Use the tools"}`, true},
		{"unknown member", `{"api_version":"v1","future_feature":{"enabled":true}}`, false},
		{"unknown member of a tool handler", fmt.Sprintf(tool, `,"future_gate":true`), false},
		{"unknown enum value", `{"api_version":"v1","scope":"future_scope"}`, false},
		{"unknown variant", connector, false},
	} {
		t.Run(current.name, func(t *testing.T) {
			t.Parallel()
			result, err := ValidateManifest(manifest(current.extension))
			if err != nil {
				t.Fatalf("manifest: %v", err)
			}
			refused := slices.ContainsFunc(result.Issues, func(issue protocol.PluginIssue) bool {
				return issue.Kind == protocol.PluginIssueKindInvalidExtensions
			})
			if (result.Extension != nil) != current.valid || refused == current.valid {
				t.Fatalf("extension accepted = %v, issues = %+v", result.Extension != nil, result.Issues)
			}
			// The refusal is the decoder's, before any rule of the validator.
			if err := json.Unmarshal([]byte(current.extension), new(protocol.PluginExtension)); (err == nil) != current.valid {
				t.Fatalf("authored decode: %v", err)
			}
			// What the installer refuses, a controller still reads from a
			// Node that declares it.
			var read protocol.PluginExtension
			if err := protocol.DecodeAs(protocol.PeerData, []byte(current.extension), &read); err != nil {
				t.Fatalf("the extension as a peer's report: %v", err)
			}
		})
	}
}
