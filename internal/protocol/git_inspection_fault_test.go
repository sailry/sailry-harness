package protocol

import (
	"encoding/json"
	"testing"
)

func TestGitInspectionFaultCompatibility(t *testing.T) {
	t.Parallel()
	fault := Fault{
		Code: ErrorCodeUnavailable, Message: "Git status contains unsupported path encoding",
		ReportRef: "e11525db-c4b2-45ff-9b82-ff23c41f1b15", GitInspectionReason: GitInspectionUnsupportedEncoding,
	}
	wire, err := json.Marshal(fault)
	if err != nil {
		t.Fatal(err)
	}
	var current Fault
	if err := DecodeAs(PeerData, wire, &current); err != nil || current != fault {
		t.Fatalf("current peer fault = %+v, %v", current, err)
	}
	var legacy struct {
		Code      ErrorCode     `json:"code"`
		Message   string        `json:"message"`
		Details   *FaultDetails `json:"details,omitempty"`
		ReportRef string        `json:"report_ref,omitempty"`
	}
	if err := DecodeObject(wire, &legacy, []string{"code", "message"}, false); err != nil {
		t.Fatal(err)
	}
	oldWire, err := json.Marshal(legacy)
	if err != nil {
		t.Fatal(err)
	}
	fault.GitInspectionReason = ""
	if err := DecodeAs(PeerData, oldWire, &current); err != nil || current != fault {
		t.Fatalf("legacy peer fault = %+v, %v", current, err)
	}
	unknown := []byte(`{"code":"unavailable","message":"Git failed","git_inspection_reason":"future_reason"}`)
	if err := DecodeAs(PeerData, unknown, &current); err != nil || current.Code != ErrorCodeUnavailable || current.GitInspectionReason.Stage() != "" {
		t.Fatalf("peer fault with an unknown reason = %+v, %v", current, err)
	}
}
