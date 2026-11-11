package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func delegationFixtureCall(ctx context.Context, db *sql.Conn, parent protocol.TurnID, name string, args any, now time.Time) (canonical.ToolIdentity, error) {
	body, err := json.Marshal(args)
	if err != nil {
		return canonical.ToolIdentity{}, err
	}
	id := protocol.NewRequestID().String()
	call := canonical.ToolRequest{ID: id, Name: name, Arguments: body}
	record := canonical.NewRecord(parent, "message", canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_request", Data: &call}), now)
	record.Provenance.Author = "assistant"
	run, err := ReadAgentRun(ctx, db, parent)
	if err != nil {
		return canonical.ToolIdentity{}, err
	}
	_, _, err = AppendCanonical(ctx, db, run.Session, record)
	return canonical.ToolIdentity{ID: &id, Name: name, Arguments: body}, err
}
func delegationFixtureParent(ctx context.Context, db *sql.Conn, node protocol.NodeID, session protocol.Session, now time.Time) (protocol.TurnID, error) {
	request := protocol.NewRequest(node, protocol.Command{Kind: "submit_turn", Data: &protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Parent task"}}})
	body, err := encode(request)
	if err != nil {
		return protocol.TurnID{}, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", node[:], request.ID.String(), body); err != nil {
		return protocol.TurnID{}, err
	}
	turn, err := admitAgentTurn(ctx, db, node, node, request, session, protocol.ConversationRunKindTask, true)
	if err != nil {
		return turn.ID, err
	}
	inv, _, err := ClaimNext(ctx, db, now)
	if err == nil && (inv == nil || inv.Run.Turn != turn.ID) {
		return turn.ID, errors.New("parent was not claimed")
	}
	return turn.ID, err
}

func TestChildConcurrencyCap(t *testing.T) {
	t.Parallel()
	storage, node, session, parent, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := context.Background()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		children := []AgentInvocation{}
		for index := 0; index < 9; index++ {
			call, err := delegationFixtureCall(ctx, db, parent, "spawn_agent", map[string]any{"task": fmt.Sprintf("Child %d", index)}, now)
			if err != nil {
				return err
			}
			child, _, err := DelegateAgent(ctx, db, node, parent, call, nil, true, now)
			if err != nil {
				return err
			}
			children = append(children, child)
		}
		if inv, _, err := ClaimNext(ctx, db, now); err != nil || inv != nil {
			return fmt.Errorf("ordinary claim bypassed child ownership: %v", err)
		}
		if inv, _, err := ClaimChild(ctx, db, children[8].Run.Turn, now); err != nil || inv != nil {
			return fmt.Errorf("child skipped FIFO: %v", err)
		}
		for index := 0; index < 8; index++ {
			inv, _, err := ClaimChild(ctx, db, children[index].Run.Turn, now)
			if err != nil {
				return err
			}
			if inv == nil || inv.Message.Text != fmt.Sprintf("Child %d", index) {
				return errors.New("child input or available slot was lost")
			}
		}
		if inv, _, err := ClaimChild(ctx, db, children[8].Run.Turn, now); err != nil || inv != nil {
			return fmt.Errorf("ninth child bypassed cap: %v", err)
		}
		if _, _, err := FinishTurn(ctx, db, children[0].Run.Turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		if inv, _, err := ClaimChild(ctx, db, children[8].Run.Turn, now); err != nil || inv == nil {
			return fmt.Errorf("released slot was not reused: %v", err)
		}
		out, _, err := createSession(ctx, db, node, storage.Profile, protocol.CommandCreateSessionPayload{Config: &session.Config})
		if err != nil {
			return err
		}
		other := out.Data.(protocol.Session)
		second, err := delegationFixtureParent(ctx, db, node, other, now)
		if err != nil {
			return err
		}
		call, err := delegationFixtureCall(ctx, db, second, "spawn_agent", map[string]any{"task": "Independent budget"}, now)
		if err != nil {
			return err
		}
		child, _, err := DelegateAgent(ctx, db, node, second, call, nil, true, now)
		if err != nil {
			return err
		}
		if inv, _, err := ClaimChild(ctx, db, child.Run.Turn, now); err != nil || inv == nil {
			return fmt.Errorf("independent main session shared another budget: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestTeamRevisionFrozen(t *testing.T) {
	t.Parallel()
	storage, node, base, _, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := context.Background()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		role := protocol.DefaultRoleProfile()
		role.ID, role.Key, role.Name, role.ReadOnly = protocol.NewRoleID(), "reviewer", "Original reviewer", true
		role.Model = &protocol.RoleModel{Provider: base.Config.Provider, Model: base.Config.Model}
		role, err := putRole(ctx, db, role, 0)
		if err != nil {
			return err
		}
		team := protocol.DefaultTeamProfile()
		team.ID, team.Key, team.Name, team.Instructions, team.Members = protocol.NewTeamID(), "review", "Review", "Frozen team instructions", []protocol.RoleID{role.ID}
		team, err = putTeam(ctx, db, team, 0)
		if err != nil {
			return err
		}
		out, _, err := createSession(ctx, db, node, storage.Profile, protocol.CommandCreateSessionPayload{Config: &base.Config})
		if err != nil {
			return err
		}
		session := out.Data.(protocol.Session)
		session.Roles, err = CaptureSessionRoles(ctx, db, node, protocol.ConversationExecutionTeam, &protocol.TeamSelection{ID: team.ID, Revision: &team.Revision}, nil)
		if err != nil {
			return err
		}
		out, _, err = reviseSession(ctx, db, session)
		if err != nil {
			return err
		}
		session = out.Data.(protocol.Session)
		parent, err := delegationFixtureParent(ctx, db, node, session, now)
		if err != nil {
			return err
		}
		before, err := readTurnCapture(ctx, db, parent)
		if err != nil {
			return err
		}
		originalEndpoint := before.Roles.Providers[0].Endpoint
		role.Name = "Changed reviewer"
		role, err = putRole(ctx, db, role, role.Revision)
		if err != nil {
			return err
		}
		if _, err = putRole(ctx, db, role, role.Revision-1); err == nil {
			return errors.New("stale role revision overwrote catalog")
		}
		team.Instructions = "Changed team instructions"
		team, err = putTeam(ctx, db, team, team.Revision)
		if err != nil {
			return err
		}
		provider, err := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE id=?1", base.Config.Provider.String())
		if err != nil {
			return err
		}
		provider.Provider.Endpoint = "http://changed.invalid/v1"
		body, err := encode(provider)
		if err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "UPDATE providers SET body=?2 WHERE id=?1", base.Config.Provider.String(), body); err != nil {
			return err
		}
		after, err := readTurnCapture(ctx, db, parent)
		if err != nil {
			return err
		}
		if after.Roles.Profiles[0].Name != "Original reviewer" || after.Roles.Instructions != "Frozen team instructions" || after.Roles.Providers[0].Endpoint != originalEndpoint {
			return errors.New("catalog edits mutated an admitted roster")
		}
		call, err := delegationFixtureCall(ctx, db, parent, "spawn_agent", map[string]any{"task": "Review", "role": "reviewer"}, now)
		if err != nil {
			return err
		}
		child, _, err := DelegateAgent(ctx, db, node, parent, call, nil, true, now)
		if err != nil {
			return err
		}
		if child.Turn.Config.Mode != protocol.WorkModePlan || child.Provider == nil || child.Provider.Endpoint != originalEndpoint {
			return errors.New("child did not retain frozen role model and Plan permission")
		}
		if _, _, err = ExecuteTeamCommand(ctx, db, node, protocol.Command{Kind: "remove_role", Data: &protocol.CommandRemoveRolePayload{Role: role.ID, ExpectedRevision: role.Revision}}); err != nil {
			return err
		}
		teams, err := ReadTeams(ctx, db)
		if err != nil {
			return err
		}
		if len(teams) != 1 || len(teams[0].Members) != 0 || teams[0].Revision != team.Revision+1 {
			return errors.New("role removal did not revise and prune its team")
		}
		captured, err := readTurnCapture(ctx, db, parent)
		if err != nil {
			return err
		}
		if len(captured.Roles.Profiles) != 1 {
			return errors.New("role deletion removed a historical capture")
		}
		// A controller may still name a team another device removed.
		removed := protocol.DefaultConversationStart()
		removed.Config, removed.Execution, removed.Team = &base.Config, protocol.ConversationExecutionTeam, &protocol.TeamSelection{ID: protocol.NewTeamID()}
		var absent *protocol.Fault
		if _, err = StartSessionProvider(ctx, db, removed); !errors.As(err, &absent) || absent.Code != protocol.ErrorCodeNotFound || absent.Message != "team does not exist" {
			return fmt.Errorf("removed team selection: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestChildOwnershipAndGuidance(t *testing.T) {
	t.Parallel()
	storage, node, session, parent, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := context.Background()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		call, err := delegationFixtureCall(ctx, db, parent, "spawn_agent", map[string]any{"task": "Original child"}, now)
		if err != nil {
			return err
		}
		child, _, err := DelegateAgent(ctx, db, node, parent, call, nil, false, now)
		if err != nil {
			return err
		}
		if inv, _, err := ClaimChild(ctx, db, child.Run.Turn, now); err != nil || inv != nil {
			return fmt.Errorf("unreleased child executed: %v", err)
		}
		if _, _, err = DelegateAgent(ctx, db, node, parent, call, nil, true, now); err == nil {
			return errors.New("duplicate canonical call was replayed")
		}
		if _, err = ReleaseChildren(ctx, db, parent, []protocol.TurnID{child.Run.Turn}); err != nil {
			return err
		}
		if inv, _, err := ClaimChild(ctx, db, child.Run.Turn, now); err != nil || inv == nil {
			return fmt.Errorf("released child was not claimed: %v", err)
		}
		nested, err := delegationFixtureCall(ctx, db, child.Run.Turn, "spawn_agent", map[string]any{"task": "Forbidden nested child"}, now)
		if err != nil {
			return err
		}
		if _, _, err = DelegateAgent(ctx, db, node, child.Run.Turn, nested, nil, true, now); err == nil {
			return errors.New("child delegated to another child")
		}
		guide, err := delegationFixtureCall(ctx, db, parent, "guide_agent", map[string]any{"session": child.Session.ID, "message": "Keep the original capture"}, now)
		if err != nil {
			return err
		}
		receipt, next, _, err := GuideChild(ctx, db, node, parent, guide, guide.Arguments, now)
		if err != nil {
			return err
		}
		if next != nil {
			return errors.New("running guidance created another worker")
		}
		if receipt.Run.Turn == child.Run.Turn || receipt.Recipient == nil || *receipt.Recipient != child.Run.Turn || receipt.Run.Status != protocol.ConversationStatusQueued {
			return errors.New("guidance receipt lost the queued turn or active recipient")
		}
		pending, err := PendingGuidance(ctx, db, child.Run.Turn)
		if err != nil || !pending {
			return fmt.Errorf("guidance was not durable: %v", err)
		}
		guidance, _, err := TakeGuidance(ctx, db, child.Run.Turn)
		if err != nil {
			return err
		}
		if len(guidance) != 1 || guidance[0].Turn.ID != receipt.Run.Turn || guidance[0].Message.Text != "Keep the original capture" || !sameAssistant(guidance[0].Turn.Config, child.Turn.Config) {
			return errors.New("guidance changed the child capture or task")
		}
		reply := canonical.NewRecord(child.Run.Turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Final child response")), now)
		reply.Provenance.Author = "assistant"
		if _, _, err = AppendCanonical(ctx, db, child.Session.ID, reply); err != nil {
			return err
		}
		if _, _, err = FinishTurn(ctx, db, child.Run.Turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		state, err := ReadChildState(ctx, db, parent, child.Session.ID, &child.Run.Turn)
		if err != nil {
			return err
		}
		if state.Response == nil || *state.Response != "Final child response" {
			return errors.New("owned child final response was lost")
		}
		other, _, err := createSession(ctx, db, node, storage.Profile, protocol.CommandCreateSessionPayload{Config: &session.Config})
		if err != nil {
			return err
		}
		alien, err := delegationFixtureParent(ctx, db, node, other.Data.(protocol.Session), now)
		if err != nil {
			return err
		}
		if _, err = ReadChildState(ctx, db, alien, child.Session.ID, nil); err == nil {
			return errors.New("unrelated main session read a child")
		}
		idleGuide, err := delegationFixtureCall(ctx, db, parent, "guide_agent", map[string]any{"session": child.Session.ID, "message": "Resume after the canonical batch"}, now)
		if err != nil {
			return err
		}
		receipt, next, _, err = GuideChild(ctx, db, node, parent, idleGuide, idleGuide.Arguments, now)
		if err != nil {
			return err
		}
		if next == nil || receipt.Run.Turn != next.Run.Turn || receipt.Recipient == nil || *receipt.Recipient != child.Run.Turn {
			return errors.New("idle guidance lost its foreground invocation or previous recipient")
		}
		if inv, _, err := ClaimChild(ctx, db, next.Run.Turn, now); err != nil || inv != nil {
			return fmt.Errorf("idle guidance ran before its canonical batch: %v", err)
		}
		if _, err = ReleaseChildren(ctx, db, parent, []protocol.TurnID{next.Run.Turn}); err != nil {
			return err
		}
		if inv, _, err := ClaimChild(ctx, db, next.Run.Turn, now); err != nil || inv == nil {
			return fmt.Errorf("idle guidance was not released: %v", err)
		}
		if _, _, err = FinishTurn(ctx, db, next.Run.Turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		reuse, err := delegationFixtureCall(ctx, db, parent, "spawn_agent", map[string]any{"session": child.Session.ID, "task": "Continue the child"}, now)
		if err != nil {
			return err
		}
		continued, _, err := DelegateAgent(ctx, db, node, parent, reuse, nil, true, now)
		if err != nil {
			return err
		}
		if continued.Session.ID != child.Session.ID || continued.Turn.ID == child.Turn.ID || !sameAssistant(continued.Turn.Config, child.Turn.Config) {
			return errors.New("idle reuse replaced child settings or identity")
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func backgroundFixtureRecord(ctx context.Context, db *sql.Conn, session protocol.Session, turn protocol.TurnID, key string, follow bool, now time.Time) (BackgroundRecord, error) {
	outcome := protocol.ConversationBackgroundOutcomeCompleted
	finished := now.UnixMilli()
	record := BackgroundRecord{Task: protocol.ConversationBackgroundTask{ID: key, Session: session.ID, Turn: turn, Kind: protocol.ConversationBackgroundKind{Kind: "command", Data: protocol.ConversationBackgroundKindCommandPayload{ID: protocol.NewRequestID()}}, Title: "Completed task", State: protocol.ConversationBackgroundStatePending, Outcome: &outcome, StartedMs: finished, FinishedMs: &finished}, FollowUp: follow}
	sequence, err := NextCounter(ctx, db, CounterBackground)
	if err != nil {
		return record, err
	}
	record.Sequence = sequence
	err = saveBackgroundRecord(ctx, db, &record)
	return record, err
}

// Over 1 s only beside the package's other parallel tests: each opens its own synced store.
func TestBackgroundNoticeEvidence(t *testing.T) {
	t.Parallel()
	t.Run("command tool collection", func(t *testing.T) {
		for _, row := range []struct {
			name     string
			finished bool
			reserved bool
			status   string
		}{
			{"terminal read before completion callback", false, false, "finished"},
			{"terminal read after completion callback", true, false, "finished"},
			{"terminal read consumes reserved evidence", true, true, "finished"},
			{"running snapshot", false, false, "running"},
			{"stopping snapshot", false, false, "stopping"},
		} {
			t.Run(row.name, func(t *testing.T) {
				t.Parallel()
				storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
				ctx := context.Background()
				err := storage.Node.Write(ctx, func(db *sql.Conn) error {
					input := protocol.DefaultCommandRunCommandPayload()
					input.Turn, input.Command, input.Background, input.FollowUp = turn, "fixture command", true, true
					request := protocol.NewRequest(node, protocol.Command{Kind: "run_command", Data: &input})
					if _, err := RegisterCommand(ctx, db, request, now); err != nil {
						return err
					}
					snapshot := protocol.ProcessSnapshot{Info: protocol.ProcessInfo{ID: request.ID, Session: session.ID, Turn: turn, Status: protocol.ProcessStatus{Kind: "finished", Data: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}}, Stdout: protocol.ProcessCapture{Text: "Observed result"}}
					elapsed := uint64(12)
					if row.finished {
						if _, err := FinishCommand(ctx, db, process.Finished{Snapshot: snapshot, ElapsedMs: &elapsed}, now); err != nil {
							return err
						}
						tasks, err := ReadBackgroundTasks(ctx, db, session.ID)
						if err != nil {
							return err
						}
						if len(tasks) != 1 || tasks[0].State != protocol.ConversationBackgroundStatePending {
							return errors.New("controller observation consumed command evidence")
						}
					}
					if row.reserved {
						if _, _, _, err := BindBackground(ctx, db, turn); err != nil {
							return err
						}
					}
					snapshot.Info.Status.Kind = row.status
					if _, err := ConsumeCommandBackground(ctx, db, turn, snapshot, now); err != nil {
						return err
					}
					record, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", "command:"+request.ID.String())
					if err != nil {
						return err
					}
					if row.status != "finished" {
						if record.Task.State != protocol.ConversationBackgroundStateRunning || record.Task.Revision != 1 || record.Task.Outcome != nil {
							return errors.New("nonterminal command snapshot consumed completion evidence")
						}
						return nil
					}
					if record.Task.State != protocol.ConversationBackgroundStateConsumed || record.Task.Delivery != nil || record.Stdout == nil || *record.Stdout != snapshot.Stdout.Text || record.Task.Outcome == nil || *record.Task.Outcome != protocol.ConversationBackgroundOutcomeCompleted {
						return errors.New("terminal tool read did not consume its exact observed result")
					}
					revision := record.Task.Revision
					if _, err = ConsumeCommandBackground(ctx, db, turn, snapshot, now); err != nil {
						return err
					}
					lateElapsed := uint64(50)
					if _, err = FinishCommand(ctx, db, process.Finished{Snapshot: snapshot, ElapsedMs: &lateElapsed}, now.Add(time.Second)); err != nil {
						return err
					}
					record, err = readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", "command:"+request.ID.String())
					if err != nil {
						return err
					}
					timingChanged := row.finished && (record.ElapsedMs == nil || *record.ElapsedMs != elapsed) || !row.finished && record.ElapsedMs != nil
					if record.Task.State != protocol.ConversationBackgroundStateConsumed || record.Task.Revision != revision || timingChanged {
						return errors.New("late completion callback or repeated tool read changed consumed evidence")
					}
					run, text, _, err := BindBackground(ctx, db, turn)
					if err != nil {
						return err
					}
					if len(run.Notices) != 0 || text != nil {
						return errors.New("consumed tool output was reserved for duplicate delivery")
					}
					if _, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
						return err
					}
					if next, _, err := ScheduleBackground(ctx, db, node, session.ID, now); err != nil || next != nil {
						return fmt.Errorf("consumed tool output scheduled automatic input: %v", err)
					}
					return nil
				})
				if err != nil {
					t.Fatal(err)
				}
			})
		}
	})
	t.Run("command collection ownership", func(t *testing.T) {
		for _, name := range []string{"another session", "another launch turn", "unregistered command", "inactive parent"} {
			t.Run(name, func(t *testing.T) {
				t.Parallel()
				storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
				ctx := context.Background()
				err := storage.Node.Write(ctx, func(db *sql.Conn) error {
					input := protocol.DefaultCommandRunCommandPayload()
					input.Turn, input.Command, input.Background = turn, "owned command", true
					request := protocol.NewRequest(node, protocol.Command{Kind: "run_command", Data: &input})
					if _, err := RegisterCommand(ctx, db, request, now); err != nil {
						return err
					}
					snapshot := protocol.ProcessSnapshot{Info: protocol.ProcessInfo{ID: request.ID, Session: session.ID, Turn: turn, Status: protocol.ProcessStatus{Kind: "finished", Data: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}}}
					switch name {
					case "another session":
						snapshot.Info.Session = protocol.NewSessionID()
					case "another launch turn":
						snapshot.Info.Turn = protocol.NewTurnID()
					case "unregistered command":
						snapshot.Info.ID = protocol.NewRequestID()
					case "inactive parent":
						run, err := ReadAgentRun(ctx, db, turn)
						if err != nil {
							return err
						}
						run.Status = protocol.ConversationStatusCompleted
						if err = putAgentRun(ctx, db, run); err != nil {
							return err
						}
					}
					if _, err := ConsumeCommandBackground(ctx, db, turn, snapshot, now); err == nil {
						return errors.New("unowned command completion was consumed")
					}
					record, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", "command:"+request.ID.String())
					if err != nil {
						return err
					}
					if record.Task.State != protocol.ConversationBackgroundStateRunning || record.Task.Revision != 1 {
						return errors.New("rejected collection changed command evidence")
					}
					return nil
				})
				if err != nil {
					t.Fatal(err)
				}
			})
		}
	})
	t.Run("collected command settles only unreserved stall evidence", func(t *testing.T) {
		for _, row := range []struct {
			name     string
			state    protocol.ConversationBackgroundState
			reserved bool
			want     protocol.ConversationBackgroundState
		}{
			{"undelivered", protocol.ConversationBackgroundStatePending, false, protocol.ConversationBackgroundStateDismissed},
			{"reserved", protocol.ConversationBackgroundStatePending, true, protocol.ConversationBackgroundStatePending},
			{"delivered", protocol.ConversationBackgroundStateDelivered, true, protocol.ConversationBackgroundStateDelivered},
		} {
			t.Run(row.name, func(t *testing.T) {
				t.Parallel()
				storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
				ctx := context.Background()
				err := storage.Node.Write(ctx, func(db *sql.Conn) error {
					input := protocol.DefaultCommandRunCommandPayload()
					input.Turn, input.Command, input.Background = turn, "observed command", true
					request := protocol.NewRequest(node, protocol.Command{Kind: "run_command", Data: &input})
					if _, err := RegisterCommand(ctx, db, request, now); err != nil {
						return err
					}
					stall, err := backgroundFixtureRecord(ctx, db, session, turn, "stall:"+request.ID.String(), true, now)
					if err != nil {
						return err
					}
					stall.Task.Kind = protocol.ConversationBackgroundKind{Kind: "command", Data: protocol.ConversationBackgroundKindCommandPayload{ID: request.ID}}
					stall.Task.State = row.state
					outcome := protocol.ConversationBackgroundOutcomeStalled
					stall.Task.Outcome = &outcome
					if row.reserved {
						stall.Task.Delivery = &turn
					}
					if err = saveBackgroundRecord(ctx, db, &stall); err != nil {
						return err
					}
					revision := stall.Task.Revision
					snapshot := protocol.ProcessSnapshot{Info: protocol.ProcessInfo{ID: request.ID, Session: session.ID, Turn: turn, Status: protocol.ProcessStatus{Kind: "finished", Data: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}}}
					if _, err = ConsumeCommandBackground(ctx, db, turn, snapshot, now); err != nil {
						return err
					}
					saved, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", stall.Task.ID)
					if err != nil {
						return err
					}
					if saved.Task.State != row.want || row.reserved && (saved.Task.Revision != revision || saved.Task.Delivery == nil || *saved.Task.Delivery != turn) {
						return errors.New("command collection changed the wrong stall evidence")
					}
					return nil
				})
				if err != nil {
					t.Fatal(err)
				}
			})
		}
	})
	t.Run("reservation exact evidence and projection", func(t *testing.T) {
		storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := context.Background()
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			if _, err := backgroundFixtureRecord(ctx, db, session, turn, "fixture-completion", true, now); err != nil {
				return err
			}
			run, text, _, err := BindBackground(ctx, db, turn)
			if err != nil {
				return err
			}
			if len(run.Notices) != 1 || text == nil || !strings.HasPrefix(*text, backgroundPrefix) {
				return errors.New("background evidence was not rendered")
			}
			record, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id='fixture-completion'")
			if err != nil {
				return err
			}
			if record.Task.State != protocol.ConversationBackgroundStatePending {
				return errors.New("reservation claimed delivery")
			}
			index := uint64(1)
			event := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Visible user input"), canonical.Text(1, *text)), now)
			event.Provenance.Author = "user"
			event.Metadata.Input = &canonical.Input{Turn: &turn, NoticePart: &index, NoticeIDs: []string{"wrong-id"}}
			if _, err = ConfirmBackground(ctx, db, session.ID, turn, event); err == nil {
				return errors.New("wrong notice identity was delivered")
			}
			event.Metadata.Input.NoticeIDs = []string{"fixture-completion"}
			event.Message().Content[1] = canonical.Text(1, "Forged notice text")
			if _, err = ConfirmBackground(ctx, db, session.ID, turn, event); err == nil {
				return errors.New("wrong notice text was delivered")
			}
			event.Message().Content[1] = canonical.Text(1, *text)
			entry, _, err := AppendCanonical(ctx, db, session.ID, event)
			if err != nil {
				return err
			}
			if len(entry.Parts) != 1 || entry.Parts[0].Kind != "text" || entry.Parts[0].Data != "Visible user input" {
				return errors.New("notice JSON leaked into displayed user input")
			}
			record, err = readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id='fixture-completion'")
			if err != nil {
				return err
			}
			if record.Task.State != protocol.ConversationBackgroundStateDelivered {
				return errors.New("canonical evidence did not confirm delivery")
			}
			revision := record.Task.Revision
			if _, _, err = AppendCanonical(ctx, db, session.ID, event); err != nil {
				return err
			}
			if _, err = ReleaseBackground(ctx, db, turn, nil, false); err != nil {
				return err
			}
			record, err = readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id='fixture-completion'")
			if err != nil {
				return err
			}
			if record.Task.Revision != revision || record.Task.State != protocol.ConversationBackgroundStateDelivered {
				return errors.New("idempotent append or release altered delivered evidence")
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
	t.Run("cancelled notice reservations remain explicit input", func(t *testing.T) {
		for _, boundary := range []string{"queued removal", "failed notice before canonical input", "superseded automatic input"} {
			t.Run(boundary, func(t *testing.T) {
				t.Parallel()
				storage, node, session, source, now := agentHistoryFixture(t, protocol.WorkModeCode)
				ctx := context.Background()
				err := storage.Node.Write(ctx, func(db *sql.Conn) error {
					if _, err := db.ExecContext(ctx, "DELETE FROM agent_pending WHERE turn=?1", source.String()); err != nil {
						return err
					}
					if _, _, err := FinishTurn(ctx, db, source, protocol.ConversationStatusCompleted, nil, now); err != nil {
						return err
					}
					if _, err := backgroundFixtureRecord(ctx, db, session, source, "released-evidence", true, now); err != nil {
						return err
					}
					notice, _, err := ScheduleBackground(ctx, db, node, session.ID, now)
					if err != nil {
						return err
					}
					if notice == nil {
						return errors.New("eligible completion was not reserved")
					}
					if boundary == "failed notice before canonical input" {
						invocation, _, err := ClaimNext(ctx, db, now)
						if err != nil {
							return err
						}
						if invocation == nil || invocation.Run.Turn != notice.ID {
							return errors.New("reserved notice was not claimed")
						}
						if _, _, err = FinishTurn(ctx, db, notice.ID, protocol.ConversationStatusFailed, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Fixture failure before canonical input"}, now); err != nil {
							return err
						}
						record, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id='released-evidence'")
						if err != nil {
							return err
						}
						if record.Task.State != protocol.ConversationBackgroundStatePending || record.Task.Delivery != nil || !record.Task.Suppressed {
							return errors.New("failed notice settlement did not release and suppress its evidence")
						}
					}
					if boundary == "superseded automatic input" {
						run, err := ReadAgentRun(ctx, db, notice.ID)
						if err != nil {
							return err
						}
						if _, err = cancelAutomaticInput(ctx, db, run, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "Fixture superseded input"}, now); err != nil {
							return err
						}
					} else {
						queue, err := ReadQueue(ctx, db, session.ID)
						if err != nil {
							return err
						}
						if len(queue.Items) != 1 || queue.Items[0].Turn != notice.ID || !queue.Items[0].Notice {
							return errors.New("unconfirmed notice input was not retained for removal")
						}
						request := protocol.NewRequest(node, protocol.Command{Kind: "remove_queued_turn", Data: &protocol.CommandRemoveQueuedTurnPayload{Turn: notice.ID, ExpectedRevision: queue.Items[0].Revision}})
						if _, _, err = ExecuteAgentCommand(ctx, db, node, storage.Profile, node, request, now); err != nil {
							return err
						}
					}
					var retained bool
					if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_pending WHERE turn=?1)", notice.ID.String()).Scan(&retained); err != nil {
						return err
					}
					if retained {
						return errors.New("removed notice retained its queued input")
					}
					record, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id='released-evidence'")
					if err != nil {
						return err
					}
					if record.Task.State != protocol.ConversationBackgroundStatePending || record.Task.Delivery != nil || !record.Task.Suppressed {
						return errors.New("cancelled notice consumed or stranded its completion evidence")
					}
					if err = setQueuePaused(ctx, db, session.ID, false); err != nil {
						return err
					}
					if scheduled, _, err := ScheduleBackground(ctx, db, node, session.ID, now); err != nil || scheduled != nil {
						return fmt.Errorf("suppressed completion scheduled automatic input: %v", err)
					}
					explicit, err := delegationFixtureParent(ctx, db, node, session, now)
					if err != nil {
						return err
					}
					run, text, _, err := BindBackground(ctx, db, explicit)
					if err != nil {
						return err
					}
					if len(run.Notices) != 1 || run.Notices[0].ID != "released-evidence" || text == nil {
						return errors.New("suppressed completion was unavailable to explicit user input")
					}
					if _, err = ReleaseBackground(ctx, db, explicit, nil, false); err != nil {
						return err
					}
					record, err = readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id='released-evidence'")
					if err != nil {
						return err
					}
					if !record.Task.Suppressed || record.Task.Delivery != nil || record.Task.State != protocol.ConversationBackgroundStatePending {
						return errors.New("ordinary release cleared prior suppression or completion evidence")
					}
					return nil
				})
				if err != nil {
					t.Fatal(err)
				}
			})
		}
	})
	t.Run("notice bounds and reservation release", func(t *testing.T) {
		storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := context.Background()
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			for index := 0; index < 17; index++ {
				if _, err := backgroundFixtureRecord(ctx, db, session, turn, fmt.Sprintf("notice-%02d", index), false, now); err != nil {
					return err
				}
			}
			run, text, _, err := BindBackground(ctx, db, turn)
			if err != nil {
				return err
			}
			if len(run.Notices) != 16 || text == nil || len(*text) > 32*1024 || run.Notices[0].ID != "notice-00" {
				return errors.New("notice bounds or sequence changed")
			}
			if _, err = ReleaseBackground(ctx, db, turn, nil, false); err != nil {
				return err
			}
			var pending int
			if err = db.QueryRowContext(ctx, "SELECT count(*) FROM agent_background WHERE state='pending' AND delivery IS NULL").Scan(&pending); err != nil {
				return err
			}
			if pending != 17 {
				return errors.New("release consumed undelivered evidence")
			}
			var suppressed bool
			if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_background WHERE json_extract(body,'$.task.suppressed')=1)").Scan(&suppressed); err != nil {
				return err
			}
			if suppressed {
				return errors.New("ordinary reservation release suppressed completion evidence")
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
}

func TestBackgroundSchedulingAndContinuationFence(t *testing.T) {
	t.Parallel()
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := context.Background()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := db.ExecContext(ctx, "DELETE FROM agent_pending WHERE turn=?1", turn.String()); err != nil {
			return err
		}
		if _, err := backgroundFixtureRecord(ctx, db, session, turn, "no-opt-in", false, now); err != nil {
			return err
		}
		if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		if next, _, err := ScheduleBackground(ctx, db, node, session.ID, now); err != nil || next != nil {
			return fmt.Errorf("non-opted command scheduled automatic input: %v", err)
		}
		if _, err := backgroundFixtureRecord(ctx, db, session, turn, "opt-in", true, now); err != nil {
			return err
		}
		// A rejected optional admission must preserve the real completion and counter state.
		original, err := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE id=?1", session.Config.Provider.String())
		if err != nil {
			return err
		}
		body, err := encode(original)
		if err != nil {
			return err
		}
		original.Provider.Enabled = false
		disabled, err := encode(original)
		if err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "UPDATE providers SET body=?2 WHERE id=?1", session.Config.Provider.String(), disabled); err != nil {
			return err
		}
		var before, after int
		if err = db.QueryRowContext(ctx, "SELECT count(*) FROM turns").Scan(&before); err != nil {
			return err
		}
		if next, _, err := ScheduleBackground(ctx, db, node, session.ID, now); err != nil || next != nil {
			return fmt.Errorf("optional failure escaped savepoint: %v", err)
		}
		if err = db.QueryRowContext(ctx, "SELECT count(*) FROM turns").Scan(&after); err != nil {
			return err
		}
		run, err := ReadAgentRun(ctx, db, turn)
		if err != nil {
			return err
		}
		if before != after || run.Status != protocol.ConversationStatusCompleted {
			return errors.New("optional admission rolled back the true completion")
		}
		if _, err = db.ExecContext(ctx, "UPDATE providers SET body=?2 WHERE id=?1", session.Config.Provider.String(), body); err != nil {
			return err
		}
		queued, _, err := ScheduleBackground(ctx, db, node, session.ID, now)
		if err != nil {
			return err
		}
		if queued == nil {
			return errors.New("opted completion did not use the ordinary queue")
		}
		next, _, err := ClaimNext(ctx, db, now)
		if err != nil {
			return err
		}
		if next == nil || !next.Run.Notice || len(next.Run.Notices) != 1 || next.Run.Notices[0].ID != "opt-in" {
			return errors.New("automatic claim did not retain its exact eligible notice")
		}
		request := protocol.NewRequest(node, protocol.Command{Kind: "continue_turn", Data: &protocol.CommandContinueTurnPayload{Session: session.ID, Message: protocol.ConversationInput{Text: "Forged continuation"}, Key: "checkpoint", ExpectedRevision: 1, Scope: protocol.PluginStorageScopeNode}})
		if _, _, err = ExecuteBackgroundCommand(ctx, db, node, node, request, now); err == nil {
			return errors.New("continuation omitted provenance")
		}
		context := protocol.DefaultPluginContext()
		context.Package.Name = "forged"
		request.Plugin = &context
		if _, _, err = ExecuteBackgroundCommand(ctx, db, node, node, request, now); err == nil {
			return errors.New("wire provenance granted package authority")
		}
		fence := continuationFence{Package: context.Package, Key: "checkpoint", Revision: 1, Scope: protocol.PluginStorageScopeNode}
		if valid, err := validContinuation(ctx, db, fence, session.ID); err != nil || valid {
			return fmt.Errorf("unavailable package passed continuation fence: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
