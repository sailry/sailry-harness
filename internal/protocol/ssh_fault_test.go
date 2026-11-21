package protocol

import (
	"encoding/json"
	"testing"
)

func TestSSHFailureReasonCompatibility(t *testing.T) {
	fault := Fault{Code: ErrorCodeOutcomeUnknown, Message: "SSH command receipt is unavailable", SSHReason: SSHCommandReceiptLost}
	wire, err := json.Marshal(fault)
	if err != nil {
		t.Fatal(err)
	}
	var decoded Fault
	if err := DecodeAs(PeerData, wire, &decoded); err != nil || decoded != fault || decoded.SSHReason.Stage() != "start_command" {
		t.Fatalf("SSH fault = %+v, %v", decoded, err)
	}
	for _, body := range []string{
		`{"code":"outcome_unknown","message":"SSH failed"}`,
		`{"code":"outcome_unknown","message":"SSH failed","ssh_reason":"future_reason"}`,
	} {
		if err := DecodeAs(PeerData, []byte(body), &decoded); err != nil || decoded.Code != ErrorCodeOutcomeUnknown || decoded.SSHReason.Stage() != "" {
			t.Fatalf("compatible SSH fault = %+v, %v", decoded, err)
		}
	}
}
