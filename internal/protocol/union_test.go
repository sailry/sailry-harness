package protocol

import (
	"encoding/json"
	"reflect"
	"testing"
)

func TestUnionUnitVariant(t *testing.T) {
	t.Parallel()
	variants := variantFactories{"ready": nil}
	for _, item := range []struct {
		tag, content, encoded string
	}{
		{"kind", "data", `{"kind":"ready"}`},
		{"action", "", `{"action":"ready"}`},
		{"", "", `"ready"`},
	} {
		encoded, err := MarshalUnion(item.tag, item.content, "ready", nil, variants)
		if err != nil || string(encoded) != item.encoded {
			t.Fatalf("encode: %s, %v", encoded, err)
		}
		kind, value, err := UnmarshalUnion(encoded, item.tag, item.content, variants, true)
		if err != nil || kind != "ready" || value != nil {
			t.Fatalf("decode: %q, %v, %v", kind, value, err)
		}
	}
	var empty []Output
	for _, item := range []struct {
		name  string
		value any
		want  string
	}{
		{"vector", Output{Kind: "plugin_transaction", Data: empty}, `{"kind":"plugin_transaction","data":[]}`},
		{"boxed vector", Output{Kind: "plugin_transaction", Data: &empty}, `{"kind":"plugin_transaction","data":[]}`},
		{"map", ConversationQuestionAnswer{Kind: "form", Data: map[string]json.RawMessage(nil)}, `{"kind":"form","data":{}}`},
		{"nested vectors", ToolContentTable{Rows: [][]string{nil}}, `{"columns":[],"rows":[[]],"truncated":false}`},
		{"nested map values", PluginHttpResponse{Headers: map[string][]string{"X-Test": nil}}, `{"status":0,"headers":{"X-Test":[]},"body":""}`},
		{"unit result", Result[Unit, Fault]{Ok: new(Unit)}, `{"Ok":null}`},
	} {
		t.Run(item.name, func(t *testing.T) {
			encoded, err := json.Marshal(item.value)
			if err != nil {
				t.Fatal(err)
			}
			if !reflect.DeepEqual(fixtureJSON(t, encoded), fixtureJSON(t, []byte(item.want))) {
				t.Fatalf("wire shape changed: got %s, want %s", encoded, item.want)
			}
		})
	}
}

func TestUnionUnknownKind(t *testing.T) {
	t.Parallel()
	variants := variantFactories{"ready": nil}
	for _, item := range []struct {
		tag, content, encoded string
	}{
		{"kind", "data", `{"kind":"unknown"}`},
		{"type", "", `{"type":"unknown"}`},
		{"", "", `"unknown"`},
	} {
		if _, _, err := UnmarshalUnion([]byte(item.encoded), item.tag, item.content, variants, false); err == nil {
			t.Fatalf("accepted %s", item.encoded)
		}
		if _, err := MarshalUnion(item.tag, item.content, "unknown", nil, variants); err == nil {
			t.Fatal("encoded unknown variant")
		}
	}
}
