package protocol

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestPluginVersionOperationsPreserveSelection(t *testing.T) {
	info := DefaultPluginInfo()
	version, api := "1.4.0", "v999"
	info.Summary.Name, info.Summary.Version, info.Summary.Digest = "fixture", &version, strings.Repeat("a", 64)
	info.Summary.Pinned = true
	info.Requirements = &PluginRequirements{APIVersion: api, Invalid: true}
	info.UpdateSource = &PluginOrigin{Kind: "official"}
	info.Versions = []PluginCatalogVersion{{Version: version, Commit: strings.Repeat("b", 40), APIVersion: &api, Withdrawn: true,
		Compatibility: PluginCompatibility{Kind: "unsupported", Data: PluginCompatibilityUnsupportedPayload{Reason: "unsupported API"}}}}
	selection := PluginCatalogSelection{Info: info, Path: "fixture", Source: PluginSkillsResolved{Repository: "https://example.test/fixture.git", Commit: strings.Repeat("b", 40), GitRef: strings.Repeat("b", 40)}}
	command := Command{Kind: "install_catalog_plugin", Data: CommandInstallCatalogPluginPayload{Selection: selection, ExpectedRevision: 7, Mode: PluginInstallModeSwitch}}
	raw, err := json.Marshal(command)
	if err != nil {
		t.Fatal(err)
	}
	var decoded Command
	if err := json.Unmarshal(raw, &decoded); err != nil {
		t.Fatal(err)
	}
	value, ok := decoded.Data.(*CommandInstallCatalogPluginPayload)
	if !ok || !decoded.Durable() || value.ExpectedRevision != 7 || value.Mode != PluginInstallModeSwitch || value.Selection.Source != selection.Source {
		t.Fatalf("version operation lost its identity: %+v", decoded)
	}
	actual := value.Selection.Info
	if actual.Summary.Digest != info.Summary.Digest || !actual.Summary.Pinned || actual.UpdateSource == nil || actual.UpdateSource.Kind != "official" ||
		actual.Requirements == nil || !actual.Requirements.Invalid || actual.Requirements.APIVersion != api || len(actual.Versions) != 1 || !actual.Versions[0].Withdrawn || actual.Versions[0].Compatibility.Kind != "unsupported" {
		t.Fatalf("version requirements or withdrawal were lost: %+v", actual)
	}
	if !(Command{Kind: "set_plugin_pinned", Data: CommandSetPluginPinnedPayload{Name: "fixture", ExpectedRevision: 7, Pinned: true}}).Durable() {
		t.Fatal("pinning bypassed the durable request ledger")
	}
}
