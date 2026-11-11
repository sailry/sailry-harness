package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestScheduledRunsAfterMerge describes, from the scheduling side, behaviour
// that the plugin authority and the stop status own. Each scenario fires a
// task the way the scheduled-tasks package registers it: one handler with a
// Node-wide scope whose callback is start_session.
func TestScheduledRunsAfterMerge(t *testing.T) {
	t.Parallel()
	type task struct {
		storage *Store
		node    protocol.NodeID
		base    protocol.Session
		info    protocol.PluginInfo
		draft   protocol.ConversationStart
		scope   protocol.DispatchScope
		now     time.Time
	}
	prepare := func(t *testing.T) *task {
		t.Helper()
		storage, node, base, _, now := agentHistoryFixture(t, protocol.WorkModeCode)
		item := &task{storage: storage, node: node, base: base, now: now, info: pluginTestInfo("scheduled-tasks", 'b')}
		item.info.Summary.Revision, item.info.Summary.Enabled = 1, true
		item.info.Extension = new(protocol.DefaultPluginExtension())
		item.info.Extension.Actions = []protocol.PluginAction{protocol.PluginActionDispatch, protocol.PluginActionStartSessions}
		item.draft = protocol.DefaultConversationStart()
		item.draft.Config, item.draft.Title = &base.Config, "Daily report"
		item.draft.Execution = protocol.ConversationExecutionModel
		item.draft.Message.Text = "Write the daily report"
		item.draft.Association = &protocol.ConversationAssociation{Namespace: item.info.Summary.Name, Kind: "task", ID: "daily"}
		// The package stores a worktree only together with its project, so
		// the fixture's worktree belongs to one.
		err := storage.Node.Write(t.Context(), func(db *sql.Conn) error {
			appearance, err := encode(protocol.DefaultProjectsAppearance())
			if err != nil {
				return err
			}
			project := protocol.NewProjectID()
			if _, err := db.ExecContext(t.Context(), "INSERT INTO projects(id,name,path,appearance) VALUES(?1,'Fixture','/fixture-project',?2)", project.String(), appearance); err != nil {
				return err
			}
			_, err = db.ExecContext(t.Context(), "UPDATE worktrees SET project=?2 WHERE id=?1", base.Worktree.String(), project.String())
			return err
		})
		if err != nil {
			t.Fatal(err)
		}
		return item
	}
	// fire registers the task, lets its schedule come due and returns the
	// callback request the dispatcher submits with the job that owns it.
	fire := func(ctx context.Context, db *sql.Conn, item *task) (protocol.Request, protocol.JobID, error) {
		body, err := encode(item.info)
		if err != nil {
			return protocol.Request{}, protocol.JobID{}, err
		}
		reference := dispatchReference(item.info.Summary)
		if _, err := db.ExecContext(ctx, "INSERT INTO plugins(name,revision,installed,body) VALUES(?1,1,1,?2)", reference.Name, body); err != nil {
			return protocol.Request{}, protocol.JobID{}, err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO plugin_packages(name,digest,body) VALUES(?1,?2,?3)", reference.Name, reference.Digest, body); err != nil {
			return protocol.Request{}, protocol.JobID{}, err
		}
		if _, err := saveDispatchHandler(ctx, db, reference, protocol.DispatchHandler{
			Name: "daily", Enabled: true, Source: protocol.DispatchSource{Package: reference.Name, Topic: "daily"}, Queue: "default",
			Callback: protocol.DispatchCallback{Scope: item.scope, Completion: protocol.DispatchCompletionTurn, Bindings: map[string]string{},
				Command: &protocol.Command{Kind: "start_session", Data: &item.draft}},
		}); err != nil {
			return protocol.Request{}, protocol.JobID{}, err
		}
		if _, err := saveDispatchSchedule(ctx, db, reference.Name, protocol.DispatchSchedule{
			ID: protocol.NewScheduleID(), Enabled: true, Topic: "daily", Payload: json.RawMessage("null"),
			Timing: protocol.DispatchTiming{Kind: "once", Data: protocol.DispatchTimingOncePayload{AtMs: 1000}},
		}, 0); err != nil {
			return protocol.Request{}, protocol.JobID{}, err
		}
		batch, _, err := DispatchTick(ctx, db, item.node, 1500, false)
		if err != nil {
			return protocol.Request{}, protocol.JobID{}, err
		}
		if len(batch.Work) != 1 || batch.Work[0].Request.Plugin == nil {
			return protocol.Request{}, protocol.JobID{}, fmt.Errorf("the due task was not claimed: %+v", batch)
		}
		return batch.Work[0].Request, batch.Work[0].Job, nil
	}
	// start admits the callback as the Node does and returns its turn.
	start := func(ctx context.Context, db *sql.Conn, item *task, request protocol.Request, job protocol.JobID) (protocol.QueuedTurn, error) {
		body, err := encode(request)
		if err != nil {
			return protocol.QueuedTurn{}, err
		}
		if previous, err := admission.Admit(ctx, db, item.node, request, body); err != nil || previous != nil {
			return protocol.QueuedTurn{}, fmt.Errorf("unexpected prior admission: %v", err)
		}
		output, events, err := ExecuteAgentCommand(ctx, db, item.node, item.storage.Profile, item.node, request, item.now)
		if err != nil {
			return protocol.QueuedTurn{}, err
		}
		if _, err := RecordEvents(ctx, db, item.node, events); err != nil {
			return protocol.QueuedTurn{}, err
		}
		if err := admission.Complete(ctx, db, item.node, request.ID, link.Response{Output: output}); err != nil {
			return protocol.QueuedTurn{}, err
		}
		turn := output.Data.(protocol.QueuedTurn)
		owned, err := ReadDispatchTurn(ctx, db, item.node, item.info.Summary.Name, job)
		if err == nil && owned != turn.ID {
			err = errors.New("the job does not own the turn its callback started")
		}
		return turn, err
	}
	worktreeOf := func(ctx context.Context, db *sql.Conn, id protocol.WorktreeID) (protocol.Worktree, error) {
		worktrees, err := ReadWorktrees(ctx, db)
		if err != nil {
			return protocol.Worktree{}, err
		}
		for _, worktree := range worktrees {
			if worktree.ID == id {
				return worktree, nil
			}
		}
		return protocol.Worktree{}, errors.New("fixture worktree is missing")
	}

	// start_session has its own rule in plugin_authority.go: the generic
	// comparison would refuse a draft worktree under a Node-wide scope.
	t.Run("task_bound_to_a_worktree_starts_its_session_at_fire", func(t *testing.T) {
		t.Parallel()
		item := prepare(t)
		ctx := t.Context()
		err := item.storage.Node.Write(ctx, func(db *sql.Conn) error {
			worktree, err := worktreeOf(ctx, db, item.base.Worktree)
			if err != nil {
				return err
			}
			item.draft.Project, item.draft.Worktree = worktree.Project, &worktree.ID
			request, job, err := fire(ctx, db, item)
			if err != nil {
				return err
			}
			if err := CheckPluginRequest(ctx, db, request, item.info); err != nil {
				return fmt.Errorf("the fire of a task bound to a worktree was refused: %w", err)
			}
			turn, err := start(ctx, db, item, request, job)
			if err != nil {
				return err
			}
			session, err := ReadSession(ctx, db, turn.Session)
			if err == nil && session.Worktree != worktree.ID {
				err = errors.New("the task's session started in another worktree")
			}
			return err
		})
		if err != nil {
			t.Fatal(err)
		}
	})
	// The control: a failure of the scenario above alone is the worktree
	// rule and not this fixture.
	t.Run("task_without_a_worktree_starts_its_session_at_fire", func(t *testing.T) {
		t.Parallel()
		item := prepare(t)
		ctx := t.Context()
		err := item.storage.Node.Write(ctx, func(db *sql.Conn) error {
			request, job, err := fire(ctx, db, item)
			if err != nil {
				return err
			}
			if err := CheckPluginRequest(ctx, db, request, item.info); err != nil {
				return fmt.Errorf("the fire of a task without a worktree was refused: %w", err)
			}
			_, err = start(ctx, db, item, request, job)
			return err
		})
		if err != nil {
			t.Fatal(err)
		}
	})
	// The exemption is for a Node-wide scope only: a handler captured in one
	// worktree still cannot start a session in another.
	t.Run("task_fire_outside_its_captured_worktree_is_refused", func(t *testing.T) {
		t.Parallel()
		item := prepare(t)
		ctx := t.Context()
		err := item.storage.Node.Write(ctx, func(db *sql.Conn) error {
			worktree, err := worktreeOf(ctx, db, item.base.Worktree)
			if err != nil {
				return err
			}
			item.draft.Project, item.draft.Worktree = worktree.Project, &worktree.ID
			item.scope.Worktree = new(protocol.NewWorktreeID())
			request, _, err := fire(ctx, db, item)
			if err != nil {
				return err
			}
			var refused *protocol.Fault
			if err := CheckPluginRequest(ctx, db, request, item.info); !errors.As(err, &refused) || refused.Code != protocol.ErrorCodePermissionDenied {
				return fmt.Errorf("a fire outside the captured worktree was admitted: %v", err)
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
	// The store half of "a task created under a team runs with that team":
	// the fire carries the team the task stored. The other half, the team
	// reaching the package's initialize input, is TestScheduledTaskTurnInput
	// in internal/node.
	t.Run("task_stored_with_a_team_runs_with_that_team", func(t *testing.T) {
		t.Parallel()
		item := prepare(t)
		ctx := t.Context()
		err := item.storage.Node.Write(ctx, func(db *sql.Conn) error {
			role := protocol.DefaultRoleProfile()
			role.ID, role.Key, role.Name = protocol.NewRoleID(), "task_reviewer", "Task reviewer"
			role.Model = &protocol.RoleModel{Provider: item.base.Config.Provider, Model: item.base.Config.Model}
			role, err := putRole(ctx, db, role, 0)
			if err != nil {
				return err
			}
			team := protocol.DefaultTeamProfile()
			team.ID, team.Key, team.Name = protocol.NewTeamID(), "task_team", "Task team"
			team.Instructions, team.Members = "Task team instructions", []protocol.RoleID{role.ID}
			if team, err = putTeam(ctx, db, team, 0); err != nil {
				return err
			}
			// The package stores the team without a revision: the fire runs
			// the team as it is then.
			item.draft.Execution, item.draft.Team = protocol.ConversationExecutionTeam, &protocol.TeamSelection{ID: team.ID}
			item.draft.Models = map[protocol.RoleID]protocol.RoleModel{role.ID: *role.Model}
			request, job, err := fire(ctx, db, item)
			if err != nil {
				return err
			}
			if err := CheckPluginRequest(ctx, db, request, item.info); err != nil {
				return fmt.Errorf("the fire of a team task was refused: %w", err)
			}
			turn, err := start(ctx, db, item, request, job)
			if err != nil {
				return err
			}
			if turn.Roles.Execution != protocol.ConversationExecutionTeam || turn.Roles.Team == nil || turn.Roles.Team.ID != team.ID ||
				turn.Roles.Team.Revision != team.Revision || turn.Roles.Instructions != team.Instructions {
				return fmt.Errorf("the task ran without its team: %+v", turn.Roles.Team)
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
	// A user's stop finishes the run as cancelled; finished interrupted it
	// would raise an error notice for a run its user ended on purpose.
	t.Run("run_its_user_stopped_raises_no_notice", func(t *testing.T) {
		t.Parallel()
		storage, _, _, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := t.Context()
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			run, err := ReadAgentRun(ctx, db, turn)
			if err != nil {
				return err
			}
			run.Status = protocol.ConversationStatusStopping
			if err := putAgentRun(ctx, db, run); err != nil {
				return err
			}
			finished, events, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now)
			if err != nil {
				return err
			}
			for _, event := range events {
				if event.Kind == "notification_changed" {
					return fmt.Errorf("a stopped run finished %s and raised a notice", finished.Status)
				}
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
}
