package node

import (
	"errors"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConfigurationFollowRetiresManualCopyControls(t *testing.T) {
	for action, expected := range map[string]protocol.ErrorCode{
		"set_source":  protocol.ErrorCodeInvalidRequest,
		"set_target":  protocol.ErrorCodeInvalidRequest,
		"mcp_local":   protocol.ErrorCodeInvalidRequest,
		"detach":      protocol.ErrorCodeInvalidRequest,
		"apply":       protocol.ErrorCodeNotConfigured,
		"sync":        protocol.ErrorCodeNotConfigured,
		"kv_ack":      protocol.ErrorCodeNotConfigured,
		"kv_snapshot": protocol.ErrorCodeNotConfigured,
	} {
		t.Run(action, func(t *testing.T) {
			// Retired controls must be rejected before touching any stored
			// records, including when a legacy caller supplies their fields.
			source := protocol.NodeID{2}
			_, events, err := ExecuteConfigurationFollow(protocol.Command{
				Kind: "configuration_follow",
				Data: protocol.ConfigurationFollowCommand{
					Action: action, Source: &source, Target: &protocol.ConfigurationFollowTarget{Host: source},
					Plugin: "fixture", Server: "server", Local: true, Category: "providers",
					Manifest: &protocol.ConfigurationManifest{Source: source, Category: "runtime_credentials", Revision: 1, Records: []protocol.ConfigurationFollowRecord{}},
				},
			})
			var fault *protocol.Fault
			if !errors.As(err, &fault) || fault.Code != expected || len(events) != 0 {
				t.Fatalf("retired configuration control was accepted: events=%v, error=%v", events, err)
			}
		})
	}
}
