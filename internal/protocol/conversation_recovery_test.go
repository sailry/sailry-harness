package protocol

import (
	"encoding/json"
	"testing"
)

func TestRecoveryBudgetReasons(t *testing.T) {
	for _, reason := range []ConversationRecoveryStop{ConversationRecoveryStopAttemptsExhausted, ConversationRecoveryStopWaitExhausted} {
		t.Run(string(reason), func(t *testing.T) {
			fault := Fault{Code: ErrorCodeUnavailable, Message: "provider busy", Details: &FaultDetails{
				Kind: "model_recovery", Data: FaultDetailsModelRecoveryPayload{Reason: reason},
			}}
			encoded, err := json.Marshal(fault)
			if err != nil {
				t.Fatal(err)
			}
			var restored Fault
			if err := DecodeAs(Authored, encoded, &restored); err != nil {
				t.Fatal(err)
			}
			if restored.Details == nil || restored.Details.Kind != "model_recovery" {
				t.Fatalf("recovery details were lost: %+v", restored)
			}
			value, ok := restored.Details.Data.(*FaultDetailsModelRecoveryPayload)
			if !ok || value.Reason != reason {
				t.Fatalf("recovery reason changed: %#v", restored.Details.Data)
			}
			text, err := reason.MarshalText()
			if err != nil {
				t.Fatal(err)
			}
			var decoded ConversationRecoveryStop
			if err := decoded.UnmarshalText(text); err != nil || decoded != reason {
				t.Fatalf("text recovery reason changed: %q, %v", decoded, err)
			}
		})
	}
	var reason ConversationRecoveryStop
	if err := json.Unmarshal([]byte(`"unknown_budget"`), &reason); err == nil {
		t.Fatal("an undeclared authored recovery reason was accepted")
	}
}
