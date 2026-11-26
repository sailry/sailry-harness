package store

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"reflect"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

const activityFixtureLimit = 20

func legacyConversationActivity(ctx context.Context, db *sql.Conn, session protocol.SessionID, limit uint16, changed ...protocol.TurnID) (ConversationActivity, error) {
	history, err := ReadConversation(ctx, db, session, nil, limit)
	if err != nil {
		return ConversationActivity{}, err
	}
	page := history.Page
	for _, turn := range changed {
		if slices.ContainsFunc(page.Children, func(child protocol.ConversationChild) bool { return child.Run.Turn == turn }) {
			continue
		}
		child, found, err := ReadConversationChild(ctx, db, session, turn)
		if err != nil {
			return ConversationActivity{}, err
		}
		if found {
			page.Children = append(page.Children, child)
		}
	}
	return ConversationActivity{Runs: page.Runs, Queue: page.Queue, Approvals: page.Approvals, Questions: page.Questions, Children: page.Children, Background: page.Background}, nil
}

func compareConversationActivity(ctx context.Context, db *sql.Conn, session protocol.SessionID, changed ...protocol.TurnID) (ConversationActivity, error) {
	want, err := legacyConversationActivity(ctx, db, session, activityFixtureLimit, changed...)
	if err != nil {
		return ConversationActivity{}, err
	}
	got, err := ReadConversationActivity(ctx, db, session, activityFixtureLimit, changed...)
	if err == nil && !reflect.DeepEqual(got, want) {
		err = fmt.Errorf("activity metadata differs from the history-based publication: got %+v, want %+v", got, want)
	}
	return got, err
}

func appendActivityFixtureText(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID, text string, now time.Time) (protocol.ConversationEntry, error) {
	record := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, text)), now)
	record.Provenance.Author = "assistant"
	entry, _, err := AppendCanonical(ctx, db, session, record)
	return entry, err
}

func activityFixtureChild(ctx context.Context, db *sql.Conn, node protocol.NodeID, parent protocol.TurnID, now time.Time) (AgentInvocation, error) {
	call, err := delegationFixtureCall(ctx, db, parent, "spawn_agent", map[string]any{"task": "Background child", "background": true}, now)
	if err != nil {
		return AgentInvocation{}, err
	}
	child, _, err := DelegateAgent(ctx, db, node, parent, call, nil, true, now)
	return child, err
}

func hideActivityFixtureEntry(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID, entry string, now time.Time) error {
	record := canonical.NewRecord(turn, "operation", canonical.Operation{Kind: "visibility", Data: &canonical.Visibility{Message: entry}}, now)
	runtime := canonical.DefaultRuntime()
	runtime.UserVisible = false
	record.Metadata.Runtime = &runtime
	_, _, err := AppendCanonical(ctx, db, session, record)
	return err
}

type activityFixtureInteraction struct {
	approval protocol.ApprovalID
	question protocol.QuestionID
}

func addActivityFixtureInteraction(ctx context.Context, db *sql.Conn, turn protocol.TurnID, now time.Time, settle bool) (activityFixtureInteraction, error) {
	var ids activityFixtureInteraction
	call, err := delegationFixtureCall(ctx, db, turn, "ask_user", map[string]any{
		"prompt": "Continue?", "input": map[string]any{"kind": "text", "max_bytes": 100},
	}, now)
	if err != nil {
		return ids, err
	}
	approval, _, err := BeginApproval(ctx, db, turn, call, nil, now)
	if err != nil {
		return ids, err
	}
	question, _, err := BeginQuestion(ctx, db, turn, call)
	if err != nil {
		return ids, err
	}
	ids = activityFixtureInteraction{approval.ID, question.ID}
	if settle {
		if _, _, err = ExpireApproval(ctx, db, approval.ID); err != nil {
			return ids, err
		}
		_, _, err = ExpireQuestion(ctx, db, question.ID)
	}
	return ids, err
}

