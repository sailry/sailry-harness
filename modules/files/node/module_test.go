package filesnode

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
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
