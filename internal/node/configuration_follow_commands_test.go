package node

import (
	"errors"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConfigurationFollowRetiresManualCopyControls(t *testing.T) {
	for _, action := range []string{"set_source", "set_target", "mcp_local", "detach"} {
		t.Run(action, func(t *testing.T) {
			// Retired controls must be rejected before touching any stored
			// records, including when a legacy caller supplies their fields.
			source := protocol.NodeID{2}
			_, events, err := ExecuteConfigurationFollow(t.Context(), nil, protocol.NodeID{1}, source, protocol.Command{
				Kind: "configuration_follow",
				Data: protocol.ConfigurationFollowCommand{
					Action: action, Source: &source, Target: &protocol.ConfigurationFollowTarget{Host: source},
					Plugin: "fixture", Server: "server", Local: true, Category: "providers",
				},
			})
			var fault *protocol.Fault
			if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeInvalidRequest || len(events) != 0 {
				t.Fatalf("retired configuration control was accepted: events=%v, error=%v", events, err)
			}
		})
	}
}
