package protocol

import (
	"encoding/json"
	"reflect"
	"slices"
	"testing"
)

func TestConversationModelInputsWire(t *testing.T) {
	for _, row := range []struct {
		name, fields, wireInputs string
		inputs, accepts          []string
	}{
		{name: "legacy text", fields: `,"vision":false`, accepts: []string{"text"}},
		{name: "legacy vision", fields: `,"vision":true`, accepts: []string{"text", "image"}},
		{name: "null", fields: `,"inputs":null,"vision":true`, accepts: []string{"text", "image"}},
		{name: "explicit empty", fields: `,"inputs":[],"vision":true`, wireInputs: `[]`, inputs: []string{}},
		{name: "explicit supported kinds", fields: `,"inputs":["text","image","video","audio","pdf"],"vision":false`, wireInputs: `["text","image","video","audio","pdf"]`,
			inputs: []string{"text", "image", "video", "audio", "pdf"}, accepts: []string{"text", "image", "video", "audio", "pdf"}},
		{name: "no implied text or image", fields: `,"inputs":["audio","pdf"],"vision":true`, wireInputs: `["audio","pdf"]`, inputs: []string{"audio", "pdf"}, accepts: []string{"audio", "pdf"}},
	} {
		t.Run(row.name, func(t *testing.T) {
			var model ConversationModel
			if err := json.Unmarshal([]byte(`{"id":"fixture","context":4096,"output":1024`+row.fields+`}`), &model); err != nil {
				t.Fatal(err)
			}
			if !reflect.DeepEqual(model.Inputs, row.inputs) {
				t.Fatalf("inputs = %#v, want %#v", model.Inputs, row.inputs)
			}
			for _, kind := range []string{"text", "image", "video", "audio", "pdf", "PDF", "application/pdf", "tool_call", "reasoning"} {
				if got, want := model.AcceptsInput(kind), slices.Contains(row.accepts, kind); got != want {
					t.Errorf("AcceptsInput(%q) = %v, want %v", kind, got, want)
				}
			}
			wantVision := slices.Contains(row.accepts, "image")
			if model.Vision != wantVision {
				t.Fatalf("decoded vision = %v, want %v", model.Vision, wantVision)
			}
			if model.Inputs != nil {
				model.Vision = !wantVision // A stale legacy flag must not affect the explicit list on the wire.
			}
			wire, err := json.Marshal(model)
			if err != nil {
				t.Fatal(err)
			}
			var fields map[string]json.RawMessage
			if err := json.Unmarshal(wire, &fields); err != nil {
				t.Fatal(err)
			}
			if got := string(fields["inputs"]); got != row.wireInputs {
				t.Errorf("wire inputs = %q, want %q", got, row.wireInputs)
			}
			var vision bool
			if err := json.Unmarshal(fields["vision"], &vision); err != nil || vision != wantVision {
				t.Errorf("wire vision = %s (err %v), want %v", fields["vision"], err, wantVision)
			}
		})
	}
}

func TestConversationModelInputsRejectMalformedLists(t *testing.T) {
	for _, fields := range []string{`,"inputs":"image"`, `,"inputs":[null]`, `,"inputs":null,"inputs":[]`, `,"efforts":null`} {
		var model ConversationModel
		if err := json.Unmarshal([]byte(`{"id":"fixture","context":4096,"output":1024`+fields+`}`), &model); err == nil {
			t.Errorf("invalid model decoded with %s", fields)
		}
	}
}
