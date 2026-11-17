package live

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestQueuedMessageEditRetainsExecutionHost(t *testing.T) {
	localSnapshot, remoteSnapshot := protocol.DefaultSnapshot(), protocol.DefaultSnapshot()
	localSnapshot.Node, remoteSnapshot.Node = protocol.NodeID{1}, protocol.NodeID{2}
	localSource := &conversationTestTransport{snapshot: localSnapshot}
	remoteSource := &conversationTestTransport{snapshot: remoteSnapshot}
	item := protocol.ConversationPending{Turn: protocol.NewTurnID(), Revision: 7}
	turn := protocol.DefaultQueuedTurn()
	turn.ID, turn.Session = item.Turn, protocol.NewSessionID()
	remoteSource.handle = func(request protocol.Request) (link.Response, bool) {
		switch request.Command.Kind {
		case "read_queued_turn":
			return link.Response{Output: protocol.Output{Kind: "queued_message", Data: protocol.ConversationQueuedMessage{
				Turn: turn, Revision: item.Revision, Message: protocol.ConversationInput{Text: "Original"},
			}}}, true
		case "edit_queued_turn":
			return link.Response{Output: protocol.Output{Kind: "queued_turn", Data: turn}}, true
		}
		return link.Response{}, false
	}
	localClient, remoteClient := client.New(localSource), client.New(remoteSource)
	defer localClient.Close()
	defer remoteClient.Close()
	if err := remoteClient.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	root := &Runtime{ctx: t.Context(), controller: localClient, App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})}
	remote := &Runtime{ctx: t.Context(), controller: remoteClient, App: root.App, parent: root, busy: map[string]bool{}, uncertain: map[string]protocol.Request{}}
	root.bindings = map[string]*Runtime{remoteSnapshot.Node.String(): remote}
	cv := &viewmodel.Conversation{ID: turn.Session.String(), Node: remoteSnapshot.Node.String(), Queue: []composer.Queued{{ID: item.Turn.String()}}}
	comp := composer.New("")
	comp.Host = localSnapshot.Node.String()
	owner := root.conversationRuntime(cv, comp)
	if owner != remote || comp.Host != cv.Node {
		t.Fatal("queue did not use its admitted session's host")
	}
	owner.enrichQueue(cv, protocol.ConversationQueue{Items: []protocol.ConversationPending{item}})
	// Navigation and another draft's host never rebind an existing queue.
	root.App.Host, root.App.Sidebar.Selected = localSnapshot.Node.String(), "another-draft"
	comp.Host = localSnapshot.Node.String()
	tester := ui.NewTester(func(c *ui.Context) {
		root.mu.Lock()
		queue := root.queue
		root.queue = nil
		root.mu.Unlock()
		for _, post := range queue {
			post(c)
		}
	}, 640, 480)
	settle := func() {
		for range 4 {
			root.workers.Wait()
			remote.workers.Wait()
			tester.Frame()
		}
	}
	defer settle()
	var loaded, saved bool
	cv.Queue[0].Load(func(text string, ok bool) { loaded = ok && text == "Original" })
	settle()
	cv.Queue[0].Save("Edited", func(ok bool) { saved = ok })
	settle()
	if !loaded || !saved {
		t.Fatalf("captured queue read or edit failed: loaded=%v saved=%v", loaded, saved)
	}
	remoteSource.mu.Lock()
	defer remoteSource.mu.Unlock()
	if len(remoteSource.requests) != 3 {
		t.Fatalf("queue used unexpected operations: %+v", remoteSource.requests)
	}
	for _, request := range remoteSource.requests {
		if request.Target != remoteSnapshot.Node {
			t.Fatal("queue operation changed its execution host")
		}
	}
	last := remoteSource.requests[2]
	input, ok := detail[protocol.CommandEditQueuedTurnPayload](last.Command.Data)
	if last.Command.Kind != "edit_queued_turn" || !ok || input.Turn != item.Turn || input.ExpectedRevision != item.Revision || input.Message.Text != "Edited" {
		t.Fatalf("queue edit lost its captured identity: %+v", last)
	}
	localSource.mu.Lock()
	defer localSource.mu.Unlock()
	if len(localSource.requests) != 0 {
		t.Fatal("queue operation followed the current desktop selection")
	}
}
