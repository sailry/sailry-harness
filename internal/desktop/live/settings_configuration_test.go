package live

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A picked file is sent only as JSON within the size limit.
func TestReadConfigurationFile(t *testing.T) {
	dir := t.TempDir()
	write := func(name, content string) string {
		path := filepath.Join(dir, name)
		if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
			t.Fatal(err)
		}
		return path
	}
	if document, problem := readConfigurationFile(write("valid.json", `{"version":1}`)); problem != "" || string(document) != `{"version":1}` {
		t.Errorf("a valid file read %q with %q", document, problem)
	}
	if _, problem := readConfigurationFile(write("invalid.json", `{"version":`)); problem != "configuration_invalid" {
		t.Errorf("an invalid file read with %q", problem)
	}
	large := `"` + strings.Repeat("a", configurationFileLimit) + `"`
	if _, problem := readConfigurationFile(write("large.json", large)); problem != "configuration_too_large" {
		t.Errorf("a large file read with %q", problem)
	}
	if _, problem := readConfigurationFile(filepath.Join(dir, "missing.json")); problem != "configuration_read_failed" {
		t.Errorf("a missing file read with %q", problem)
	}
}

// An exported document is written indented, as the Node returned it.
func TestWriteConfigurationFile(t *testing.T) {
	path := filepath.Join(t.TempDir(), "sailry-configuration.json")
	if err := writeConfigurationFile(t.Context(), path, []byte(`{"version":1}`)); err != nil {
		t.Fatal(err)
	}
	if data, err := os.ReadFile(path); err != nil || string(data) != "{\n  \"version\": 1\n}\n" {
		t.Fatalf("wrote %q, %v", data, err)
	}
}

func TestConfigurationReport(t *testing.T) {
	reason := "plugin is not installed"
	items := configurationItems(protocol.ConfigurationImportReport{Items: []protocol.ConfigurationImportItem{
		{Area: "providers", Name: "OpenAI", Action: protocol.ConfigurationImportAdded},
		{Area: "plugins", Name: "git", Action: protocol.ConfigurationImportSkipped, Reason: &reason},
	}})
	want := []settings.ConfigurationItem{{Area: "providers", Name: "OpenAI", Action: "added"}, {Area: "plugins", Name: "git", Action: "skipped", Reason: reason}}
	if len(items) != 2 || items[0] != want[0] || items[1] != want[1] {
		t.Fatalf("report items %+v", items)
	}
	command := configurationImportCommand([]byte(`{"version":1}`), true)
	if payload, ok := command.Data.(protocol.CommandImportConfigurationPayload); command.Kind != "import_configuration" || !ok || !payload.Preview || string(payload.Document) != `{"version":1}` {
		t.Fatalf("import command %+v", command)
	}
	for code, want := range map[protocol.ErrorCode]kit.Outcome{
		protocol.ErrorCodeRevisionConflict: kit.Conflict,
		protocol.ErrorCodeOutcomeUnknown:   kit.Uncertain,
		protocol.ErrorCodeInvalidRequest:   kit.Failed,
		protocol.ErrorCodeInternal:         kit.Failed,
	} {
		if outcome, _ := configurationFailure(&protocol.Fault{Code: code}); outcome != want {
			t.Errorf("%s is outcome %v, want %v", code, outcome, want)
		}
	}
}
