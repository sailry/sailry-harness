package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPlanQuestionFeedbackKeepsPlanning(t *testing.T) {
	t.Parallel()
	for _, feedback := range []string{"", "  ", "Keep the existing API and revise step two"} {
		t.Run(fmt.Sprintf("feedback=%q", feedback), func(t *testing.T) {
			t.Parallel()
			storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModePlan)
			ctx := context.Background()
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				call := canonical.ToolRequest{ID: "review", Name: "ask_user", Arguments: json.RawMessage(`{"prompt":"Review this plan","input":{"kind":"plan"}}`)}
				record := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_request", Data: &call})), now)
				record.Provenance.Author = "assistant"
				if _, _, err := AppendCanonical(ctx, db, session.ID, record); err != nil {
					return err
				}
				question, _, err := BeginQuestion(ctx, db, turn, canonical.ToolIdentity{ID: &call.ID, Name: call.Name, Arguments: call.Arguments})
				if err != nil {
					return err
				}
				request := protocol.NewRequest(node, protocol.Command{Kind: "resolve_question", Data: &protocol.CommandResolveQuestionPayload{Session: session.ID, Question: question.ID, Response: protocol.ConversationQuestionResponse{Kind: "answer", Data: &protocol.ConversationQuestionAnswer{Kind: "text", Data: &feedback}}}})
				if _, _, err = resolveQuestion(ctx, db, node, node, request); err != nil {
					return err
				}
				reviewed, err := AgentTurnReviewed(ctx, db, turn)
				if err != nil {
					return err
				}
				wantsRevision := feedback == "Keep the existing API and revise step two"
				if reviewed == wantsRevision {
					return fmt.Errorf("reviewed=%v for feedback %q", reviewed, feedback)
				}
				saved, err := ReadQuestion(ctx, db, question.ID)
				if err != nil {
					return err
				}
				if saved.State.Kind != "answered" || *saved.State.Data.(*protocol.ConversationQuestionAnswer).Data.(*string) != feedback {
					return fmt.Errorf("feedback was not preserved: %+v", saved.State)
				}
				answer := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Revised plan"))), now)
				answer.Provenance.Author = "assistant"
				if _, _, err = AppendCanonical(ctx, db, session.ID, answer); err != nil {
					return err
				}
				if _, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
					return err
				}
				entry, next, _, err := PublishPlanReview(ctx, db, turn, now)
				if err != nil {
					return err
				}
				if !wantsRevision {
					if entry != nil || next != nil {
						return fmt.Errorf("empty Continue feedback reopened review")
					}
					return nil
				}
				if entry == nil || next == nil || next.State.Kind != "pending" || next.ID == question.ID {
					return fmt.Errorf("revised plan did not request a new review: %+v", next)
				}
				published, err := readBody[canonical.Record](ctx, db, "SELECT body FROM agent_events WHERE turn=?1 AND id=?2", turn.String(), next.Entry)
				if err != nil {
					return err
				}
				if published == nil || published.Data.(*canonical.Notice).Data.(*canonical.PlanReview).Source != answer.ID {
					return fmt.Errorf("review does not reference the revised answer")
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}
