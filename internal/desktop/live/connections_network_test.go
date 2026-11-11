package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// A network change the Node has no record of is decided by the settings the
// Node holds, so the editor never waits on an answer that cannot come.
func TestSettleConnectionNetwork(t *testing.T) {
	selected := protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{"https://relay.example"}}}
	previous := protocol.NetworkSelection{Kind: "iroh"}
	for _, row := range []struct {
		name            string
		stored, running *protocol.NetworkSelection
		code            protocol.ErrorCode
	}{
		{"in_effect", &selected, &selected, ""},
		{"not_applied", &previous, &previous, protocol.ErrorCodeInternal},
		{"never_selected", nil, nil, protocol.ErrorCodeInternal},
		{"running_only", &previous, &selected, protocol.ErrorCodeRevisionConflict},
		{"stored_only", &selected, &previous, protocol.ErrorCodeRevisionConflict},
	} {
		t.Run(row.name, func(t *testing.T) {
			err := settleConnectionNetwork(protocol.NetworkSettings{Selection: row.stored, Effective: row.running}, selected)
			if row.code == "" {
				if err != nil {
					t.Fatalf("change in effect was reported as %v", err)
				}
				return
			}
			if faultCode(err) != row.code {
				t.Fatalf("fault = %v, want %s", err, row.code)
			}
			if faultCode(err) == protocol.ErrorCodeOutcomeUnknown {
				t.Fatal("a decided change must release the editor")
			}
		})
	}
}
