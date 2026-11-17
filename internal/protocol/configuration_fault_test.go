package protocol

import (
	"encoding/json"
	"testing"
)

func TestConfigurationFaultRoundTrip(t *testing.T) {
	t.Parallel()
	failure := NewConfigurationFault(ConfigurationCredentialMissing, "provider credential is unavailable")
	body, err := json.Marshal(failure)
	if err != nil {
		t.Fatal(err)
	}
	var decoded Fault
	if err := json.Unmarshal(body, &decoded); err != nil {
		t.Fatal(err)
	}
	if decoded.Code != failure.Code || decoded.Message != failure.Message || decoded.ConfigurationReason() != ConfigurationCredentialMissing {
		t.Fatalf("configuration refusal changed in storage: %+v", decoded)
	}
	unknown := []byte(`{"code":"not_configured","message":"future refusal","details":{"kind":"configuration","reason":"future_reason"}}`)
	if err := DecodeAs(PeerData, unknown, &decoded); err != nil {
		t.Fatal(err)
	}
	if reason := decoded.ConfigurationReason(); reason != "future_reason" || reason.Known() {
		t.Fatalf("unknown peer reason = %q", reason)
	}
	if err := DecodeAs(Authored, unknown, &decoded); err == nil {
		t.Fatal("unknown authored configuration reason accepted")
	}
	if err := json.Unmarshal([]byte(`{"kind":"configuration"}`), new(FaultDetails)); err == nil {
		t.Fatal("configuration detail without a reason accepted")
	}
}
