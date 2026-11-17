package engine

import (
	"context"
	"sync"
	"sync/atomic"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestLoginRecoveryContinuesUnsentStepWithoutRepeatingCompletedTool(t *testing.T) {
	m := &scriptedModel{scripts: []script{
		response(call("effect", "probe", `{}`), finish()),
		response(text("Continued after sign-in"), finish()),
	}}
	var effects atomic.Int32
	tool := objectTool("probe", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
		effects.Add(1)
		return ToolResult{Value: map[string]bool{"saved": true}}
	})
	f := newFixture(t, m, tool)
	waiting, resumed := make(chan struct{}, 1), make(chan struct{})
	old := protocol.CredentialRef{Node: f.node, ID: protocol.NewCredentialID()}
	next := protocol.CredentialRef{Node: f.node, ID: protocol.NewCredentialID()}
	f.r.authorizeProvider = func(ctx context.Context, invocation *store.AgentInvocation) error {
		access, reference := "old-access", old
		if effects.Load() != 0 {
			select {
			case waiting <- struct{}{}:
			default:
			}
			select {
			case <-resumed:
			case <-ctx.Done():
				return ctx.Err()
			}
			access, reference = "new-access", next
		}
		secret := protocol.NewSecret(`{"kind":"copilot","access":"` + access + `","endpoint":"https://api.githubcopilot.com","renewal_pending":false}`)
		invocation.Provider.Authentication, invocation.Provider.Endpoint = protocol.AuthenticationCopilot, "https://api.githubcopilot.com"
		invocation.Secret, invocation.EffectiveCredential = &secret, &reference
		return nil
	}
	var mu sync.Mutex
	var effective []protocol.CredentialRef
	f.r.model = func(_ context.Context, binding model.Binding) (fantasy.LanguageModel, error) {
		mu.Lock()
		if binding.Config.Credential != nil {
			effective = append(effective, *binding.Config.Credential)
		}
		mu.Unlock()
		return m, nil
	}
	f.start()
	turn := f.submit("Perform the operation once")
	select {
	case <-waiting:
	case <-f.ctx.Done():
		t.Fatal(f.ctx.Err())
	}
	if effects.Load() != 1 || m.count() != 1 {
		t.Fatalf("waiting repeated an effect or sent the next model request: effects=%d calls=%d", effects.Load(), m.count())
	}
	close(resumed)
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	if effects.Load() != 1 || m.count() != 2 {
		t.Fatalf("sign-in replayed completed work: effects=%d calls=%d", effects.Load(), m.count())
	}
	mu.Lock()
	defer mu.Unlock()
	if len(effective) == 0 || effective[len(effective)-1] != next {
		t.Fatalf("the next model transport did not capture the accepted replacement: %v", effective)
	}
}
