package plugins

import (
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAssessUpdateUsesNodePolicy(t *testing.T) {
	t.Parallel()
	skill := protocol.PluginSkillsProvenance{Kind: "Git", Data: protocol.PluginSkillsGitProvenance{
		Source: protocol.PluginSkillsResolved{Repository: "https://example.test/skills.git", GitRef: "main", Commit: strings.Repeat("a", 40)}, Path: "review",
	}}
	for _, current := range []struct {
		name      string
		change    func(*protocol.PluginInfo, *protocol.PluginInfo)
		available bool
		manual    bool
		refused   bool
	}{
		{name: "higher stable", available: true},
		{name: "pinned", change: func(before, _ *protocol.PluginInfo) { before.Summary.Pinned = true }},
		{name: "stable to prerelease", change: func(_, after *protocol.PluginInfo) { after.Summary.Version = versionText("1.1.0-beta.1") }},
		{name: "prerelease to prerelease", available: true, change: func(before, after *protocol.PluginInfo) {
			before.Summary.Version, after.Summary.Version = versionText("1.1.0-beta.1"), versionText("1.1.0-beta.2")
		}},
		{name: "prerelease to stable", available: true, change: func(before, _ *protocol.PluginInfo) { before.Summary.Version = versionText("1.1.0-beta.1") }},
		{name: "same precedence", change: func(_, after *protocol.PluginInfo) { after.Summary.Version = versionText("1.0.0+build.2") }},
		{name: "older version", change: func(_, after *protocol.PluginInfo) { after.Summary.Version = versionText("0.9.0") }},
		{name: "candidate version missing", change: func(_, after *protocol.PluginInfo) { after.Summary.Version = nil }},
		{name: "candidate version invalid", change: func(_, after *protocol.PluginInfo) { after.Summary.Version = versionText("later") }},
		{name: "current version missing", manual: true, change: func(before, _ *protocol.PluginInfo) { before.Summary.Version = nil }},
		{name: "current version invalid", manual: true, change: func(before, _ *protocol.PluginInfo) { before.Summary.Version = versionText("earlier") }},
		{name: "different identity", refused: true, change: func(_, after *protocol.PluginInfo) { after.Summary.Name = "another-fixture" }},
		{name: "unsupported API", change: func(_, after *protocol.PluginInfo) {
			after.Requirements = &protocol.PluginRequirements{APIVersion: "v2"}
		}},
		{name: "newer Node required", change: func(_, after *protocol.PluginInfo) {
			after.Requirements = &protocol.PluginRequirements{APIVersion: "v1", MinSailryVersion: versionText("999.0.0")}
		}},
		{name: "invalid declaration", change: func(_, after *protocol.PluginInfo) {
			after.Requirements = &protocol.PluginRequirements{APIVersion: "v1", Invalid: true}
		}},
		{name: "stale compatibility summary", available: true, change: func(before, after *protocol.PluginInfo) {
			before.Summary.Compatibility = unsupportedCompatibility("stale controller result")
			after.Summary.Compatibility = Compatibility(versionText("999.0.0"))
		}},
		{name: "standalone skill changed digest", available: true, change: func(before, after *protocol.PluginInfo) {
			before.Skill, after.Skill = &skill, &skill
			before.Summary.Version, after.Summary.Version = nil, versionText("1.1.0-beta.1")
		}},
		{name: "standalone skill same digest", change: func(before, after *protocol.PluginInfo) {
			before.Skill, after.Skill = &skill, &skill
			after.Summary.Digest = before.Summary.Digest
		}},
	} {
		t.Run(current.name, func(t *testing.T) {
			t.Parallel()
			before, after := protocol.DefaultPluginInfo(), protocol.DefaultPluginInfo()
			before.Summary.Name, before.Summary.Version, before.Summary.Digest = "update-fixture", versionText("1.0.0"), strings.Repeat("a", 64)
			after.Summary.Name, after.Summary.Version, after.Summary.Digest = "update-fixture", versionText("1.1.0"), strings.Repeat("b", 64)
			if current.change != nil {
				current.change(&before, &after)
			}
			report, err := AssessUpdate(before, after)
			if (err != nil) != current.refused || (report.Available != nil) != current.available || report.Manual != current.manual || report.Blocked != nil {
				t.Fatalf("update assessment: %+v %v", report, err)
			}
			if report.Current.Compatibility.Kind != "compatible" {
				t.Fatalf("current compatibility was not recomputed: %+v", report.Current.Compatibility)
			}
			if report.Available != nil {
				candidate, ok := report.Available.Data.(protocol.PluginInfo)
				if !ok || report.Available.Kind != "package" || candidate.Summary.Compatibility.Kind != "compatible" {
					t.Fatalf("candidate compatibility was not recomputed: %+v", report.Available)
				}
			}
		})
	}
}
