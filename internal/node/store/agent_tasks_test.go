package store

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func testTaskAdmission(t *testing.T) {
	for _, selection := range []string{"explicit model", "default model", "fixed team lead"} {
		t.Run(selection, func(t *testing.T) {
			t.Parallel()
			storage, node, base, _, now := agentHistoryFixture(t, protocol.WorkModeCode)
			ctx := context.Background()
			draft := protocol.DefaultConversationStart()
			draft.Config = &base.Config
			draft.Execution = protocol.ConversationExecutionModel
			draft.Title = "Automated task"
			draft.Message.Text = "Keep the original task input"
			draft.Association = &protocol.ConversationAssociation{Namespace: "automation", Kind: "issue", ID: "issue-42"}
			expected := base.Config
			var role protocol.RoleProfile
			var team protocol.TeamProfile
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				if selection == "default model" {
					stored, err := encode(base.Config)
					if err != nil {
						return err
					}
					if _, err := db.ExecContext(ctx, "INSERT INTO defaults(singleton,revision,config) VALUES(1,1,?1)", stored); err != nil {
						return err
					}
					draft.Config = nil
				}
				if selection != "fixed team lead" {
					return nil
				}
				provider, err := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE id=?1", base.Config.Provider.String())
				if err != nil {
					return err
				}
				lead := provider.Provider.Models[0]
				lead.ID = "lead"
				provider.Provider.Models = append(provider.Provider.Models, lead)
				body, err := encode(provider)
				if err != nil {
					return err
				}
				if _, err = db.ExecContext(ctx, "UPDATE providers SET body=?2 WHERE id=?1", base.Config.Provider.String(), body); err != nil {
					return err
				}
				role = protocol.DefaultRoleProfile()
				role.ID, role.Key, role.Name = protocol.NewRoleID(), "task_reviewer", "Original task reviewer"
				role.Model = &protocol.RoleModel{Provider: base.Config.Provider, Model: base.Config.Model}
				role, err = putRole(ctx, db, role, 0)
				if err != nil {
					return err
				}
				team = protocol.DefaultTeamProfile()
				team.ID, team.Key, team.Name = protocol.NewTeamID(), "task_team", "Task team"
				team.Model = &protocol.RoleModel{Provider: base.Config.Provider, Model: lead.ID}
				team.Instructions, team.Members = "Original team instructions", []protocol.RoleID{role.ID}
				team, err = putTeam(ctx, db, team, 0)
				if err != nil {
					return err
				}
				draft.Execution = protocol.ConversationExecutionTeam
				draft.Team = &protocol.TeamSelection{ID: team.ID, Revision: &team.Revision}
				draft.Models = map[protocol.RoleID]protocol.RoleModel{role.ID: *role.Model}
				expected.Model = lead.ID
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
			request := protocol.NewRequest(node, protocol.Command{Kind: "start_session", Data: &draft})
			caller := node
			caller[0]++
			body, err := encode(request)
			if err != nil {
				t.Fatal(err)
			}
			var turn protocol.QueuedTurn
			err = storage.Node.Write(ctx, func(db *sql.Conn) error {
				if previous, err := admission.Admit(ctx, db, caller, request, body); err != nil || previous != nil {
					return fmt.Errorf("unexpected prior task admission: %v", err)
				}
				output, events, err := ExecuteAgentCommand(ctx, db, node, storage.Profile, caller, request, now)
				if err != nil {
					return err
				}
				turn = output.Data.(protocol.QueuedTurn)
				if len(events) != 1 || events[0].Kind != "task_started" {
					return errors.New("first admission did not publish one task-started event")
				}
				started := events[0].Data.(protocol.EventTaskStartedPayload)
				if started.Session == nil || started.Session.ID != turn.Session || started.Session.Activity.Title != draft.Title || !reflect.DeepEqual(started.Session.Association, draft.Association) {
					return errors.New("task-started event lost the session title or association")
				}
				if turn.Request != request.ID || turn.Revision != started.Session.Revision || !reflect.DeepEqual(turn.Config, expected) {
					return errors.New("first admission changed the request or configuration")
				}
				if selection == "fixed team lead" {
					if turn.Roles.Team == nil || turn.Roles.Team.ID != team.ID || turn.Roles.Team.Revision != team.Revision || turn.Roles.Instructions != team.Instructions || len(turn.Roles.Profiles) != 1 || turn.Roles.Profiles[0].Name != role.Name || !reflect.DeepEqual(turn.Roles.Models, draft.Models) {
						return errors.New("first admission lost the selected team capture")
					}
				} else if turn.Roles.Execution != protocol.ConversationExecutionModel || len(turn.Roles.Profiles) != 0 {
					return errors.New("model execution captured a delegation roster")
				}
				if _, err = RecordEvents(ctx, db, node, events); err != nil {
					return err
				}
				return admission.Complete(ctx, db, caller, request.ID, link.Response{Output: output})
			})
			if err != nil {
				t.Fatal(err)
			}
			before := schemaInteger(t, storage.Node, "SELECT count(*) FROM turns")
			err = storage.Node.Write(ctx, func(db *sql.Conn) error {
				previous, err := admission.Admit(ctx, db, caller, request, body)
				if err != nil {
					return err
				}
				if previous == nil || previous.Fault != nil || previous.Output.Kind != "queued_turn" {
					return errors.New("original request did not recover its completed admission")
				}
				replayed := previous.Output.Data.(*protocol.QueuedTurn)
				if replayed.ID != turn.ID || replayed.Request != request.ID {
					return errors.New("request replay admitted another task")
				}
				session, err := ReadSession(ctx, db, turn.Session)
				if err != nil {
					return err
				}
				session.Config.Mode = protocol.WorkModePlan
				if _, _, err = reviseSession(ctx, db, session); err != nil {
					return err
				}
				provider, err := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE id=?1", base.Config.Provider.String())
				if err != nil {
					return err
				}
				provider.Provider.Endpoint = "http://changed.invalid/v1"
				changed, err := encode(provider)
				if err != nil {
					return err
				}
				if _, err = db.ExecContext(ctx, "UPDATE providers SET body=?2 WHERE id=?1", base.Config.Provider.String(), changed); err != nil {
					return err
				}
				if selection == "fixed team lead" {
					role.Name = "Changed task reviewer"
					if _, err = putRole(ctx, db, role, role.Revision); err != nil {
						return err
					}
					team.Instructions = "Changed team instructions"
					if _, err = putTeam(ctx, db, team, team.Revision); err != nil {
						return err
					}
				}
				queue, err := ReadQueue(ctx, db, turn.Session)
				if err != nil {
					return err
				}
				if len(queue.Items) != 1 || !queue.Items[0].Ready || queue.Items[0].Request == nil || *queue.Items[0].Request != request.ID || queue.Items[0].Preview != draft.Message.Text {
					return errors.New("first admission did not enter the ordinary ready queue")
				}
				invocation, _, err := ClaimNext(ctx, db, now)
				if err != nil {
					return err
				}
				if invocation == nil || invocation.Turn.ID != turn.ID || invocation.Caller != caller || invocation.Message.Text != draft.Message.Text || !reflect.DeepEqual(invocation.Turn.Config, expected) || invocation.Provider == nil || invocation.Provider.Endpoint != "http://fixture.invalid/v1" || !reflect.DeepEqual(invocation.Turn.Roles, turn.Roles) {
					return errors.New("ordinary execution lost the original input or frozen configuration")
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
			if schemaInteger(t, storage.Node, "SELECT count(*) FROM turns") != before {
				t.Fatal("original request replay created another turn")
			}
		})
	}
	t.Run("failed first admission rolls back the complete session", func(t *testing.T) {
		t.Parallel()
		storage, node, base, _, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := context.Background()
		tables := []string{"sessions", "session_revisions", "session_media", "session_associations", "worktrees", "turns", "conversation_turns", "agent_runs", "agent_pending", "agent_queues", "requests", "agent_state"}
		before := map[string]int64{}
		for _, table := range tables {
			before[table] = schemaInteger(t, storage.Node, "SELECT count(*) FROM "+table)
		}
		draft := protocol.DefaultConversationStart()
		draft.Config, draft.Title = &base.Config, "Rejected task"
		draft.Association = &protocol.ConversationAssociation{Namespace: "automation", Kind: "issue", ID: "rejected"}
		draft.Message.Text = "Reject after the first turn has been inserted"
		draft.Message.Attachments = []protocol.AttachmentID{protocol.NewAttachmentID()}
		request := protocol.NewRequest(node, protocol.Command{Kind: "start_session", Data: &draft})
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			body, err := encode(request)
			if err != nil {
				return err
			}
			if _, err = admission.Admit(ctx, db, node, request, body); err != nil {
				return err
			}
			_, _, err = ExecuteAgentCommand(ctx, db, node, storage.Profile, node, request, now)
			return err
		})
		var rejected *protocol.Fault
		if !errors.As(err, &rejected) || rejected.Code != protocol.ErrorCodeNotFound {
			t.Fatalf("expected unavailable attachment rejection, got %v", err)
		}
		for _, table := range tables {
			if after := schemaInteger(t, storage.Node, "SELECT count(*) FROM "+table); after != before[table] {
				t.Fatalf("failed first admission retained %s rows: before=%d after=%d", table, before[table], after)
			}
		}
	})
	t.Run("submit task stays ordinary and preserves optional revisions", func(t *testing.T) {
		t.Parallel()
		storage, node, session, active, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := context.Background()
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			turns := []protocol.QueuedTurn{}
			for index := 0; index < 2; index++ {
				var revision *uint64
				if index == 1 {
					revision = &session.Revision
				}
				message := protocol.DefaultConversationInput()
				message.Text = fmt.Sprintf("Queued task %d", index)
				request := protocol.NewRequest(node, protocol.Command{Kind: "submit_task", Data: &protocol.CommandSubmitTaskPayload{Session: session.ID, ExpectedRevision: revision, Message: message}})
				body, err := encode(request)
				if err != nil {
					return err
				}
				if _, err = admission.Admit(ctx, db, node, request, body); err != nil {
					return err
				}
				output, events, err := ExecuteAgentCommand(ctx, db, node, storage.Profile, node, request, now)
				if err != nil {
					return err
				}
				turn := output.Data.(protocol.QueuedTurn)
				if len(events) != 1 || events[0].Kind != "turn_queued" || turn.Request != request.ID {
					return errors.New("task submission lost ordinary queue admission")
				}
				turns = append(turns, turn)
			}
			stale := session.Revision + 1
			rejected := protocol.NewRequest(node, protocol.Command{Kind: "submit_task", Data: &protocol.CommandSubmitTaskPayload{Session: session.ID, ExpectedRevision: &stale, Message: protocol.ConversationInput{Text: "Stale task"}}})
			_, _, err := ExecuteAgentCommand(ctx, db, node, storage.Profile, node, rejected, now)
			var conflict *protocol.Fault
			if !errors.As(err, &conflict) || conflict.Code != protocol.ErrorCodeRevisionConflict {
				return errors.New("task submission accepted a stale expected revision")
			}
			rejected.Plugin = &protocol.PluginContext{}
			_, _, err = ExecuteAgentCommand(ctx, db, node, storage.Profile, node, rejected, now)
			var denied *protocol.Fault
			if !errors.As(err, &denied) || denied.Code != protocol.ErrorCodePermissionDenied {
				return errors.New("plugin task submission bypassed the native-controller boundary")
			}
			queue, err := ReadQueue(ctx, db, session.ID)
			if err != nil {
				return err
			}
			pending := []protocol.ConversationPending{}
			for _, item := range queue.Items {
				if item.Turn != active {
					pending = append(pending, item)
				}
			}
			if len(pending) != 2 || pending[0].Turn != turns[0].ID || pending[1].Turn != turns[1].ID || !pending[0].Ready || !pending[1].Ready || pending[0].Guidance != nil || pending[1].Guidance != nil {
				return errors.New("task submission joined guidance or changed queue order")
			}
			if next, _, err := ClaimNext(ctx, db, now); err != nil || next != nil {
				return fmt.Errorf("queued task bypassed active execution: %v", err)
			}
			if _, _, err = FinishTurn(ctx, db, active, protocol.ConversationStatusCompleted, nil, now); err != nil {
				return err
			}
			for index, turn := range turns {
				invocation, _, err := ClaimNext(ctx, db, now)
				if err != nil {
					return err
				}
				if invocation == nil || invocation.Turn.ID != turn.ID || invocation.Message.Text != fmt.Sprintf("Queued task %d", index) {
					return errors.New("ordinary queue lost task input or FIFO order")
				}
				if _, _, err = FinishTurn(ctx, db, turn.ID, protocol.ConversationStatusCompleted, nil, now); err != nil {
					return err
				}
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
}
