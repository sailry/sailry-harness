package engine

import (
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCurrentExecutionIdentityAfterForkAndCompaction(t *testing.T) {
	t.Parallel()
	model := &scriptedModel{scripts: []script{response(text("Original answer"), finish()), response(text("Fork answer"), finish())}}
	fixture := newFixture(t, model)
	fixture.start()
	source := fixture.session.ID
	first := fixture.submit("Start the original conversation")
	fixture.waitRun(first, protocol.ConversationStatusCompleted)
	records := fixture.state().Records
	now := fixture.r.clock.Now()
	summary := canonical.NewRecord(first, "compaction", &canonical.Compaction{Boundary: records[len(records)-1].ID,
		Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Historical session ID: "+source.String())), Start: now, End: now,
	}, now)
	if err := fixture.r.save(fixture.ctx, source, summary); err != nil {
		t.Fatal(err)
	}
	fork := fixture.command("fork_conversation", &protocol.CommandForkConversationPayload{Session: source, Through: first, ExpectedRevision: fixture.session.Revision}).Data.(protocol.Session)
	fixture.session = fork
	second := fixture.submit("Continue in the fork")
	fixture.waitRun(second, protocol.ConversationStatusCompleted)
	model.mu.Lock()
	before, after := promptText(model.calls[0]), promptText(model.calls[1])
	model.mu.Unlock()
	if !strings.Contains(before, "Session ID: "+source.String()) || !strings.Contains(before, "Turn ID: "+first.String()) {
		t.Fatal("the original request did not receive its captured execution identity")
	}
	if !strings.Contains(after, "Historical session ID: "+source.String()) {
		t.Fatal("the fork fixture did not retain its source's summary")
	}
	position := strings.LastIndex(after, "Current execution identity\n")
	if position < 0 {
		t.Fatal("the fork has no current execution identity")
	}
	current := after[position:]
	if !strings.Contains(current, "Session ID: "+fork.ID.String()) || !strings.Contains(current, "Turn ID: "+second.String()) || strings.Contains(current, "Session ID: "+source.String()) {
		t.Fatalf("the fork reused identity from copied history: %s", current)
	}
}
