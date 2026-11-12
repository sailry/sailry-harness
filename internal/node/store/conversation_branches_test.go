package store

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func TestBranchAdmission(t *testing.T) {
	t.Parallel()
	// admitted stores the request row a pending turn refers to.
	admitted := func(ctx context.Context, db *sql.Conn, node protocol.NodeID, command protocol.Command) (protocol.Request, error) {
		request := protocol.NewRequest(node, command)
		body, err := encode(request)
		if err != nil {
			return request, err
		}
		_, err = db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", node[:], request.ID.String(), body)
		return request, err
	}
	upload := func(ctx context.Context, db *sql.Conn, node protocol.NodeID, session protocol.Session) (protocol.ConversationInput, error) {
		attachment := protocol.Attachment{ID: protocol.NewAttachmentID(), Spec: protocol.AttachmentSpec{Worktree: session.Worktree, Name: "input.txt", MediaType: "text/plain", Size: 5, Revision: fmt.Sprintf("%x", blake3.Sum256([]byte("input")))}}
		message := protocol.DefaultConversationInput()
		message.Text = "Replacement"
		message.Attachments = []protocol.AttachmentID{attachment.ID}
		return message, InsertAttachment(ctx, db, node, attachment)
	}
	t.Run("a replacement turn binds its attachments", func(t *testing.T) {
		t.Parallel()
		storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := context.Background()
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			user := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.User, canonical.Text(0, "Fixture input"))), now)
			user.Provenance.Author = "user"
			if _, _, err := AppendCanonical(ctx, db, session.ID, user); err != nil {
				return err
			}
			if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
				return err
			}
			message, err := upload(ctx, db, node, session)
			if err != nil {
				return err
			}
			revision, err := historyRevision(ctx, db, session.ID)
			if err != nil {
				return err
			}
			request, err := admitted(ctx, db, node, protocol.Command{Kind: "replace_turn", Data: &protocol.CommandReplaceTurnPayload{Session: session.ID, Turn: turn, ExpectedHead: turn, ExpectedRevision: session.Revision, ExpectedHistoryRevision: revision, Message: message}})
			if err != nil {
				return err
			}
			output, _, err := replaceConversationTurn(ctx, db, node, node, request)
			if err != nil {
				return err
			}
			replaced := output.Data.(protocol.OutputTurnReplacedPayload).Turn.ID
			if _, err = GetTurnAttachment(ctx, db, replaced, message.Attachments[0]); err != nil {
				return fmt.Errorf("replacement turn cannot read its attachment: %w", err)
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	})
	t.Run("a coding turn started from a plan binds its attachments", func(t *testing.T) {
		t.Parallel()
		storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModePlan)
		ctx := context.Background()
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			record := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Completed plan"))), now)
			record.Provenance.Author = "assistant"
			if _, _, err := AppendCanonical(ctx, db, session.ID, record); err != nil {
				return err
			}
			if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
				return err
			}
			_, question, _, err := PublishPlanReview(ctx, db, turn, now)
			if err != nil {
				return err
			}
			if question == nil {
				return errors.New("plan published no review")
			}
			message, err := upload(ctx, db, node, session)
			if err != nil {
				return err
			}
			request, err := admitted(ctx, db, node, protocol.Command{Kind: "resolve_question", Data: &protocol.CommandResolveQuestionPayload{Session: session.ID, Question: question.ID, Response: protocol.ConversationQuestionResponse{Kind: "start_coding", Data: &protocol.ConversationQuestionResponseStartCodingPayload{ExpectedRevision: session.Revision, Message: message}}}})
			if err != nil {
				return err
			}
			output, _, err := resolveQuestion(ctx, db, node, node, request)
			if err != nil {
				return err
			}
			coding := output.Data.(protocol.ConversationQuestionAcceptedPlan).Turn.ID
			if _, err = GetTurnAttachment(ctx, db, coding, message.Attachments[0]); err != nil {
				return fmt.Errorf("coding turn cannot read its attachment: %w", err)
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	})
	t.Run("a rewind takes the retained input of a turn it removes", func(t *testing.T) {
		t.Parallel()
		storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := context.Background()
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			// The turn fails before its input reaches the history.
			if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusFailed, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture"}, now); err != nil {
				return err
			}
			before, err := ReadQueue(ctx, db, session.ID)
			if err != nil {
				return err
			}
			if len(before.Items) != 1 || before.Items[0].Turn != turn {
				return fmt.Errorf("fixture queue = %+v", before.Items)
			}
			revision, err := historyRevision(ctx, db, session.ID)
			if err != nil {
				return err
			}
			if _, _, err = rewindConversation(ctx, db, protocol.CommandRewindConversationPayload{Session: session.ID, ExpectedHead: turn, ExpectedRevision: revision}, false); err != nil {
				return err
			}
			after, err := ReadQueue(ctx, db, session.ID)
			if err != nil {
				return err
			}
			if len(after.Items) != 0 || after.Revision <= before.Revision {
				return fmt.Errorf("rewound turn stayed in the queue: %+v", after)
			}
			if _, err = startQueued(ctx, db, turn); err == nil {
				return errors.New("a turn outside the conversation was started")
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	})
	t.Run("a fork does not carry its source's association", func(t *testing.T) {
		t.Parallel()
		storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := context.Background()
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			if _, err := db.ExecContext(ctx, "INSERT INTO session_associations(session,namespace,kind,identity) VALUES(?1,'fixture','task','one')", session.ID.String()); err != nil {
				return err
			}
			if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
				return err
			}
			source, err := ReadSession(ctx, db, session.ID)
			if err != nil {
				return err
			}
			if source.Association == nil {
				return errors.New("fixture session has no association")
			}
			fork, events, err := forkConversation(ctx, db, protocol.CommandForkConversationPayload{Session: session.ID, Through: turn, ExpectedRevision: source.Revision})
			if err != nil {
				return err
			}
			stored, err := ReadSession(ctx, db, fork.ID)
			if err != nil {
				return err
			}
			if fork.Association != nil || stored.Association != nil || len(events) != 1 || events[0].Data.(*protocol.Session).Association != nil {
				return errors.New("fork reported its source's association")
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	})
}
