package protocol

import (
	"encoding/json"
	"math"
	"reflect"
	"testing"
)

func TestConversationProviderBillingRateJSON(t *testing.T) {
	t.Parallel()
	const legacy = `{"id":"00000000-0000-0000-0000-000000000001","revision":1,"name":"Fixture","api":"responses","authentication":"api_key","endpoint":"https://fixture.invalid/v1","enabled":true,"default_model":""`
	zero, full, maxRate := uint32(0), DefaultBillingRateBps, uint32(math.MaxUint32)
	for _, tt := range []struct {
		name, wire string
		want       *uint32
		invalid    bool
	}{
		{name: "legacy omitted"},
		{name: "null is unset", wire: "null"},
		{name: "zero", wire: "0", want: &zero},
		{name: "explicit default", wire: "10000", want: &full},
		{name: "maximum", wire: "4294967295", want: &maxRate},
		{name: "negative", wire: "-1", invalid: true},
		{name: "overflow", wire: "4294967296", invalid: true},
		{name: "fraction", wire: "1.5", invalid: true},
		{name: "numeric string", wire: `"10000"`, invalid: true},
	} {
		t.Run(tt.name, func(t *testing.T) {
			body := legacy
			if tt.wire != "" {
				body += `,"billing_rate_bps":` + tt.wire
			}
			var provider ConversationProvider
			err := json.Unmarshal([]byte(body+"}"), &provider)
			if tt.invalid {
				if err == nil {
					t.Fatal("decoded an invalid billing rate")
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			if !reflect.DeepEqual(provider.BillingRateBps, tt.want) {
				t.Fatalf("decoded rate does not match %q", tt.wire)
			}
			captured := SessionProfile{Source: NodeID{2}, Provider: provider}
			raw, err := json.Marshal(captured)
			if err != nil {
				t.Fatal(err)
			}
			var members struct {
				Provider map[string]json.RawMessage `json:"provider"`
			}
			if err := json.Unmarshal(raw, &members); err != nil {
				t.Fatal(err)
			}
			wire, present := members.Provider["billing_rate_bps"]
			if present != (tt.want != nil) || present && string(wire) != tt.wire {
				t.Fatalf("captured rate = %q, present = %t", wire, present)
			}
			var restored SessionProfile
			if err := json.Unmarshal(raw, &restored); err != nil {
				t.Fatal(err)
			}
			if restored.Source != captured.Source || !reflect.DeepEqual(restored.Provider.BillingRateBps, tt.want) {
				t.Fatal("restored profile did not keep its source and captured billing rate")
			}
		})
	}
}
