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

func TestPrepareOfficeAndPDF(t *testing.T) {
	if _, err := prepareOffice(context.Background(), json.RawMessage(`{"source":{"kind":"path","value":"report.docx"},"offset":2}`)); err != nil {
		t.Fatalf("valid office args rejected: %v", err)
	}
	for _, bad := range []string{
		`{"source":{"kind":"path","value":"report.pdf"},"pages":[1]}`,
		`{"source":{"kind":"path","value":"report.pdf"},"mode":"images"}`,
	} {
		if _, err := prepareOffice(context.Background(), json.RawMessage(bad)); err == nil {
			t.Errorf("read_office should reject PDF-only args: %s", bad)
		}
	}

	raw, err := preparePDF(context.Background(), json.RawMessage(`{"source":{"kind":"path","value":"report.pdf"}}`))
	if err != nil {
		t.Fatalf("default read_pdf args rejected: %v", err)
	}
	var def protocol.OfficeRead
	if err := json.Unmarshal(raw, &def); err != nil {
		t.Fatalf("unmarshal default read_pdf args: %v", err)
	}
	if def.Mode != "text" || len(def.Pages) != 0 {
		t.Fatalf("default read_pdf = %+v, want mode=text pages=[]", def)
	}

	raw, err = preparePDF(context.Background(), json.RawMessage(`{"source":{"kind":"path","value":"report.pdf"},"mode":"images","pages":[1,3]}`))
	if err != nil {
		t.Fatalf("valid image mode pages rejected: %v", err)
	}
	var out protocol.OfficeRead
	if err := json.Unmarshal(raw, &out); err != nil {
		t.Fatalf("unmarshal prepared pdf args: %v", err)
	}
	if out.Mode != "images" || !slices.Equal(out.Pages, []uint32{1, 3}) {
		t.Fatalf("pdf = %+v, want mode=images pages=[1 3]", out)
	}

	// Text mode allows selecting more than 4 pages at once.
	if _, err := preparePDF(context.Background(), json.RawMessage(`{"source":{"kind":"path","value":"report.pdf"},"mode":"text","pages":[1,2,3,4,5]}`)); err != nil {
		t.Fatalf("text mode 5 pages rejected: %v", err)
	}
	// Images mode allows omitting pages (uses offset).
	if _, err := preparePDF(context.Background(), json.RawMessage(`{"source":{"kind":"path","value":"report.pdf"},"mode":"images","offset":4}`)); err != nil {
		t.Fatalf("images mode with offset rejected: %v", err)
	}

	for _, bad := range []string{
		`{"source":{"kind":"path","value":"report.pdf"},"mode":"raw"}`,
		`{"source":{"kind":"path","value":"report.pdf"},"mode":"images","pages":[0]}`,
		`{"source":{"kind":"path","value":"report.pdf"},"mode":"images","pages":[1,2,3,4,5]}`,
		`{"source":{"kind":"path","value":"report.pdf"},"pages":"1"}`,
	} {
		if _, err := preparePDF(context.Background(), json.RawMessage(bad)); err == nil {
			t.Errorf("invalid read_pdf accepted: %s", bad)
		}
	}
}
