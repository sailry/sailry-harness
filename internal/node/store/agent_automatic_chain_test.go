package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestAutomaticChain covers the one budget notice turns and package
// continuations share, the limit a continuation declares for the chain it
// owns, and what a refused continuation tells its package.
func TestAutomaticChain(t *testing.T) {
	t.Parallel()
	for _, name := range []string{
		"default refuses the ninth automatic turn",
		"declared limit owns the chain",
		"user input resets the chain",
		"notice turns share the declared limit",
		"held result binds to the next continuation",
		"refusals name their reason",
		"plan mode refuses",
		"continuation names a declared marker",
		"automatic turn takes back the unread mark",
	} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			mode := protocol.WorkModeCode
			if name == "plan mode refuses" {
				mode = protocol.WorkModePlan
			}
			storage, node, session, first, now := agentHistoryFixture(t, mode)
			ctx := context.Background()
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				if err := queueFixtureInput(ctx, db, session.ID, first, now); err != nil {
					return err
				}
				if _, _, err := FinishTurn(ctx, db, first, protocol.ConversationStatusCompleted, nil, now); err != nil {
					return err
				}
				info := pluginTestInfo("chain-fixture", 'd')
				extension := protocol.DefaultPluginExtension()
				extension.ApiVersion = "v1"
				extension.Actions = []protocol.PluginAction{protocol.PluginActionControlConversation}
				extension.ContinuationMarker = &protocol.PluginDesktopNavigation{Label: "Continue work"}
				extension.ContinuationMarkers = map[string]protocol.PluginDesktopNavigation{"verify": {Label: "Verify work"}}
				info.Extension = &extension
				info, err := InstallPlugin(ctx, db, info, 0, true)
				if err != nil {
					return err
				}
				scope := protocol.DefaultPluginContext()
				scope.Package, scope.Session, scope.Worktree = plugins.PackageReference(info), &session.ID, &session.Worktree
				value, err := WritePluginValue(ctx, db, scope.Package, "loop", json.RawMessage(`{"active":true}`), 0, nil)
				if err != nil {
					return err
				}
				// admit requests one continuation as the package would.
				admit := func(limit *uint32, marker *string) (protocol.TurnID, error) {
					input := protocol.CommandContinueTurnPayload{Session: session.ID, Message: protocol.ConversationInput{Text: "Continue"}, Key: "loop", ExpectedRevision: value.Revision, Scope: protocol.PluginStorageScopeNode, Limit: limit, Marker: marker}
					request := protocol.NewRequest(node, protocol.Command{Kind: "continue_turn", Data: &input})
					request.Plugin = &scope
					body, err := encode(request)
					if err != nil {
						return protocol.TurnID{}, err
					}
					if _, err = db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", node[:], request.ID.String(), body); err != nil {
						return protocol.TurnID{}, err
					}
					if _, err = db.ExecContext(ctx, "SAVEPOINT chain_fixture"); err != nil {
						return protocol.TurnID{}, err
					}
					output, _, err := ExecuteBackgroundCommand(ctx, db, node, node, request, now)
					if err != nil {
						if _, rollback := db.ExecContext(ctx, "ROLLBACK TO chain_fixture"); rollback != nil {
							return protocol.TurnID{}, rollback
						}
					}
					if _, release := db.ExecContext(ctx, "RELEASE chain_fixture"); release != nil {
						return protocol.TurnID{}, release
					}
					if err != nil {
						return protocol.TurnID{}, err
					}
					return output.Data.(protocol.QueuedTurn).ID, nil
				}
				// run starts the queued turn, records its input and ends it.
				run := func(turn protocol.TurnID) (*AgentInvocation, error) {
					next, _, err := ClaimNext(ctx, db, now)
					if err != nil {
						return nil, err
					}
					if next == nil || next.Run.Turn != turn {
						return nil, errors.New("queued automatic turn was not claimed")
					}
					if _, _, _, err = BindBackground(ctx, db, turn); err != nil {
						return nil, err
					}
					if err = queueFixtureInput(ctx, db, session.ID, turn, now); err != nil {
						return nil, err
					}
					_, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now)
					return next, err
				}
				continued := func(count int, limit *uint32) error {
					for index := 0; index < count; index++ {
						turn, err := admit(limit, nil)
						if err != nil {
							return fmt.Errorf("continuation %d was refused: %w", index, err)
						}
						if _, err = run(turn); err != nil {
							return err
						}
					}
					return nil
				}
				// refused reads the reason a refused continuation carries.
				refused := func(err error) (protocol.FaultDetailsContinuationPayload, error) {
					var fault *protocol.Fault
					if !errors.As(err, &fault) || fault.Details == nil || fault.Details.Kind != "continuation" {
						return protocol.FaultDetailsContinuationPayload{}, fmt.Errorf("refusal carries no reason: %v", err)
					}
					return fault.Details.Data.(protocol.FaultDetailsContinuationPayload), nil
				}
				notice := func(key string) (*AgentInvocation, error) {
					if _, err := backgroundFixtureRecord(ctx, db, session, first, key, true, now); err != nil {
						return nil, err
					}
					next, _, err := ClaimNext(ctx, db, now)
					if err != nil || next == nil {
						return nil, err
					}
					if !next.Run.Notice {
						return nil, errors.New("claimed turn is not a notice turn")
					}
					if _, _, _, err = BindBackground(ctx, db, next.Run.Turn); err != nil {
						return nil, err
					}
					if err = queueFixtureInput(ctx, db, session.ID, next.Run.Turn, now); err != nil {
						return nil, err
					}
					_, _, err = FinishTurn(ctx, db, next.Run.Turn, protocol.ConversationStatusCompleted, nil, now)
					return next, err
				}
				user := func() (protocol.TurnID, error) {
					current, err := ReadSession(ctx, db, session.ID)
					if err != nil {
						return protocol.TurnID{}, err
					}
					turn, err := delegationFixtureParent(ctx, db, node, current, now)
					if err != nil {
						return turn, err
					}
					if _, _, _, err = BindBackground(ctx, db, turn); err != nil {
						return turn, err
					}
					if err = queueFixtureInput(ctx, db, session.ID, turn, now); err != nil {
						return turn, err
					}
					_, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now)
					return turn, err
				}
				switch name {
				case "default refuses the ninth automatic turn":
					if err := continued(automaticLimit, nil); err != nil {
						return err
					}
					_, err := admit(nil, nil)
					details, e := refused(err)
					if e != nil {
						return e
					}
					if !providerFixtureFault(err, protocol.ErrorCodeConflict) || details.Reason != protocol.ConversationContinuationRefusalLimit || details.Count == nil || *details.Count != automaticLimit || details.Limit == nil || *details.Limit != automaticLimit {
						return fmt.Errorf("ninth continuation was refused as %+v: %v", details, err)
					}
				case "declared limit owns the chain":
					limit := uint32(25)
					if err := continued(25, &limit); err != nil {
						return err
					}
					_, err := admit(&limit, nil)
					details, e := refused(err)
					if e != nil {
						return e
					}
					if details.Reason != protocol.ConversationContinuationRefusalLimit || *details.Count != 25 || *details.Limit != 25 {
						return fmt.Errorf("continuation past the declared limit was refused as %+v", details)
					}
					for _, bad := range []uint32{0, automaticLimitMax + 1} {
						if _, err = admit(&bad, nil); !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) {
							return fmt.Errorf("limit %d was accepted: %v", bad, err)
						}
					}
				case "user input resets the chain":
					if err := continued(automaticLimit, nil); err != nil {
						return err
					}
					if _, err := user(); err != nil {
						return err
					}
					chain, err := ReadAutomaticChain(ctx, db, session.ID)
					if err != nil || chain.Count != 0 || chain.Limit != automaticLimit {
						return fmt.Errorf("user input left the chain at %+v: %v", chain, err)
					}
					if err = continued(1, nil); err != nil {
						return err
					}
				case "notice turns share the declared limit":
					limit := uint32(11)
					if err := continued(2, &limit); err != nil {
						return err
					}
					// Nine more automatic turns reach eleven: past the default,
					// inside the limit the chain's continuation declared.
					for index := 0; index < 9; index++ {
						if next, err := notice(fmt.Sprintf("result-%d", index)); err != nil || next == nil {
							return fmt.Errorf("notice turn %d did not start: %v", index, err)
						}
					}
					if next, err := notice("held"); err != nil || next != nil {
						return fmt.Errorf("chain passed its declared limit: %v", err)
					}
					held, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id='held'")
					if err != nil {
						return err
					}
					if held == nil || !held.Task.Suppressed || held.Task.Hold == nil || *held.Task.Hold != protocol.ConversationBackgroundHoldLimit {
						return errors.New("result past the limit is not held")
					}
					_, err = admit(&limit, nil)
					details, e := refused(err)
					if e != nil {
						return e
					}
					if details.Reason != protocol.ConversationContinuationRefusalLimit || *details.Count != 11 {
						return fmt.Errorf("notice turns did not count toward the continuation's limit: %+v", details)
					}
				case "held result binds to the next continuation":
					for index := 0; index < automaticLimit; index++ {
						if next, err := notice(fmt.Sprintf("result-%d", index)); err != nil || next == nil {
							return fmt.Errorf("notice turn %d did not start: %v", index, err)
						}
					}
					if next, err := notice("held"); err != nil || next != nil {
						return fmt.Errorf("chain passed its limit: %v", err)
					}
					limit := uint32(25)
					turn, err := admit(&limit, nil)
					if err != nil {
						return fmt.Errorf("a continuation with its own limit was refused: %w", err)
					}
					next, _, err := ClaimNext(ctx, db, now)
					if err != nil || next == nil || next.Run.Turn != turn {
						return fmt.Errorf("continuation was not claimed: %v", err)
					}
					bound, text, _, err := BindBackground(ctx, db, turn)
					if err != nil {
						return err
					}
					// The fixture never confirms a notice turn's input, so the
					// earlier results are released to this turn as well.
					if len(bound.Notices) == 0 || bound.Notices[len(bound.Notices)-1].ID != "held" || text == nil {
						return fmt.Errorf("continuation bound %d results without the held one", len(bound.Notices))
					}
					held, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id='held'")
					if err != nil {
						return err
					}
					if held == nil || held.Task.Delivery == nil || *held.Task.Delivery != turn || held.Task.Hold != nil {
						return errors.New("held result was not reserved for the continuation")
					}
				case "refusals name their reason":
					reason := func(want protocol.ConversationContinuationRefusal, code protocol.ErrorCode) error {
						_, err := admit(nil, nil)
						details, e := refused(err)
						if e != nil {
							return e
						}
						if details.Reason != want || !providerFixtureFault(err, code) {
							return fmt.Errorf("refusal is %s, want %s: %v", details.Reason, want, err)
						}
						return nil
					}
					if err := setQueuePaused(ctx, db, session.ID, true); err != nil {
						return err
					}
					if err := reason(protocol.ConversationContinuationRefusalQueuePaused, protocol.ErrorCodeBusy); err != nil {
						return err
					}
					if err := setQueuePaused(ctx, db, session.ID, false); err != nil {
						return err
					}
					// The named source is no longer the session's latest turn.
					if _, err := user(); err != nil {
						return err
					}
					input := protocol.CommandContinueTurnPayload{Session: session.ID, After: &first, Message: protocol.ConversationInput{Text: "Continue"}, Key: "loop", ExpectedRevision: value.Revision, Scope: protocol.PluginStorageScopeNode}
					request := protocol.NewRequest(node, protocol.Command{Kind: "continue_turn", Data: &input})
					request.Plugin = &scope
					_, _, err := ExecuteBackgroundCommand(ctx, db, node, node, request, now)
					details, e := refused(err)
					if e != nil {
						return e
					}
					if details.Reason != protocol.ConversationContinuationRefusalStale || !providerFixtureFault(err, protocol.ErrorCodeConflict) {
						return fmt.Errorf("stale source was refused as %s: %v", details.Reason, err)
					}
					// A user turn is running.
					current, err := ReadSession(ctx, db, session.ID)
					if err != nil {
						return err
					}
					if _, err = delegationFixtureParent(ctx, db, node, current, now); err != nil {
						return err
					}
					if err = reason(protocol.ConversationContinuationRefusalRunning, protocol.ErrorCodeBusy); err != nil {
						return err
					}
					// A second message waits behind it.
					if _, err = queueFixtureCommand(ctx, db, storage, node, "submit_turn", &protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: current.Revision, Message: protocol.ConversationInput{Text: "Queued"}}, now); err != nil {
						return err
					}
					return reason(protocol.ConversationContinuationRefusalQueuedInput, protocol.ErrorCodeBusy)
				case "plan mode refuses":
					_, err := admit(nil, nil)
					details, e := refused(err)
					if e != nil {
						return e
					}
					if details.Reason != protocol.ConversationContinuationRefusalPlan || !providerFixtureFault(err, protocol.ErrorCodeConflict) {
						return fmt.Errorf("Plan mode refused as %s: %v", details.Reason, err)
					}
				case "continuation names a declared marker":
					unknown := "missing"
					if _, err := admit(nil, &unknown); !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) {
						return fmt.Errorf("undeclared marker was accepted: %v", err)
					}
					verify, limit := "verify", uint32(25)
					turn, err := admit(&limit, &verify)
					if err != nil {
						return err
					}
					stored, err := ReadAgentRun(ctx, db, turn)
					if err != nil {
						return err
					}
					if stored.Continuation == nil || stored.Continuation.Kind == nil || *stored.Continuation.Kind != "verify" || stored.Continuation.Marker == nil || stored.Continuation.Marker.Label != "Verify work" || stored.Continuation.Limit == nil || *stored.Continuation.Limit != 25 {
						return fmt.Errorf("continuation stored %+v", stored.Continuation)
					}
					if _, err = run(turn); err != nil {
						return err
					}
					turn, err = admit(nil, nil)
					if err != nil {
						return err
					}
					stored, err = ReadAgentRun(ctx, db, turn)
					if err != nil {
						return err
					}
					if stored.Continuation.Kind != nil || stored.Continuation.Marker == nil || stored.Continuation.Marker.Label != "Continue work" {
						return fmt.Errorf("default continuation stored %+v", stored.Continuation)
					}
				case "automatic turn takes back the unread mark":
					before, err := readActivity(ctx, db, session.ID)
					if err != nil {
						return err
					}
					if !before.Attention.Unread {
						return errors.New("the ended user turn raised no attention")
					}
					turn, err := admit(nil, nil)
					if err != nil {
						return err
					}
					// Admission alone changes nothing: a restart may withdraw the turn.
					queued, err := readActivity(ctx, db, session.ID)
					if err != nil {
						return err
					}
					if !queued.Attention.Unread {
						return errors.New("a queued automatic turn cleared the unread mark")
					}
					if next, _, err := ClaimNext(ctx, db, now); err != nil || next == nil || next.Run.Turn != turn {
						return fmt.Errorf("continuation was not claimed: %v", err)
					}
					started, err := readActivity(ctx, db, session.ID)
					if err != nil {
						return err
					}
					if started.Attention.Unread {
						return errors.New("a started automatic turn left the session unread")
					}
					if err = queueFixtureInput(ctx, db, session.ID, turn, now); err != nil {
						return err
					}
					if _, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
						return err
					}
					ended, err := readActivity(ctx, db, session.ID)
					if err != nil {
						return err
					}
					if !ended.Attention.Unread {
						return errors.New("the chain's last turn raised no attention")
					}
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}
