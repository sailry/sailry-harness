package store

import (
	"database/sql"
	"errors"
	"fmt"
	"reflect"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestChildPermissionInheritance(t *testing.T) {
	t.Parallel()
	for _, permission := range []protocol.Permission{protocol.PermissionAsk, protocol.PermissionProject, protocol.PermissionFull} {
		for _, readOnly := range []bool{false, true} {
			t.Run(fmt.Sprintf("%s/read_only_%t", permission, readOnly), func(t *testing.T) {
				t.Parallel()
				storage, node, session, parent, now := delegationPermissionFixture(t, permission, readOnly)
				ctx := t.Context()
				err := storage.Node.Write(ctx, func(db *sql.Conn) error {
					captured, err := readTurnCapture(ctx, db, parent)
					if err != nil {
						return err
					}
					// Settings changed after admission are not the parent's authority.
					session.Config.Mode = protocol.WorkModePlan
					session.Config.Permission = protocol.PermissionFull
					if permission == protocol.PermissionFull {
						session.Config.Permission = protocol.PermissionAsk
					}
					if _, _, err := reviseSession(ctx, db, session); err != nil {
						return err
					}
					args := map[string]any{"task": "Inspect the captured workspace"}
					if readOnly {
						args["role"] = "reviewer"
					}
					call, err := delegationFixtureCall(ctx, db, parent, "spawn_agent", args, now)
					if err != nil {
						return err
					}
					child, _, err := DelegateAgent(ctx, db, node, parent, call, nil, true, now)
					if err != nil {
						return err
					}
					expected := captured.Config
					if readOnly {
						expected.Mode = protocol.WorkModePlan
					}
					if !reflect.DeepEqual(child.Turn.Config, expected) || !reflect.DeepEqual(child.Session.Config, expected) || child.Run.Worktree != session.Worktree {
						return fmt.Errorf("child did not inherit captured authority: turn=%+v session=%+v", child.Turn.Config, child.Session.Config)
					}
					claimed, _, err := ClaimChild(ctx, db, child.Run.Turn, now)
					if err != nil || claimed == nil {
						return fmt.Errorf("child was not claimed: %v", err)
					}
					if !reflect.DeepEqual(claimed.Turn.Config, expected) {
						return fmt.Errorf("claim changed child authority: %+v", claimed.Turn.Config)
					}
					if !readOnly {
						return nil
					}
					// Both native and plugin command admission must respect Plan,
					// including a read-only child that inherited Full permission.
					for _, access := range []protocol.ProcessAccess{protocol.ProcessAccessWorkspace, protocol.ProcessAccessUnrestricted} {
						input := protocol.DefaultCommandRunCommandPayload()
						input.Turn, input.Command, input.Access = child.Run.Turn, "git --no-pager diff", access
						command := protocol.Command{Kind: "run_command", Data: &input}
						approval := protocol.DefaultConversationApproval()
						approval.Session, approval.Turn = child.Session.ID, child.Run.Turn
						approval.State, approval.Source = protocol.ConversationApprovalStateApproved, protocol.ConversationApprovalSourceFull
						if _, err := ConfigureWorkspaceApproval(ctx, db, child.Run.Turn, approval, protocol.ToolOperationRunCommand, command, nil); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
							return fmt.Errorf("read-only child authorized native %s command: %v", access, err)
						}
						scope := protocol.DefaultPluginContext()
						scope.Worktree, scope.Session, scope.Turn = &child.Session.Worktree, &child.Session.ID, &child.Run.Turn
						request := protocol.Request{Plugin: &scope, Command: command}
						if err := CheckPluginPlanAction(ctx, db, request, protocol.PluginActionControlCommands); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
							return fmt.Errorf("read-only child authorized plugin %s command: %v", access, err)
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
}

func TestChildReusePermissionBounds(t *testing.T) {
	t.Parallel()
	for _, scenario := range []struct {
		name    string
		child   protocol.Permission
		parent  protocol.Permission
		mode    protocol.WorkMode
		allowed bool
	}{
		{"full_child_ask_parent", protocol.PermissionFull, protocol.PermissionAsk, protocol.WorkModeCode, false},
		{"full_child_project_parent", protocol.PermissionFull, protocol.PermissionProject, protocol.WorkModeCode, false},
		{"project_child_ask_parent", protocol.PermissionProject, protocol.PermissionAsk, protocol.WorkModeCode, false},
		{"code_child_plan_parent", protocol.PermissionFull, protocol.PermissionFull, protocol.WorkModePlan, false},
		{"same_permission", protocol.PermissionProject, protocol.PermissionProject, protocol.WorkModeCode, true},
		{"lower_permission_child", protocol.PermissionAsk, protocol.PermissionFull, protocol.WorkModeCode, true},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			t.Parallel()
			storage, node, session, parent, now := delegationPermissionFixture(t, scenario.child, false)
			ctx := t.Context()
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				call, err := delegationFixtureCall(ctx, db, parent, "spawn_agent", map[string]any{"task": "Initial child task"}, now)
				if err != nil {
					return err
				}
				child, _, err := DelegateAgent(ctx, db, node, parent, call, nil, true, now)
				if err != nil {
					return err
				}
				if claimed, _, err := ClaimChild(ctx, db, child.Run.Turn, now); err != nil || claimed == nil {
					return fmt.Errorf("child was not claimed: %v", err)
				}
				if _, _, err := FinishTurn(ctx, db, child.Run.Turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
					return err
				}
				if _, err := ConsumeChildBackground(ctx, db, parent, child.Session.ID, &child.Run.Turn); err != nil {
					return err
				}
				if _, _, err := FinishTurn(ctx, db, parent, protocol.ConversationStatusCompleted, nil, now); err != nil {
					return err
				}
				session, err = ReadSession(ctx, db, session.ID)
				if err != nil {
					return err
				}
				session.Config.Permission, session.Config.Mode = scenario.parent, scenario.mode
				output, _, err := reviseSession(ctx, db, session)
				if err != nil {
					return err
				}
				session = output.Data.(protocol.Session)
				parent, err = delegationFixtureParent(ctx, db, node, session, now)
				if err != nil {
					return err
				}
				reuse, err := delegationFixtureCall(ctx, db, parent, "spawn_agent", map[string]any{"session": child.Session.ID, "task": "Continue the existing child"}, now)
				if err != nil {
					return err
				}
				continued, _, err := DelegateAgent(ctx, db, node, parent, reuse, nil, true, now)
				if scenario.allowed {
					if err != nil {
						return err
					}
					if continued.Session.ID != child.Session.ID || continued.Run.Turn == child.Run.Turn || !reflect.DeepEqual(continued.Turn.Config, child.Turn.Config) {
						return errors.New("permitted reuse changed the child's identity or captured settings")
					}
					return nil
				}
				var rejected *protocol.Fault
				if !errors.As(err, &rejected) || rejected.Code != protocol.ErrorCodePermissionDenied || rejected.Message != "child exceeds parent permissions" {
					return fmt.Errorf("lower-authority parent was not refused: %v", err)
				}
				saved, err := ReadSession(ctx, db, child.Session.ID)
				if err != nil {
					return err
				}
				if saved.Revision != child.Session.Revision || !reflect.DeepEqual(saved.Config, child.Turn.Config) {
					return errors.New("rejected reuse changed the child session")
				}
				var turns int
				if err := db.QueryRowContext(ctx, "SELECT count(*) FROM turns WHERE session=?1", child.Session.ID.String()).Scan(&turns); err != nil {
					return err
				}
				if turns != 1 {
					return fmt.Errorf("rejected reuse admitted another child turn: %d", turns)
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}

func delegationPermissionFixture(t *testing.T, permission protocol.Permission, readOnly bool) (*Store, protocol.NodeID, protocol.Session, protocol.TurnID, time.Time) {
	t.Helper()
	storage, node, base, _, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	var session protocol.Session
	var parent protocol.TurnID
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		config := base.Config
		config.Permission = permission
		output, _, err := createSession(ctx, db, node, storage.Profile, protocol.CommandCreateSessionPayload{Config: &config})
		if err != nil {
			return err
		}
		session = output.Data.(protocol.Session)
		if readOnly {
			role := protocol.DefaultRoleProfile()
			role.ID, role.Key, role.Name, role.ReadOnly = protocol.NewRoleID(), "reviewer", "Reviewer", true
			role, err = putRole(ctx, db, role, 0)
			if err != nil {
				return err
			}
			team := protocol.DefaultTeamProfile()
			team.ID, team.Key, team.Name, team.Members = protocol.NewTeamID(), "review", "Review", []protocol.RoleID{role.ID}
			team, err = putTeam(ctx, db, team, 0)
			if err != nil {
				return err
			}
			session.Roles, err = CaptureSessionRoles(ctx, db, node, protocol.ConversationExecutionTeam, &protocol.TeamSelection{ID: team.ID, Revision: &team.Revision}, nil)
			if err != nil {
				return err
			}
			output, _, err = reviseSession(ctx, db, session)
			if err != nil {
				return err
			}
			session = output.Data.(protocol.Session)
		}
		parent, err = delegationFixtureParent(ctx, db, node, session, now)
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
	return storage, node, session, parent, now
}
