package store

import (
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginContinuationAdmission(t *testing.T) {
	t.Parallel()
	for _, scenario := range []string{"direct", "transaction", "changed private revision", "disabled owner", "undeclared action", "restart"} {
		t.Run(scenario, func(t *testing.T) {
			t.Parallel()
			storage, node, session, previous, now := agentHistoryFixture(t, protocol.WorkModeCode)
			ctx := t.Context()
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				record := canonical.NewRecord(previous, "message", ptr(canonical.NewMessage(canonical.User, canonical.Text(0, "Recorded request"))), now)
				record.Provenance.Author = "user"
				if _, _, err := AppendCanonical(ctx, db, session.ID, record); err != nil {
					return err
				}
				if _, _, err := FinishTurn(ctx, db, previous, protocol.ConversationStatusCompleted, nil, now); err != nil {
					return err
				}
				info := pluginTestInfo("continuation-fixture", 'c')
				extension := protocol.DefaultPluginExtension()
				extension.ApiVersion = "v1"
				extension.Actions = []protocol.PluginAction{protocol.PluginActionControlConversation}
				extension.ContinuationMarker = &protocol.PluginDesktopNavigation{Label: "Continue goal"}
				if scenario == "undeclared action" {
					extension.Actions = nil
				}
				info.Extension = &extension
				info, err := InstallPlugin(ctx, db, info, 0, true)
				if err != nil {
					return err
				}
				scope := protocol.DefaultPluginContext()
				scope.Package, scope.Session, scope.Worktree = plugins.PackageReference(info), &session.ID, &session.Worktree
				value, err := WritePluginValue(ctx, db, scope.Package, "run/state", json.RawMessage(`{"active":true}`), 0, nil)
				if err != nil {
					return err
				}
				input := protocol.CommandContinueTurnPayload{Session: session.ID, After: &previous, Message: protocol.ConversationInput{Text: "Continue the requested work"}, Key: "run/state", ExpectedRevision: value.Revision, Scope: protocol.PluginStorageScopeNode}
				request := protocol.NewRequest(node, protocol.Command{Kind: "continue_turn", Data: &input})
				request.Plugin = &scope
				if scenario == "transaction" {
					request.Command = protocol.Command{Kind: "plugin_transaction", Data: &protocol.CommandPluginTransactionPayload{Operations: []protocol.PluginTransactionOperation{{
						Kind: "continue", Data: protocol.PluginTransactionOperationContinuePayload{Session: input.Session, After: input.After, Message: input.Message, Key: input.Key, ExpectedRevision: input.ExpectedRevision, Scope: input.Scope},
					}}}}
				}
				body, err := encode(request)
				if err != nil {
					return err
				}
				if _, err = db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", node[:], request.ID.String(), body); err != nil {
					return err
				}
				var output protocol.Output
				if scenario == "transaction" {
					output, _, err = ExecutePluginStorageCommand(ctx, db, node, storage.Profile, node, request)
				} else {
					output, _, err = ExecuteBackgroundCommand(ctx, db, node, node, request, now)
				}
				if scenario == "undeclared action" {
					if !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
						return fmt.Errorf("undeclared continuation reached admission: %v", err)
					}
					return nil
				}
				if err != nil {
					return err
				}
				if scenario == "transaction" {
					results, ok := output.Data.([]protocol.Output)
					if !ok || len(results) != 1 {
						return errors.New("transaction lost its continuation output")
					}
					output = results[0]
				}
				queued, ok := output.Data.(protocol.QueuedTurn)
				if output.Kind != "queued_turn" || !ok {
					return errors.New("continuation bypassed the ordinary queued-turn output")
				}
				run, err := ReadAgentRun(ctx, db, queued.ID)
				if err != nil || run.Continuation == nil || run.Continuation.Package != scope.Package || run.Continuation.Marker == nil || run.Continuation.Marker.Label != "Continue goal" {
					return fmt.Errorf("continuation lost captured provenance: %v", err)
				}
				switch scenario {
				case "changed private revision":
					_, err = WritePluginValue(ctx, db, scope.Package, input.Key, json.RawMessage(`{"active":false}`), value.Revision, nil)
				case "disabled owner":
					_, err = SetPluginEnabled(ctx, db, info.Summary.Name, info.Summary.Revision, false)
				case "restart":
					_, err = RecoverRuns(ctx, db, now)
				}
				if err != nil {
					return err
				}
				invocation, _, err := ClaimNext(ctx, db, now)
				if err != nil {
					return err
				}
				if scenario == "direct" || scenario == "transaction" {
					if invocation == nil || invocation.Run.Turn != queued.ID || invocation.Message.Text != input.Message.Text || invocation.Run.Continuation == nil {
						return errors.New("continuation was not claimed as ordinary frozen input")
					}
				} else if invocation != nil {
					return errors.New("invalidated automatic input was executed")
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}
