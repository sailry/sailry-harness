package plugins

import (
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func versionText(value string) *string { return &value }

func catalogVersionFixture(version string) protocol.PluginCatalogVersion {
	result := protocol.DefaultPluginCatalogVersion()
	result.Version, result.Commit = version, strings.Repeat("a", 40)
	return result
}

func TestPlatformRequirementsSurviveFunctionalityDecoding(t *testing.T) {
	t.Parallel()
	for _, current := range []struct {
		name, extension, compatibility string
	}{
		{"supported", `{"api_version":"v1","min_sailry_version":"0.0.1"}`, "compatible"},
		{"newer Node", `{"api_version":"v1","min_sailry_version":"999.0.0"}`, "requires_newer"},
		{"future API", `{"api_version":"v2","min_sailry_version":"0.0.1","future_feature":true}`, "unsupported"},
		{"future member", `{"api_version":"v1","min_sailry_version":"999.0.0","future_feature":true}`, "unsupported"},
		{"invalid minimum", `{"api_version":"v1","min_sailry_version":"later"}`, "unsupported"},
		{"null minimum", `{"api_version":"v1","min_sailry_version":null}`, "unsupported"},
		{"missing API", `{"min_sailry_version":"0.0.1"}`, "unsupported"},
		{"invalid declaration", `null`, "unsupported"},
	} {
		t.Run(current.name, func(t *testing.T) {
			t.Parallel()
			entries := []packageEntry{{name: "plugin.json", data: []byte(`{"$schema":"` + ManifestSchema + `","name":"requirements-fixture","version":"1.0.0","extensions":{"dev.sailry.platform":` + current.extension + `}}`)}}
			info, err := describePackage(entries, nil, "requirements-fixture")
			if err != nil || info.Requirements == nil || info.Summary.Compatibility.Kind != current.compatibility {
				t.Fatalf("requirements: %+v %v", info, err)
			}
			if info.Requirements.MinSailryVersion != nil && !strings.Contains(current.extension, *info.Requirements.MinSailryVersion) {
				t.Fatalf("minimum requirement changed: %+v", info.Requirements)
			}
			encoded := info
			encoded.Extension = nil
			if InfoCompatibility(encoded).Kind != current.compatibility {
				t.Fatalf("compatibility depended on parsed functionality: %+v", encoded)
			}
			manager := NewManager(t.TempDir())
			_, err = manager.publish(t.Context(), entries, nil, "requirements-fixture", protocol.PluginOrigin{Kind: "directory"})
			if (err == nil) != (current.compatibility == "compatible") {
				t.Fatalf("installation compatibility = %s: %v", current.compatibility, err)
			}
		})
	}
	t.Run("ordinary skill and MCP package", func(t *testing.T) {
		t.Parallel()
		entries := []packageEntry{
			{name: "plugin.json", data: []byte(`{"$schema":"` + ManifestSchema + `","name":"ordinary-fixture","version":"1.0.0","extensions":{"another.platform":{"api_version":"v8"}}}`)},
			{name: "skills", directory: true},
			{name: "skills/review", directory: true},
			{name: "skills/review/SKILL.md", data: []byte("---\nname: review\ndescription: Review changes\n---\nReview the changes.")},
			{name: "mcp.json", data: []byte(`{"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{"fixture":{"type":"stdio","command":"fixture"}}}`)},
		}
		sortPackageEntries(entries)
		info, err := NewManager(t.TempDir()).publish(t.Context(), entries, nil, "ordinary-fixture", protocol.PluginOrigin{Kind: "directory"})
		if err != nil || info.Requirements != nil || info.Summary.Compatibility.Kind != "compatible" || len(info.Skills) != 1 || len(info.Mcp) != 1 {
			t.Fatalf("ordinary package: %+v %v", info, err)
		}
	})
	t.Run("older stored declaration", func(t *testing.T) {
		t.Parallel()
		info := protocol.DefaultPluginInfo()
		info.Issues = []protocol.PluginIssue{{Path: "plugin.json.extensions.dev.sailry.platform", Kind: protocol.PluginIssueKindUnsupportedExtension}}
		if InfoCompatibility(info).Kind != "unsupported" {
			t.Fatal("an older unsupported declaration became compatible")
		}
		info.Issues = nil
		info.Extension = &protocol.PluginExtension{ApiVersion: "v2"}
		if InfoCompatibility(info).Kind != "unsupported" {
			t.Fatal("an older parsed declaration bypassed API compatibility")
		}
	})
}

func TestSelectHighestCompatibleVersion(t *testing.T) {
	t.Parallel()
	entry := protocol.DefaultPluginCatalogEntry()
	entry.Version = versionText("3.0.0")
	entry.Compatibility = Compatibility(versionText("999.0.0"))
	newer := catalogVersionFixture("3.0.0")
	newer.MinSailryVersion = versionText("999.0.0")
	future := catalogVersionFixture("2.0.0")
	future.APIVersion = versionText("v2")
	withdrawn := catalogVersionFixture("1.9.0")
	withdrawn.Withdrawn = true
	invalidMinimum := catalogVersionFixture("1.8.0")
	invalidMinimum.MinSailryVersion = versionText("later")
	invalidCommit := catalogVersionFixture("1.7.0")
	invalidCommit.Commit = "moving-branch"
	entry.Versions = []protocol.PluginCatalogVersion{
		catalogVersionFixture("1.2.0"), newer, future, withdrawn,
		invalidMinimum, invalidCommit, catalogVersionFixture("1.06.0"),
		catalogVersionFixture("1.4.0-beta.2"), catalogVersionFixture("1.3.0"),
	}
	for _, current := range []struct {
		name      string
		installed *string
		want      string
	}{
		{"new installation", nil, "1.3.0"},
		{"stable installation", versionText("1.2.0"), "1.3.0"},
		{"prerelease installation", versionText("1.4.0-beta.1"), "1.4.0-beta.2"},
	} {
		t.Run(current.name, func(t *testing.T) {
			t.Parallel()
			candidate := SelectCompatibleVersion(entry, current.installed)
			if candidate == nil || candidate.Version != current.want {
				t.Fatalf("candidate = %+v, want %s", candidate, current.want)
			}
		})
	}
	if candidate, err := officialVersion(entry, versionText("1.4.0-beta.2")); err != nil || candidate.Version != "1.4.0-beta.2" {
		t.Fatalf("explicit prerelease: %+v %v", candidate, err)
	}
	for _, version := range []string{"3.0.0", "2.0.0", "1.9.0", "1.8.0", "1.7.0", "1.06.0"} {
		if _, err := officialVersion(entry, &version); err == nil {
			t.Errorf("explicit incompatible version %s was accepted", version)
		}
	}
	for _, candidate := range []string{"1.3.0", "1.3.0+new", "1.2.9", "1.3.0-beta.1"} {
		if IsNewer(versionText("1.3.0+old"), &candidate) {
			t.Errorf("non-increasing precedence offered %s", candidate)
		}
	}
}
