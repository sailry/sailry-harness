package preferences

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestViewZoomPreference(t *testing.T) {
	t.Parallel()
	d := Default()
	encoded, err := json.Marshal(d)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(encoded), "view_zoom") {
		t.Fatal("an unset view zoom was written")
	}
	for _, level := range ViewZoomLevels {
		d.ViewZoom = &level
		b, err := json.Marshal(d)
		if err != nil {
			t.Fatal(err)
		}
		var back Data
		if err := json.Unmarshal(b, &back); err != nil || back.ViewZoom == nil || *back.ViewZoom != level {
			t.Fatalf("view zoom %d came back as %v: %v", level, back.ViewZoom, err)
		}
	}
	for _, bad := range []uint16{0, 90, 105, 200} {
		d.ViewZoom = &bad
		if d.Validate() == nil {
			t.Errorf("view zoom %d validated", bad)
		}
	}
}
