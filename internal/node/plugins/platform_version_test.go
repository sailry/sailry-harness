package plugins

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCompatibilityUsesHarnessPlatformVersion(t *testing.T) {
	previous := link.ProductVersion
	link.ProductVersion = "0.1.0"
	t.Cleanup(func() { link.ProductVersion = previous })
	minimum := "0.2.14"
	info := protocol.DefaultPluginInfo()
	info.Requirements = &protocol.PluginRequirements{APIVersion: "v1", MinSailryVersion: &minimum}
	if compatibility := InfoCompatibility(info, Runtime{ProductID: "code"}); compatibility.Kind != "compatible" {
		t.Fatalf("Code 0.1.0 rejected Harness %s: %+v", minimum, compatibility)
	}
	version := protocol.DefaultPluginCatalogVersion()
	version.Version, version.Commit, version.MinSailryVersion = "1.0.0", "0123456789012345678901234567890123456789", &minimum
	if compatibility := CatalogVersionCompatibility(version, Runtime{ProductID: "code"}); compatibility.Kind != "compatible" {
		t.Fatalf("catalog used marketing version: %+v", compatibility)
	}
	future := "0.3.1"
	if compatibility := Compatibility(&future); compatibility.Kind != "requires_newer" {
		t.Fatalf("future platform requirement was ignored: %+v", compatibility)
	}
}
