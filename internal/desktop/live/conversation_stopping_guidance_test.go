package live

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestStoppingQueueGuidance(t *testing.T) {
	session, recipient, queued := protocol.SessionID{1}, protocol.TurnID{2}, protocol.TurnID{3}
	snapshot := protocol.DefaultConversationSnapshot()
	snapshot.Node, snapshot.Sequence = protocol.NodeID{1}, 1
	snapshot.Page.Session, snapshot.Page.Revision = session, 1
	run := protocol.ConversationRun{Session: session, Turn: recipient, Kind: "task", Status: "running", Sequence: 1, Revision: 1}
	snapshot.Page.Runs = []protocol.ConversationRun{run}
	snapshot.Page.Queue = protocol.ConversationQueue{Revision: 1, Items: []protocol.ConversationPending{{Turn: queued, Kind: "task", Revision: 1, ConfigRevision: 1, Preview: "Queued input"}}}
	r, cv, transport, frame := watchCallbackTranscript(t, snapshot)
	run.Status = "stopping"
	frame("run", run)
	called := false
	tester := ui.NewTester(func(c *ui.Context) {
		if !called {
			called = true
			x := &conversationActions{r: r, c: c, cv: cv, comp: composer.New(cv.ID)}
			if _, ok := x.queued(queued.String()); !ok || x.readonly() {
				t.Fatal("fixture has no actionable queue item")
			}
			x.SendQueued(queued.String())
		}
	}, 320, 240)
	tester.Frame()
	r.workers.Wait()
	if !called {
		t.Fatal("fixture did not invoke SendQueued")
	}
	transport.mu.Lock()
	defer transport.mu.Unlock()
	for _, request := range transport.requests {
		if request.Command.Durable() {
			t.Fatalf("STOPPING_QUEUE_GUIDANCE: SendQueued issued %s after the recipient began stopping", request.Command.Kind)
		}
	}
}
