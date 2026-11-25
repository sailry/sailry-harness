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
	if err != nil || bytes.Contains(oldWire, []byte(`"report_ref"`)) || bytes.Contains(oldWire, []byte(`"report_owned"`)) {
		t.Fatalf("fault without a reference = %s, %v", oldWire, err)
	}
	fault.ReportRef = "e11525db-c4b2-45ff-9b82-ff23c41f1b15"
	legacyWire, err := json.Marshal(fault)
	if err != nil || bytes.Contains(legacyWire, []byte(`"report_owned"`)) {
		t.Fatalf("fault without an ownership claim = %s, %v", legacyWire, err)
	}
	fault.ReportOwned = true
	newWire, err := json.Marshal(fault)
	if err != nil {
		t.Fatal(err)
	}
	var current Fault
	if err := DecodeAs(PeerData, newWire, &current); err != nil || !current.ReportOwned || current.ReportRef != fault.ReportRef || current.ConfigurationReason() != ConfigurationCredentialMissing {
		t.Fatalf("new peer fault = %+v, %v", current, err)
	}
	if err := DecodeAs(PeerData, legacyWire, &current); err != nil || current.ReportOwned || current.ReportRef != fault.ReportRef || current.ConfigurationReason() != ConfigurationCredentialMissing {
		t.Fatalf("legacy peer fault with a reference = %+v, %v", current, err)
	}
	if err := DecodeAs(PeerData, oldWire, &current); err != nil || current.ReportOwned || current.ReportRef != "" || current.Code != fault.Code || current.Message != fault.Message || current.ConfigurationReason() != ConfigurationCredentialMissing {
		t.Fatalf("old peer fault = %+v, %v", current, err)
	}
	var legacy struct {
		Code      ErrorCode     `json:"code"`
		Message   string        `json:"message"`
		Details   *FaultDetails `json:"details,omitempty"`
		ReportRef string        `json:"report_ref,omitempty"`
	}
	// The previous Fault decoder required code and message and allowed new fields.
	if err := DecodeObject(newWire, &legacy, []string{"code", "message"}, false); err != nil {
		t.Fatal(err)
	}
	got, err := json.Marshal(legacy)
	if err != nil || !bytes.Equal(got, legacyWire) {
		t.Fatalf("legacy decoder changed the original fault: %s, %v", got, err)
	}
}
