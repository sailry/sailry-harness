package providers

import (
	"encoding/json"
	"reflect"
	"testing"
)

func TestCatalogRejectsEmptySnapshot(t *testing.T) {
	if _, err := ParseCatalog([]byte(`{"fixture":{"models":{}}}`)); err == nil {
		t.Fatal("empty catalog could replace usable model metadata")
	}
}

func TestCatalogModelInputModalities(t *testing.T) {
	for _, row := range []struct {
		field string
		want  []string
	}{
		{field: ""},
		{field: `,"modalities":{"input":null}`},
		{field: `,"modalities":{"input":[]}`, want: []string{}},
		{field: `,"modalities":{"input":["text","image","video","audio","pdf"]}`, want: []string{"text", "image", "video", "audio", "pdf"}},
	} {
		model, err := CatalogModel(json.RawMessage(`{"id":"fixture","tool_call":false,"reasoning":true` + row.field + `}`))
		if err != nil {
			t.Fatal(err)
		}
		if !reflect.DeepEqual(model.Inputs, row.want) || model.Tools == nil || *model.Tools || model.Reasoning == nil || !*model.Reasoning {
			t.Fatalf("catalog metadata for %s = %+v, want inputs %#v and independent capabilities", row.field, model, row.want)
		}
	}
	for _, kind := range []string{"PDF", "application/pdf", " images ", "tool_call", "reasoning"} {
		if _, err := CatalogModel(json.RawMessage(`{"id":"fixture","modalities":{"input":["` + kind + `"]}}`)); err == nil {
			t.Errorf("catalog accepted unsupported input %q", kind)
		}
	}
}
