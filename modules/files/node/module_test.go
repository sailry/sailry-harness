package filesnode

import (
	"context"
	"encoding/json"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPrepareLeavesOmittedRevisionToNode(t *testing.T) {
	revision := strings.Repeat("a", 64)
	for _, test := range []struct {
		tool, args string
		revision   any
		present    bool
	}{
		{"edit_file", `{"path":"a","old_text":"x","new_text":"y"}`, nil, false},
		{"edit_file", `{"path":"a","old_text":"x","new_text":"y","expected_revision":"` + revision + `"}`, revision, true},
		{"delete_file", `{"path":"a"}`, nil, false},
		{"move_file", `{"path":"a","destination":"b"}`, nil, false},
		{"edit_notebook", `{"path":"a.ipynb","edits":[{"kind":"delete","index":0}]}`, nil, false},
		{"write_file", `{"path":"a","text":"x"}`, nil, false},
		{"write_file", `{"path":"a","text":"x","expected_revision":null}`, nil, true},
	} {
		raw, err := prepareFor(test.tool)(context.Background(), json.RawMessage(test.args))
		if err != nil {
			t.Fatalf("%s %s: %v", test.tool, test.args, err)
		}
		var out map[string]any
		if err := json.Unmarshal(raw, &out); err != nil {
			t.Fatal(err)
		}
		got, present := out["expected_revision"]
		if present != test.present || got != test.revision {
			t.Fatalf("%s %s: expected_revision %v (present %v)", test.tool, test.args, got, present)
		}
	}
	if _, err := prepareEdit(context.Background(), json.RawMessage(`{"path":"a","old_text":"x","new_text":"y","expected_revision":"short"}`)); err == nil {
		t.Fatal("a malformed revision was accepted")
	}
}

// Edits declare standalone rows, so a transcript shows the changes a turn
// made apart from the reads and searches it groups.
func TestCatalogGroupsEditsApart(t *testing.T) {
	edits := []string{"write_file", "edit_file", "apply_patch", "delete_file", "move_file", "edit_notebook"}
	for _, meta := range Module().Catalog() {
		want := protocol.ToolGroupingSequence
		if slices.Contains(edits, meta.Name) {
			want = protocol.ToolGroupingStandalone
		}
		if meta.Grouping != want {
			t.Errorf("%s grouping = %q, want %q", meta.Name, meta.Grouping, want)
		}
	}
}

func TestModelSchemaHidesAutoTrackedRevision(t *testing.T) {
	catalog, err := loadCatalog()
	if err != nil {
		t.Fatal(err)
	}
	hidden := map[string]bool{"edit_file": true, "delete_file": true, "move_file": true, "edit_notebook": true}
	for _, spec := range catalog.Tools {
		var schema struct {
			Properties map[string]any `json:"properties"`
		}
		if err := json.Unmarshal(spec.Parameters, &schema); err != nil {
			t.Fatalf("%s schema: %v", spec.Name, err)
		}
		_, hasRev := schema.Properties["expected_revision"]
		if hidden[spec.Name] && hasRev {
			t.Errorf("%s should omit expected_revision from the model-facing schema", spec.Name)
		}
		if spec.Name == "write_file" && !hasRev {
			t.Error("write_file must keep expected_revision to distinguish create from replace")
		}
	}
}
