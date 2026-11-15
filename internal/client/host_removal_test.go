package client

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestHostRemovalInspectsOriginalUnknownRequest(t *testing.T) {
	t.Parallel()
	for _, outcome := range []string{"admitted", "unknown", "not_admitted"} {
		t.Run(outcome, func(t *testing.T) {
			transport := &clientTransport{}
			remote := New(transport)
			defer remote.Close()
			request := remote.Prepare(protocol.Command{Kind: "identity", Data: protocol.IdentityCommand{Kind: "revoke_device", Data: protocol.RevokeDevice{Device: protocol.NodeID{2}, ConfirmLast: true}}})
			mutations := 0
			transport.dispatch = func(actual protocol.Request) (link.Admission, error) {
				completed := make(chan link.Response, 1)
				if actual.Command.Kind == "inspect_request" {
					completed <- link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: request.ID, Outcome: protocol.RequestOutcome{Kind: outcome}}}}
				} else {
					mutations++
					if actual.ID != request.ID {
						t.Error("self-revocation changed its original identity")
					}
					completed <- link.Response{Output: protocol.Output{Kind: "device_revoked", Data: protocol.DeviceRevoked{Device: protocol.NodeID{2}}}}
				}
				close(completed)
				return link.Admission{Receipt: protocol.Receipt{ID: actual.ID, Durable: true}, Completion: completed}, nil
			}
			states := []string{}
			result := notifyHostRemoval(t.Context(), remote, link.HostRemovalAttempt{Request: request, State: "unknown", Active: true}, func(state string, _ *protocol.Fault) error { states = append(states, state); return nil })
			if result.Request.ID != request.ID || len(states) == 0 {
				t.Fatal("original request/outcome was not retained")
			}
			want := 0
			if outcome == "not_admitted" {
				want = 1
			}
			if mutations != want {
				t.Fatalf("unknown removal %s replayed %d effects, want %d", outcome, mutations, want)
			}
			if outcome != "not_admitted" && result.State != "unknown" {
				t.Fatal("pending/terminal-unknown effect was relabeled as completed")
			}
		})
	}
}
