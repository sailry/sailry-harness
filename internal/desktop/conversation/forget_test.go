package conversation

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestForgetSessionReleasesPerSessionState(t *testing.T) {
	s := New()
	s.anchors = map[string]transcriptAnchor{"sess-a": {turn: "t1"}, "sess-b": {turn: "t2"}}
	s.edits = map[string]*edit{"sess-a": {turn: "t1"}, "sess-b": {turn: "t2"}}
	s.references = map[string]map[string][]Reference{
		"sess-a": {"t1": {{Label: "@a"}}},
		"sess-b": {"t2": {{Label: "@b"}}},
	}
	s.rewind, s.rewindOf = "t1", "sess-a"

	s.setOpen("sess-a", "tools-1", true)
	s.setOpen("sess-b", "tools-2", true)
	s.revealOpen = "tools-1"
	s.setWork("sess-a", "work-t1", true)
	s.setWork("sess-b", "work-t2", false)

	s.Forget("sess-a")

	if _, ok := s.anchors["sess-a"]; ok {
		t.Fatal("Forget retained anchor for sess-a")
	}
	if _, ok := s.edits["sess-a"]; ok {
		t.Fatal("Forget retained edit for sess-a")
	}
	if _, ok := s.references["sess-a"]; ok {
		t.Fatal("Forget retained references for sess-a")
	}
	if s.rewind != "" || s.rewindOf != "" {
		t.Fatal("Forget retained rewind target for sess-a")
	}
	if s.open["tools-1"] || s.revealOpen != "" || len(s.openOwner["sess-a"]) != 0 {
		t.Fatal("Forget retained disclosure state for sess-a")
	}
	if _, ok := s.work["work-t1"]; ok || len(s.workOwner["sess-a"]) != 0 {
		t.Fatal("Forget retained work toggle for sess-a")
	}
	// Another session's state stays untouched.
	if s.anchors["sess-b"].turn != "t2" || !s.open["tools-2"] || s.work["work-t2"] != false {
		t.Fatal("Forget disturbed another session's state")
	}
	var _ ui.Element
}
