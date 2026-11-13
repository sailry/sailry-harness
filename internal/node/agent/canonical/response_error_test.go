package canonical

import (
	"encoding/json"
	"testing"
)

func TestResponseErrorRetryBudgetCompatibility(t *testing.T) {
	for _, test := range []struct {
		name     string
		input    string
		attempts bool
		waiting  bool
	}{
		{"before_budget_flags", `{"code":"unavailable","message":"provider busy"}`, false, false},
		{"attempts_exhausted", `{"code":"unavailable","message":"provider busy","retry_exhausted":true}`, true, false},
		{"wait_exhausted", `{"code":"unavailable","message":"provider busy","retry_wait_exhausted":true}`, false, true},
	} {
		t.Run(test.name, func(t *testing.T) {
			var stored ResponseError
			if err := json.Unmarshal([]byte(test.input), &stored); err != nil {
				t.Fatal(err)
			}
			if stored.Code == nil || *stored.Code != "unavailable" || stored.Message != "provider busy" || stored.RetryExhausted != test.attempts || stored.RetryWaitExhausted != test.waiting {
				t.Fatalf("stored failure changed its cause or retry budget: %+v", stored)
			}
			encoded, err := json.Marshal(stored)
			if err != nil {
				t.Fatal(err)
			}
			var fields map[string]json.RawMessage
			if err := json.Unmarshal(encoded, &fields); err != nil {
				t.Fatal(err)
			}
			if (fields["retry_exhausted"] != nil) != test.attempts || (fields["retry_wait_exhausted"] != nil) != test.waiting {
				t.Fatalf("serialization invented or dropped a budget flag: %s", encoded)
			}
		})
	}
}
