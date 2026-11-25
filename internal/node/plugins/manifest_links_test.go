package plugins

import (
	"encoding/json"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const conversationLinkManifest = `{"$schema":%q,"name":"link-fixture","extensions":{"dev.sailry.platform":{
	"api_version":"v1","desktop":{"ui_entry":"dev.sailry.platform/ui.js","resources":["dev.sailry.platform/ui.js"]},"ui":[%s]}}}`

const conversationLinkOwnScheme = `,"link_scheme":"sailry-plugin-link-fixture"`

func conversationLinkManifestButton(id, slot, fields string) string {
	return fmt.Sprintf(`{"id":%q,"slot":%q,"kind":"button","label":{"label":"Fixture"},"handler":"open"%s}`, id, slot, fields)
}

func TestConversationLinkManifest(t *testing.T) {
	button, own := conversationLinkManifestButton, conversationLinkOwnScheme
	for _, tc := range []struct {
		name, ui string
		valid    bool
	}{
		{"legacy v1", button("one", "composer", ""), true},
		{"own scheme", button("one", "composer", own), true},
		{"duplicate declaration", button("one", "composer", own) + "," + button("two", "composer", own), false},
		{"duplicate at different priorities", button("one", "composer", own+`,"priority":10`) + "," + button("two", "composer", own), false},
		{"priority without scheme", button("one", "composer", `,"priority":0`), false},
		{"priority with null scheme", button("one", "composer", `,"link_scheme":null,"priority":0`), false},
		{"not a composer", button("one", "context", own), false},
		{"priority on another slot", button("one", "context", own+`,"priority":2147483647`), false},
		{"not a button", `{"id":"one","slot":"composer","kind":"toggle","label":{"label":"Fixture"},"handler":"open"` + own + `}`, false},
		{"no handler", `{"id":"one","slot":"composer","kind":"button","label":{"label":"Fixture"}` + own + `}`, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			manifest, err := ValidateManifest(fmt.Appendf(nil, conversationLinkManifest, ManifestSchema, tc.ui))
			if err != nil || (manifest.Extension != nil) != tc.valid {
				t.Fatalf("manifest admission = %+v, %v, want valid %t", manifest, err, tc.valid)
			}
		})
	}
	for _, scheme := range []string{"https", "file", "javascript", "sailry", "sailry-knowledge", "sailry-plugin-other", "sailry-plugin-link-fixture:/", "sailry-plugin-*"} {
		t.Run(scheme, func(t *testing.T) {
			for _, priority := range []string{"", `,"priority":2147483647`} {
				ui := button("one", "composer", fmt.Sprintf(`,"link_scheme":%q`, scheme)+priority)
				manifest, err := ValidateManifest(fmt.Appendf(nil, conversationLinkManifest, ManifestSchema, ui))
				if err != nil || manifest.Extension != nil || manifest.Requirements == nil || !manifest.Requirements.Invalid ||
					len(manifest.Issues) != 1 || manifest.Issues[0].Kind != protocol.PluginIssueKindInvalidExtensions {
					t.Fatalf("unowned scheme admission with %q = %+v, %v", priority, manifest, err)
				}
			}
		})
	}
}

func TestConversationLinkManifestPriority(t *testing.T) {
	manifest := func(literal string) (Manifest, error) {
		fields := conversationLinkOwnScheme
		if literal != "" {
			fields += `,"priority":` + literal
		}
		ui := conversationLinkManifestButton("one", "composer", fields)
		return ValidateManifest(fmt.Appendf(nil, conversationLinkManifest, ManifestSchema, ui))
	}
	for _, test := range []struct {
		literal string
		want    int
	}{{"", 0}, {"0", 0}, {"12", 12}, {"-2147483648", -2147483648}, {"2147483647", 2147483647}} {
		parsed, err := manifest(test.literal)
		if err != nil || parsed.Extension == nil {
			t.Fatalf("priority %q was not admitted: %+v, %v", test.literal, parsed, err)
		}
		declaration := parsed.Extension.Ui[0]
		if actual := declaration.Priority.Int(); actual != test.want {
			t.Fatalf("priority %q decoded as %d, want %d", test.literal, actual, test.want)
		}
		body, err := json.Marshal(declaration)
		var fields map[string]json.RawMessage
		if err != nil || json.Unmarshal(body, &fields) != nil || string(fields["priority"]) != test.literal {
			t.Fatalf("priority %q did not round-trip: %s (%v)", test.literal, body, err)
		}
	}
	for _, literal := range []string{`null`, `"1"`, `true`, `1.5`, `1.0`, `1e0`, `2147483648`, `-2147483649`} {
		parsed, err := manifest(literal)
		if err != nil || parsed.Extension != nil || parsed.Requirements == nil || !parsed.Requirements.Invalid {
			t.Fatalf("invalid priority %q admission = %+v, %v", literal, parsed, err)
		}
	}
}
