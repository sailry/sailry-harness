package protocol

import (
	"bytes"
	"encoding/json"
	"testing"
)

func TestFaultReportRefCompatibility(t *testing.T) {
	t.Parallel()
	fault := NewConfigurationFault(ConfigurationCredentialMissing, "provider credential unavailable")
	oldWire, err := json.Marshal(fault)
	if err != nil || bytes.Contains(oldWire, []byte(`"report_ref"`)) {
		t.Fatalf("fault without a reference = %s, %v", oldWire, err)
	}
	fault.ReportRef = "e11525db-c4b2-45ff-9b82-ff23c41f1b15"
	newWire, err := json.Marshal(fault)
	if err != nil {
		t.Fatal(err)
	}
	var current Fault
	if err := DecodeAs(PeerData, newWire, &current); err != nil || current.ReportRef != fault.ReportRef || current.ConfigurationReason() != ConfigurationCredentialMissing {
		t.Fatalf("new peer fault = %+v, %v", current, err)
	}
	if err := DecodeAs(PeerData, oldWire, &current); err != nil || current.ReportRef != "" || current.Code != fault.Code || current.Message != fault.Message || current.ConfigurationReason() != ConfigurationCredentialMissing {
		t.Fatalf("old peer fault = %+v, %v", current, err)
	}
	var legacy struct {
		Code    ErrorCode     `json:"code"`
		Message string        `json:"message"`
		Details *FaultDetails `json:"details,omitempty"`
	}
	// The previous Fault decoder required code and message and allowed new fields.
	if err := DecodeObject(newWire, &legacy, []string{"code", "message"}, false); err != nil {
		t.Fatal(err)
	}
	got, err := json.Marshal(legacy)
	if err != nil || !bytes.Equal(got, oldWire) {
		t.Fatalf("legacy decoder changed the original fault: %s, %v", got, err)
	}
}
