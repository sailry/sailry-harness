package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestHistoryBoundariesFollowExecutionAndGuidance(t *testing.T) {
	t.Parallel()
	for _, operation := range []string{"fork recipient", "fork reordered", "rewind recipient", "rewind reordered", "edit recipient", "edit reordered", "reject consumed guidance edit"} {
		t.Run(operation, func(t *testing.T) {
			t.Parallel()
			storage, node, session, recipient, now := agentHistoryFixture(t, protocol.WorkModeCode)
			ctx := context.Background()
			if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				appendMessage := func(turn protocol.TurnID, role canonical.Role, text string) error {
					record := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(role, canonical.Text(0, text))), now)
					record.Provenance.Author = string(role)
					_, _, err := AppendCanonical(ctx, db, session.ID, record)
					return err
				}
				if err := appendMessage(recipient, canonical.User, "Original request"); err != nil {
					return err
				}
				// Admission order is recipient, first, second, guidance. Execution
				// order is recipient+guidance, second, first, all in the same ms.
				queue := func(text string) (protocol.TurnID, error) {
					output, err := queueFixtureCommand(ctx, db, storage, node, "submit_turn", &protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: text}}, now)
					if err != nil {
						return protocol.TurnID{}, err
					}
					return output.Data.(protocol.QueuedTurn).ID, nil
				}
				first, err := queue("Admitted first, executed last")
				if err != nil {
					return err
				}
				second, err := queue("Admitted second, executed next")
				if err != nil {
					return err
				}
				queued, err := ReadQueue(ctx, db, session.ID)
				if err != nil {
					return err
				}
				if _, err = queueFixtureCommand(ctx, db, storage, node, "move_queued_turn", &protocol.CommandMoveQueuedTurnPayload{Session: session.ID, ExpectedRevision: queued.Revision, Turn: second, Before: &first}, now); err != nil {
					return err
				}
				output, err := queueFixtureCommand(ctx, db, storage, node, "guide_turn", &protocol.CommandGuideTurnPayload{Recipient: recipient, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Guidance consumed by the original request"}}, now)
				if err != nil {
					return err
				}
				guide := output.Data.(protocol.QueuedTurn).ID
				guidance, _, err := TakeGuidance(ctx, db, recipient)
				if err != nil || len(guidance) != 1 || guidance[0].Turn.ID != guide {
					return fmt.Errorf("guidance fixture was not consumed: %v", err)
				}
				if err = appendMessage(guide, canonical.User, guidance[0].Message.Text); err != nil {
					return err
				}
				if err = appendMessage(recipient, canonical.Assistant, "Answer incorporating the guidance"); err != nil {
					return err
				}
				if _, _, err = FinishTurn(ctx, db, recipient, protocol.ConversationStatusCompleted, nil, now); err != nil {
					return err
				}
				info, err := InstallPlugin(ctx, db, pluginTestInfo("conversation-fixture", 'a'), 0, true)
				if err != nil {
					return err
				}
				reference := plugins.PackageReference(info)
				checkpoint, err := WritePluginConversationValue(ctx, db, reference, session.ID, &guide, "checkpoint", json.RawMessage(`"guidance"`), 0)
				if err != nil {
					return err
				}
				for _, turn := range []protocol.TurnID{second, first} {
					invocation, _, err := ClaimNext(ctx, db, now)
					if err != nil || invocation == nil || invocation.Run.Turn != turn {
						return fmt.Errorf("reordered task was not claimed: %v", err)
					}
					if err = appendMessage(turn, canonical.User, invocation.Message.Text); err != nil {
						return err
					}
					if err = appendMessage(turn, canonical.Assistant, "Answer to reordered task"); err != nil {
						return err
					}
					value := json.RawMessage(`"second"`)
					if turn == first {
						value = json.RawMessage(`"first"`)
					}
					checkpoint, err = WritePluginConversationValue(ctx, db, reference, session.ID, &turn, "checkpoint", value, checkpoint.Revision)
					if err != nil {
						return err
					}
					if _, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
						return err
					}
				}
				if _, err = WritePluginConversationValue(ctx, db, reference, session.ID, nil, "latest", json.RawMessage(`"after the last execution"`), 0); err != nil {
					return err
				}
				head, err := conversationHead(ctx, db, session.ID)
				if err != nil || head == nil || *head != guide {
					return fmt.Errorf("fixture lost the public admission head: %v", err)
				}
				revision, err := historyRevision(ctx, db, session.ID)
				if err != nil {
					return err
				}
				if _, _, err = rewindConversation(ctx, db, protocol.CommandRewindConversationPayload{Session: session.ID, Through: &recipient, ExpectedHead: recipient, ExpectedRevision: revision}, false); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
					return fmt.Errorf("stale public history head was accepted: %v", err)
				}
				var beforeRecords, beforeSessions, beforeTurns int
				if err = db.QueryRowContext(ctx, "SELECT (SELECT count(*) FROM agent_events),(SELECT count(*) FROM sessions),(SELECT count(*) FROM turns)").Scan(&beforeRecords, &beforeSessions, &beforeTurns); err != nil {
					return err
				}
				beforeState, err := LoadAgentState(ctx, db, session.ID)
				if err != nil {
					return err
				}
				beforeHistory, err := encode(beforeState)
				if err != nil {
					return err
				}
				rejected := operation == "reject consumed guidance edit"
				target := recipient
				want := []protocol.TurnID{recipient, guide}
				wantCheckpoint := `"guidance"`
				if operation == "fork reordered" || operation == "rewind reordered" || operation == "edit reordered" {
					target = second
					want = append(want, second)
					wantCheckpoint = `"second"`
				}
				view := session.ID
				var backup *protocol.Session
				switch operation {
				case "reject consumed guidance edit":
					request := protocol.NewRequest(node, protocol.Command{Kind: "replace_turn", Data: &protocol.CommandReplaceTurnPayload{Session: session.ID, Turn: guide, ExpectedHead: *head, ExpectedRevision: session.Revision, ExpectedHistoryRevision: revision, Message: protocol.ConversationInput{Text: "Edited guidance"}}})
					body, err := encode(request)
					if err != nil {
						return err
					}
					if _, err = db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", node[:], request.ID.String(), body); err != nil {
						return err
					}
					// Call without an enclosing command rollback: rejection itself
					// must precede any history, checkpoint or admission mutation.
					_, events, err := replaceConversationTurn(ctx, db, node, node, request)
					if !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) || len(events) != 0 {
						return fmt.Errorf("consumed guidance edit was not rejected: %v", err)
					}
					want, wantCheckpoint = []protocol.TurnID{recipient, guide, second, first}, `"first"`
				case "fork recipient", "fork reordered":
					fork, _, err := forkConversation(ctx, db, protocol.CommandForkConversationPayload{Session: session.ID, Through: target, ExpectedRevision: session.Revision})
					if err != nil {
						return err
					}
					view = fork.ID
				case "rewind recipient", "rewind reordered":
					rewound, _, err := rewindConversation(ctx, db, protocol.CommandRewindConversationPayload{Session: session.ID, Through: &target, ExpectedHead: *head, ExpectedRevision: revision}, true)
					if err != nil {
						return err
					}
					backup = rewound.Backup
					if backup == nil {
						return errors.New("explicit rewind lost its backup")
					}
				case "edit recipient", "edit reordered":
					output, err := queueFixtureCommand(ctx, db, storage, node, "replace_turn", &protocol.CommandReplaceTurnPayload{Session: session.ID, Turn: target, ExpectedHead: *head, ExpectedRevision: session.Revision, ExpectedHistoryRevision: revision, Message: protocol.ConversationInput{Text: "Edited request"}}, now)
					if err != nil {
						return err
					}
					replaced := output.Data.(protocol.OutputTurnReplacedPayload)
					if replaced.History == nil || replaced.History.Backup != nil || replaced.Turn.Session != session.ID {
						return errors.New("edit changed its session or created a backup")
					}
					want, wantCheckpoint = []protocol.TurnID{replaced.Turn.ID}, ""
					if target == second {
						want = append(want, recipient, guide)
						wantCheckpoint = `"guidance"`
					}
				}
				assertMembers := func(id protocol.SessionID, want []protocol.TurnID) error {
					members, err := readTexts(ctx, db, "SELECT turn FROM conversation_turns WHERE session=?1", id.String())
					if err != nil {
						return err
					}
					if len(members) != len(want) {
						return fmt.Errorf("history kept %d turns, want %d", len(members), len(want))
					}
					for _, turn := range want {
						if !slices.Contains(members, turn.String()) {
							return errors.New("history lost an execution group member")
						}
					}
					state, err := LoadAgentState(ctx, db, id)
					if err != nil {
						return err
					}
					expectedRecords := 0
					for _, turn := range want {
						if turn == guide {
							expectedRecords++
						} else if turn == recipient || turn == first || turn == second {
							expectedRecords += 2
						}
					}
					if len(state.Records) != expectedRecords {
						return fmt.Errorf("model history kept %d records, want %d", len(state.Records), expectedRecords)
					}
					for _, record := range state.Records {
						if !slices.Contains(want, record.Turn) {
							return errors.New("model history retained an excluded turn")
						}
					}
					return nil
				}
				if err = assertMembers(view, want); err != nil {
					return err
				}
				if backup != nil {
					if err = assertMembers(backup.ID, []protocol.TurnID{recipient, guide, second, first}); err != nil {
						return err
					}
					latest, err := ReadPluginConversationValue(ctx, db, reference, backup.ID, "latest")
					if err != nil || !latest.Present {
						return fmt.Errorf("rewind backup lost the current plugin value: %v", err)
					}
				}
				latest, err := ReadPluginConversationValue(ctx, db, reference, view, "latest")
				if err != nil || latest.Present != rejected || rejected && string(latest.Value) != `"after the last execution"` {
					return fmt.Errorf("history inherited plugin data from a later execution: %v", err)
				}
				value, err := ReadPluginConversationValue(ctx, db, reference, view, "checkpoint")
				if err != nil || value.Present != (wantCheckpoint != "") || value.Present && string(value.Value) != wantCheckpoint {
					return fmt.Errorf("plugin checkpoint does not match history: %+v, %v", value, err)
				}
				if rejected && (value.Revision != checkpoint.Revision || value.Restored != checkpoint.Restored) {
					return errors.New("rejected guidance edit changed the plugin checkpoint")
				}
				var afterRecords, afterSessions, afterTurns int
				if err = db.QueryRowContext(ctx, "SELECT (SELECT count(*) FROM agent_events),(SELECT count(*) FROM sessions),(SELECT count(*) FROM turns)").Scan(&afterRecords, &afterSessions, &afterTurns); err != nil {
					return err
				}
				if afterRecords != beforeRecords {
					return errors.New("branch operation modified immutable canonical records")
				}
				if (operation == "edit recipient" || operation == "edit reordered" || rejected) && afterSessions != beforeSessions {
					return errors.New("edit created an unsolicited session")
				}
				if rejected {
					afterState, err := LoadAgentState(ctx, db, session.ID)
					if err != nil {
						return err
					}
					afterHistory, err := encode(afterState)
					if err != nil {
						return err
					}
					afterRevision, err := historyRevision(ctx, db, session.ID)
					if err != nil || afterRevision != revision || afterTurns != beforeTurns || string(afterHistory) != string(beforeHistory) {
						return fmt.Errorf("rejected guidance edit changed original history: %v", err)
					}
				}
				return nil
			}); err != nil {
				t.Fatal(err)
			}
		})
	}
}
