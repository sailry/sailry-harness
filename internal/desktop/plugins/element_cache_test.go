package plugins

import (
	"encoding/json"
	"testing"
)

// A widget's key reads its naming props as the props map does, and an
// element parses its props once.
func TestElementCacheKeysAndParsesOnce(t *testing.T) {
	mounted := &Mounted{}
	for _, row := range []struct {
		props  string
		styles []ElementStyle
		key    string
	}{
		{`{"id":"save","motion_id":"m"}`, nil, "kit/Button/save"},
		{`{"ID":"ignored","motion_id":"m"}`, nil, "kit/Button/m"},
		{`{"state":{"__coreState":3}}`, nil, "kit/Button/state:3"},
		{`{"__ordinal":2}`, nil, "kit/Button/position:2"},
		{`{"id":"props"}`, []ElementStyle{{Name: "id", Arguments: []json.RawMessage{json.RawMessage(`"style"`)}}}, "kit/Button/style"},
		{`["positional",{"id":"x"}]`, nil, "kit/Button/7"},
		{``, nil, "kit/Button/7"},
	} {
		element := &NativeElement{ID: 7, Module: "kit", Export: "Button", Props: json.RawMessage(row.props), Styles: row.styles}
		if got := mounted.widgetKey(element); got != row.key {
			t.Errorf("widgetKey(%s) = %q, want %q", row.props, got, row.key)
		}
	}
	element := &NativeElement{ID: 1, Props: json.RawMessage(`{"label":"Run","open":true}`)}
	first := runtimeProperties(element)
	if runtimeString(first["label"]) != "Run" || !runtimeBool(first["open"]) {
		t.Fatalf("properties = %v", first)
	}
	if allocs := testing.AllocsPerRun(100, func() { runtimeProperties(element); mounted.widgetKey(element) }); allocs != 0 {
		t.Fatalf("a kept element allocated %v times per read", allocs)
	}
	if _, _, err := nativeElementProps(element); err != nil {
		t.Fatal(err)
	}
	if allocs := testing.AllocsPerRun(100, func() { _, _, _ = nativeProps(*element) }); allocs != 0 {
		t.Fatalf("native props of a kept element allocated %v times per read", allocs)
	}
}
