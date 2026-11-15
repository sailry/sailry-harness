package node

import (
	"github.com/sailry/sailry-harness/internal/protocol"
	"testing"
)

func TestCommandAuthorizationCoverage(t *testing.T) {
	registered := map[string]bool{}
	for _, kind := range protocol.CommandKinds() {
		registered[kind] = true
		if commandPolicies[kind] == 0 {
			t.Errorf("command %s has no authorization policy", kind)
		}
	}
	for kind := range commandPolicies {
		if !registered[kind] {
			t.Errorf("authorization policy names unregistered command %s", kind)
		}
	}
	if _, ok := commandPolicies["unregistered_command"]; ok {
		t.Fatal("unknown command has a policy")
	}
}
