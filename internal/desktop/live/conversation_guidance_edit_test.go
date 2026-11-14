package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestHistoryEditControlsExcludeConsumedGuidance(t *testing.T) {
	node, session, worktree := protocol.NodeID{1}, protocol.SessionID{2}, protocol.WorktreeID{3}
	recipient, joined, independent := protocol.TurnID{1}, protocol.TurnID{2}, protocol.TurnID{3}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = node
	stored := protocol.DefaultSession()
	stored.ID, stored.Revision, stored.Worktree = session, 1, worktree
	snapshot.Sessions = []protocol.Session{stored}
	history := protocol.DefaultConversationSnapshot()
	history.Node, history.Page.Session, history.Page.Revision = node, session, 1
	for index, turn := range []protocol.TurnID{recipient, joined, independent} {
		run := protocol.DefaultConversationRun()
		run.Session, run.Worktree, run.Turn = session, worktree, turn
		run.Sequence, run.Revision = uint64(index+1), 1
		run.Kind, run.Status = protocol.ConversationRunKindTask, protocol.ConversationStatusCompleted
		if turn != recipient {
			run.Guidance = &recipient
		}
		if turn == joined {
			run.Joined = &recipient
		}
		history.Page.Runs = append(history.Page.Runs, run)
		history.Page.Entries = append(history.Page.Entries, protocol.ConversationEntry{ID: turn.String(), Sequence: uint64(index + 1), Turn: turn, Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Sent input"}}})
	}
	controller := client.New(&conversationTestTransport{snapshot: snapshot, conversation: history})
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	changed := make(chan struct{}, 1)
	release, err := controller.WatchConversation(t.Context(), session, func() {
		select {
		case changed <- struct{}{}:
		default:
		}
	})
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	for {
		state, ok := controller.Conversation(session)
		if state.Err != nil {
			t.Fatal(state.Err)
		}
		if ok && state.Connected {
			break
		}
		select {
		case <-changed:
		case <-t.Context().Done():
			t.Fatal(t.Context().Err())
		}
	}
	runtime := &Runtime{controller: controller, conversationEdits: map[string]conversationEdit{}}
	view := &viewmodel.Conversation{ID: session.String(), Node: node.String()}
	actions := &conversationActions{r: runtime, cv: view, comp: &composer.State{}}
	controls := actions.pageControls()
	if controls(recipient).NoEdit || controls(independent).NoEdit || !controls(joined).NoEdit {
		t.Fatal("history edit controls rejected independent input or allowed consumed guidance")
	}
	actions.Edit(joined.String())
	if len(runtime.conversationEdits) != 0 {
		t.Fatal("consumed guidance opened an editor")
	}
	actions.Edit(independent.String())
	if edit, ok := runtime.conversationEdits[view.ID]; !ok || edit.Turn != independent {
		t.Fatal("independently executed guidance could not be edited")
	}
}
