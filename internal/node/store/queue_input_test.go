package store

import (
	"bytes"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestQueuedInputReadAndEdit(t *testing.T) {
	t.Parallel()
	for _, source := range []string{"request", "plugin transaction"} {
		for _, retained := range []bool{false, true} {
			t.Run(fmt.Sprintf("%s/retained=%t", source, retained), func(t *testing.T) {
				t.Parallel()
				storage, node, session, previous, now := agentHistoryFixture(t, protocol.WorkModeCode)
				ctx := t.Context()
				err := storage.Node.Write(ctx, func(db *sql.Conn) error {
					if err := queueFixtureInput(ctx, db, session.ID, previous, now); err != nil {
						return err
					}
					if _, _, err := FinishTurn(ctx, db, previous, protocol.ConversationStatusCompleted, nil, now); err != nil {
						return err
					}
					attachments := []protocol.AttachmentID{protocol.NewAttachmentID(), protocol.NewAttachmentID()}
					for _, id := range attachments {
						attachment := protocol.DefaultAttachment()
						attachment.ID, attachment.Spec.Worktree = id, session.Worktree
						body, err := encode(attachment)
						if err != nil {
							return err
						}
						if _, err = db.ExecContext(ctx, "INSERT INTO attachments(id,worktree,caller,body) VALUES(?1,?2,?3,?4)", id.String(), session.Worktree.String(), node[:], body); err != nil {
							return err
						}
					}
					original := protocol.ConversationInput{Text: strings.Repeat("Original input ", 24), Attachments: attachments[:1], References: []protocol.ConversationReference{{Target: protocol.ConversationReferenceTarget{Kind: "host"}, Label: "Host"}}}
					edited := protocol.ConversationInput{Text: strings.Repeat("Edited input ", 24), Attachments: attachments[1:], References: original.References}
					request := protocol.NewRequest(node, protocol.Command{Kind: "queue_turn", Data: &protocol.CommandQueueTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: original}})
					if source == "plugin transaction" {
						info := pluginTestInfo("queue-fixture", 'e')
						extension := protocol.DefaultPluginExtension()
						extension.ApiVersion = "v1"
						extension.Actions = []protocol.PluginAction{protocol.PluginActionControlConversation}
						info.Extension = &extension
						info, err := InstallPlugin(ctx, db, info, 0, true)
						if err != nil {
							return err
						}
						scope := protocol.DefaultPluginContext()
						scope.Package, scope.Session, scope.Worktree = plugins.PackageReference(info), &session.ID, &session.Worktree
						request.Plugin = &scope
						request.Command = protocol.Command{Kind: "plugin_transaction", Data: &protocol.CommandPluginTransactionPayload{Operations: []protocol.PluginTransactionOperation{{Kind: "submit", Data: protocol.PluginTransactionOperationSubmitPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: original}}}}}
					}
					body, err := encode(request)
					if err != nil {
						return err
					}
					if replay, err := admission.Admit(ctx, db, node, request, body); err != nil || replay != nil {
						return fmt.Errorf("initial admission: replay=%v, error=%v", replay, err)
					}
					var admitted protocol.Output
					if source == "plugin transaction" {
						admitted, _, err = ExecutePluginStorageCommand(ctx, db, node, storage.Profile, node, request)
					} else {
						admitted, _, err = ExecuteAgentCommand(ctx, db, node, storage.Profile, node, request, now)
					}
					if err != nil {
						return err
					}
					if err = admission.Complete(ctx, db, node, request.ID, link.Response{Output: admitted}); err != nil {
						return err
					}
					queue, err := ReadQueue(ctx, db, session.ID)
					if err != nil || len(queue.Items) != 1 {
						return fmt.Errorf("initial queue = %+v: %v", queue.Items, err)
					}
					turn := queue.Items[0].Turn
					capture, err := readTurnCapture(ctx, db, turn)
					if err != nil {
						return err
					}
					checkInput := func(want protocol.ConversationInput, revision uint64) error {
						output, err := ReadAgentCommand(ctx, db, node, protocol.Command{Kind: "read_queued_turn", Data: &protocol.CommandReadQueuedTurnPayload{Turn: turn}})
						if err != nil {
							return err
						}
						full := output.Data.(protocol.ConversationQueuedMessage)
						if full.Revision != revision || !reflect.DeepEqual(full.Message, want) || !reflect.DeepEqual(full.Turn, capture) {
							return fmt.Errorf("queued full input or capture changed: %+v", full)
						}
						_, item, err := queueFixtureItem(ctx, db, session.ID, turn)
						if err != nil || item == nil || item.Revision != revision || !item.Truncated || item.Preview != string([]rune(want.Text)[:256]) || len(item.Attachments) != 1 || item.Attachments[0].ID != want.Attachments[0] {
							return fmt.Errorf("queued preview or attachments changed: %+v: %v", item, err)
						}
						return nil
					}
					if err = checkInput(original, 1); err != nil {
						return err
					}
					command := func(kind string, value any) (protocol.Output, error) {
						return queueFixtureCommand(ctx, db, storage, node, kind, value, now)
					}
					if retained {
						if _, err = command("start_queued_turn", &protocol.CommandStartQueuedTurnPayload{Turn: turn}); err != nil {
							return err
						}
						invocation, _, err := ClaimNext(ctx, db, now)
						if err != nil || invocation == nil || invocation.Run.Turn != turn || !reflect.DeepEqual(invocation.Message, original) {
							return fmt.Errorf("original input was not claimed: %+v: %v", invocation, err)
						}
						if _, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusFailed, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Before recording input"}, now); err != nil {
							return err
						}
					}
					if _, err = command("edit_queued_turn", &protocol.CommandEditQueuedTurnPayload{Turn: turn, ExpectedRevision: 1, Message: edited}); err != nil {
						return fmt.Errorf("unconsumed input could not be edited: %w", err)
					}
					if _, err = command("edit_queued_turn", &protocol.CommandEditQueuedTurnPayload{Turn: turn, ExpectedRevision: 1, Message: original}); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
						return fmt.Errorf("stale edit = %v", err)
					}
					if err = checkInput(edited, 2); err != nil {
						return err
					}
					var ledger []byte
					var captured bool
					if err = db.QueryRowContext(ctx, "SELECT body FROM requests WHERE id=?1 AND caller=?2", request.ID.String(), node[:]).Scan(&ledger); err != nil {
						return err
					}
					if !bytes.Equal(ledger, admission.Body(request.Command, body)) || source == "plugin transaction" && !bytes.HasPrefix(ledger, []byte("credential-v1:")) {
						return errors.New("editing rewrote the original request ledger")
					}
					if err = db.QueryRowContext(ctx, "SELECT input IS NOT NULL FROM agent_runs WHERE turn=?1", turn.String()).Scan(&captured); err != nil {
						return err
					}
					if captured != (retained || source == "plugin transaction") {
						return errors.New("editing changed when ordinary input is captured")
					}
					replay, err := admission.Admit(ctx, db, node, request, body)
					if err != nil || replay == nil || replay.Fault != nil || replay.Output.Kind != admitted.Kind {
						return fmt.Errorf("original admission no longer replays: %+v: %v", replay, err)
					}
					changedBody := bytes.Replace(body, []byte(original.Text), []byte("Different admission content"), 1)
					var changed protocol.Request
					if err = json.Unmarshal(changedBody, &changed); err != nil {
						return err
					}
					if _, err = admission.Admit(ctx, db, node, changed, changedBody); !providerFixtureFault(err, protocol.ErrorCodeConflict) {
						return fmt.Errorf("request identity accepted changed content: %v", err)
					}
					if _, err = command("send_queued_turn", &protocol.CommandSendQueuedTurnPayload{Turn: turn, ExpectedRevision: 2}); err != nil {
						return err
					}
					invocation, _, err := ClaimNext(ctx, db, now)
					if err != nil || invocation == nil || invocation.Run.Turn != turn || !reflect.DeepEqual(invocation.Message, edited) || !reflect.DeepEqual(invocation.Turn, capture) {
						return fmt.Errorf("execution did not use the edited input and original capture: %+v: %v", invocation, err)
					}
					// Claim retains agent_pending until the user record is saved;
					// that row must not make a running input editable.
					if _, err = command("edit_queued_turn", &protocol.CommandEditQueuedTurnPayload{Turn: turn, ExpectedRevision: 2, Message: original}); !providerFixtureFault(err, protocol.ErrorCodeConflict) {
						return fmt.Errorf("running input edit = %v", err)
					}
					current, err := readQueuedMessage(ctx, db, turn)
					if err != nil || !reflect.DeepEqual(current.Message, edited) {
						return fmt.Errorf("refused running edit changed its input: %+v: %v", current, err)
					}
					if err = queueFixtureInput(ctx, db, session.ID, turn, now); err != nil {
						return err
					}
					if _, err = readQueuedMessage(ctx, db, turn); !providerFixtureFault(err, protocol.ErrorCodeConflict) {
						return fmt.Errorf("consumed input remains queued: %v", err)
					}
					return nil
				})
				if err != nil {
					t.Fatal(err)
				}
			})
		}
	}
}