func TestReadConversationActivityMatchesPublishedHead(t *testing.T) {
	t.Parallel()
	storage, node, session, older, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	var active, joined, queued protocol.TurnID
	var aged, visible AgentInvocation
	var pending, settled activityFixtureInteraction
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if err := queueFixtureInput(ctx, db, session.ID, older, now); err != nil {
			return err
		}
		if _, _, err := FinishTurn(ctx, db, older, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		var err error
		if active, err = delegationFixtureParent(ctx, db, node, session, now); err != nil {
			return err
		}
		if err = queueFixtureInput(ctx, db, session.ID, active, now); err != nil {
			return err
		}
		guide := func(text string) (protocol.TurnID, error) {
			output, err := queueFixtureCommand(ctx, db, storage, node, "guide_turn", &protocol.CommandGuideTurnPayload{
				Recipient: active, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: text},
			}, now)
			if err != nil {
				return protocol.TurnID{}, err
			}
			return output.Data.(protocol.QueuedTurn).ID, nil
		}
		if joined, err = guide("Joined guidance"); err != nil {
			return err
		}
		if taken, _, err := TakeGuidance(ctx, db, active); err != nil || len(taken) != 1 || taken[0].Turn.ID != joined {
			return fmt.Errorf("guidance did not join the active run: %v", err)
		}
		if queued, err = guide("Queued guidance"); err != nil {
			return err
		}
		if aged, err = activityFixtureChild(ctx, db, node, active, now); err != nil {
			return err
		}
		if pending, err = addActivityFixtureInteraction(ctx, db, active, now, false); err != nil {
			return err
		}
		if _, err = addActivityFixtureInteraction(ctx, db, active, now, true); err != nil {
			return err
		}
		// The older settled interaction is just outside the head's entry cap.
		for range historyEntryLimit - 2 {
			if _, err = appendActivityFixtureText(ctx, db, session.ID, active, "Recent output", now); err != nil {
				return err
			}
		}
		if settled, err = addActivityFixtureInteraction(ctx, db, active, now, true); err != nil {
			return err
		}
		visible, err = activityFixtureChild(ctx, db, node, active, now)
		return err
	}); err != nil {
		t.Fatal(err)
	}

	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		// The session list ranks all runs, first messages and delegations in a
		// batch. Each row must match an independent read of the same session.
		sessions, err := ReadSessions(ctx, db)
		if err != nil {
			return err
		}
		for _, id := range []protocol.SessionID{session.ID, aged.Session.ID, visible.Session.ID} {
			i := slices.IndexFunc(sessions, func(s protocol.Session) bool { return s.ID == id })
			if i < 0 {
				return fmt.Errorf("session list omitted %v", id)
			}
			want, err := ReadSession(ctx, db, id)
			if err != nil {
				return err
			}
			if !reflect.DeepEqual(sessions[i], want) {
				return fmt.Errorf("batched session %+v differs from single session %+v", sessions[i], want)
			}
		}

		got, err := compareConversationActivity(ctx, db, session.ID, visible.Run.Turn, aged.Run.Turn, aged.Run.Turn)
		if err != nil {
			return err
		}
		if len(got.Runs) != 3 || got.Runs[0].Turn != active || got.Runs[1].Turn != joined || got.Runs[2].Turn != queued {
			return fmt.Errorf("head lost an active group or kept the pruned group: %+v", got.Runs)
		}
		if got.Runs[0].Input != nil || got.Runs[1].Joined == nil || *got.Runs[1].Joined != active || got.Runs[1].Input == nil || got.Runs[1].Input.Message.Text != "Joined guidance" ||
			got.Runs[2].Input == nil || got.Runs[2].Input.Message.Text != "Queued guidance" || len(got.Queue.Items) != 1 || got.Queue.Items[0].Turn != queued {
			return errors.New("active and queued guidance did not preserve the published input and queue")
		}
		if len(got.Approvals) != 2 || got.Approvals[0].ID != pending.approval || got.Approvals[1].ID != settled.approval ||
			len(got.Questions) != 2 || got.Questions[0].ID != pending.question || got.Questions[1].ID != settled.question {
			return errors.New("head lost a pending or anchored settled interaction, or kept an aged settled interaction")
		}
		if len(got.Children) != 2 || got.Children[0].Run.Turn != visible.Run.Turn || got.Children[1].Run.Turn != aged.Run.Turn || len(got.Background) != 2 {
			return errors.New("head did not publish its visible child, the changed aged child and both background tasks")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		origin := aged.Session.Delegation
		if origin == nil || *origin == nil {
			return errors.New("aged child has no founding delegation")
		}
		return hideActivityFixtureEntry(ctx, db, session.ID, active, (*origin).Entry, now)
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		got, err := compareConversationActivity(ctx, db, session.ID, aged.Run.Turn)
		if err != nil {
			return err
		}
		if len(got.Children) != 1 || got.Children[0].Run.Turn != visible.Run.Turn {
			return errors.New("changed child with a hidden origin was published")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestReadConversationActivityFollowsForkAndRewindMembership(t *testing.T) {
	t.Parallel()
	storage, node, source, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	var fork protocol.Session
	var child AgentInvocation
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if err := queueFixtureInput(ctx, db, source.ID, turn, now); err != nil {
			return err
		}
		var err error
		if child, err = activityFixtureChild(ctx, db, node, turn, now); err != nil {
			return err
		}
		if _, err = addActivityFixtureInteraction(ctx, db, turn, now, true); err != nil {
			return err
		}
		if _, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		if fork, _, err = forkConversation(ctx, db, protocol.CommandForkConversationPayload{Session: source.ID, Through: turn, ExpectedRevision: source.Revision}); err != nil {
			return err
		}
		origin := child.Session.Delegation
		if origin == nil || *origin == nil {
			return errors.New("child has no founding delegation")
		}
		if err = hideActivityFixtureEntry(ctx, db, source.ID, turn, (*origin).Entry, now); err != nil {
			return err
		}
		revision, err := historyRevision(ctx, db, source.ID)
		if err != nil {
			return err
		}
		_, _, err = rewindConversation(ctx, db, protocol.CommandRewindConversationPayload{Session: source.ID, ExpectedHead: turn, ExpectedRevision: revision}, false)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		before, err := compareConversationActivity(ctx, db, fork.ID, child.Run.Turn)
		if err != nil {
			return err
		}
		if len(before.Runs) != 1 || before.Runs[0].Turn != turn || before.Runs[0].Session != fork.ID || before.Runs[0].Origin == nil || *before.Runs[0].Origin != source.ID ||
			len(before.Children) != 1 || before.Children[0].Run.Turn != child.Run.Turn ||
			len(before.Approvals) != 1 || before.Approvals[0].Session != fork.ID || len(before.Questions) != 1 || before.Questions[0].Session != fork.ID {
			return errors.New("fork lost its inherited child or interaction anchors, or applied a source-only visibility override")
		}
		after, err := compareConversationActivity(ctx, db, source.ID, child.Run.Turn)
		if err != nil {
			return err
		}
		if len(after.Runs)+len(after.Children)+len(after.Approvals)+len(after.Questions) != 0 {
			return errors.New("rewind retained metadata from a removed turn")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func appendActivityFixtureToolResult(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID, result []byte, now time.Time) (protocol.ConversationEntry, error) {
	response := canonical.ToolResponse{
		ID: protocol.NewRequestID().String(), Name: "fixture_tool", Status: canonical.ToolSuccess,
		Revision: fmt.Sprintf("%x", blake3.Sum256(result)), Result: canonical.Payload{Value: result},
	}
	record := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Block{Kind: "tool_response", Data: &response}), now)
	record.Provenance.Author = "tool"
	entry, _, err := AppendCanonical(ctx, db, session, record)
	return entry, err
}

func TestReadConversationActivityStoredByteBound(t *testing.T) {
	t.Parallel()
	storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	appendText := func(text string) protocol.ConversationEntry {
		t.Helper()
		var entry protocol.ConversationEntry
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			var err error
			entry, err = appendActivityFixtureText(ctx, db, session.ID, turn, text, now)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		return entry
	}
	appendText("A small entry beyond the cutoff")
	appendText(strings.Repeat("a", historyEntryByteLimit/2))
	for _, newestSize := range []int{historyEntryByteLimit / 2, historyEntryByteLimit + 1} {
		entry := appendText(strings.Repeat("b", newestSize))
		if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
			got, err := compareConversationActivity(ctx, db, session.ID)
			if err != nil {
				return err
			}
			anchors, err := conversationActivityAnchors(ctx, db, session.ID, got.Runs)
			if err != nil {
				return err
			}
			expected := conversationActivityAnchor{id: entry.ID, turn: entry.Turn, sequence: entry.Sequence}
			if !slices.Equal(anchors, []conversationActivityAnchor{expected}) {
				return fmt.Errorf("entry with %d text bytes: anchors = %+v, want %+v", newestSize, anchors, expected)
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	}
}

func TestReadConversationActivityDoesNotLoadToolPayload(t *testing.T) {
	t.Parallel()
	storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		_, err := appendActivityFixtureToolResult(ctx, db, session.ID, turn, []byte(`{"data":"AQID","mime_type":"image/png"}`), now)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	var want ConversationActivity
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		want, err = compareConversationActivity(ctx, db, session.ID)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	digest := fmt.Sprintf("%x", blake3.Sum256([]byte{1, 2, 3}))
	schemaExecute(t, storage.Node, "DELETE FROM agent_blobs WHERE digest=?1", digest)
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		_, err := ReadConversation(ctx, db, session.ID, nil, activityFixtureLimit)
		if failure, ok := errors.AsType[*protocol.Fault](err); !ok || failure.Code != protocol.ErrorCodeNotFound {
			return fmt.Errorf("fixture history did not report the removed tool payload: %v", err)
		}
		got, err := ReadConversationActivity(ctx, db, session.ID, activityFixtureLimit)
		if err == nil && !reflect.DeepEqual(got, want) {
			return errors.New("missing tool payload changed the published activity metadata")
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
}

func BenchmarkReadConversationActivity(b *testing.B) {
	storage, _, session, turn, now := agentHistoryFixture(b, protocol.WorkModeCode)
	ctx := b.Context()
	text := strings.Repeat("x", 512*1024)
	output := []byte(`{"output":"` + strings.Repeat("y", 2*1024*1024) + `"}`)
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := appendActivityFixtureText(ctx, db, session.ID, turn, text, now); err != nil {
			return err
		}
		_, err := appendActivityFixtureToolResult(ctx, db, session.ID, turn, output, now)
		return err
	}); err != nil {
		b.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		_, err := compareConversationActivity(ctx, db, session.ID)
		return err
	}); err != nil {
		b.Fatal(err)
	}
	for _, sample := range []struct {
		name string
		read func(context.Context, *sql.Conn, protocol.SessionID, uint16, ...protocol.TurnID) (ConversationActivity, error)
	}{
		{"history", legacyConversationActivity},
		{"metadata", ReadConversationActivity},
	} {
		b.Run(sample.name, func(b *testing.B) {
			ctx := b.Context()
			b.ReportAllocs()
			for b.Loop() {
				if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
					_, err := sample.read(ctx, db, session.ID, activityFixtureLimit)
					return err
				}); err != nil {
					b.Fatal(err)
				}
			}
		})
	}
}
