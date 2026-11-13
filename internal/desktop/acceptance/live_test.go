package acceptance

import (
	"context"
	"encoding/json"
	"errors"
	"slices"
	"strings"
	"testing"

	core "github.com/sailry/sailry-harness/internal/acceptance"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Encoding both sides keeps this fixture subject to the control socket's
// declared union payload types, even though it never opens a Node or a model.
type liveFixtureTransport struct {
	dispatch func(context.Context, protocol.Request) (protocol.Output, error)
}

func (*liveFixtureTransport) Target() protocol.NodeID { return protocol.NodeID{1} }
func (*liveFixtureTransport) Open(context.Context, protocol.StreamID) (link.Stream, error) {
	return nil, errors.New("fixture has no binary streams")
}
func (*liveFixtureTransport) Subscribe(context.Context, protocol.Topic) (link.Subscription, error) {
	return nil, errors.New("fixture has no subscriptions")
}
func (*liveFixtureTransport) WatchConnection(ctx context.Context, _ func(link.ConnectionInfo)) error {
	<-ctx.Done()
	return ctx.Err()
}
func (transport *liveFixtureTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	encoded, err := json.Marshal(request)
	if err != nil {
		return link.Admission{}, err
	}
	var received protocol.Request
	if err := json.Unmarshal(encoded, &received); err != nil {
		return link.Admission{}, err
	}
	output, err := transport.dispatch(ctx, received)
	if err != nil {
		return link.Admission{}, err
	}
	encoded, err = json.Marshal(output)
	if err != nil {
		return link.Admission{}, err
	}
	if err := json.Unmarshal(encoded, &output); err != nil {
		return link.Admission{}, err
	}
	completed := make(chan link.Response, 1)
	completed <- link.Response{Output: output}
	close(completed)
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completed}, nil
}

func TestLiveSessionSetupPreservesTypedRequestsAndOwner(t *testing.T) {
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = protocol.ProviderID{1}, "fixture-model"
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	config.Effort = protocol.Effort{Kind: "medium"}
	session := protocol.DefaultSession()
	session.ID, session.Worktree, session.Revision, session.Config = protocol.SessionID{2}, protocol.WorktreeID{3}, 1, config
	for _, malformed := range []bool{false, true} {
		t.Run(map[bool]string{false: "complete", true: "invalid_update"}[malformed], func(t *testing.T) {
			current := session
			var commands []string
			var progress []core.CaseResult
			transport := &liveFixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (protocol.Output, error) {
				commands = append(commands, request.Command.Kind)
				switch request.Command.Kind {
				case "create_session":
					input := request.Command.Data.(*protocol.CommandCreateSessionPayload)
					if input.Config == nil || input.Project != nil || input.Config.Model != config.Model {
						t.Fatal("setup changed its isolated model configuration")
					}
				case "set_session_team":
					input := request.Command.Data.(*protocol.CommandSetSessionExecutionPayload)
					if input.Session != current.ID || input.ExpectedRevision != 1 || input.Execution != protocol.ConversationExecutionModel {
						t.Fatal("setup changed its session, revision or execution mode")
					}
					if len(progress) != 1 || progress[0].Status != "running" || progress[0].Session != current.ID.String() {
						t.Fatal("owned session was not emitted before the next mutation")
					}
					if malformed {
						return protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{}}, nil
					}
					current.Revision, current.Roles.Execution = 2, protocol.ConversationExecutionModel
				case "rename_session":
					input := request.Command.Data.(*protocol.CommandRenameSessionPayload)
					if input.Session != current.ID || input.ExpectedRevision != 2 || input.Title != "Acceptance E2E99" {
						t.Fatal("setup lost the typed title update or its latest revision")
					}
					current.Revision, current.Activity.Title = 3, input.Title
				default:
					t.Fatalf("setup dispatched unexpected command %q", request.Command.Kind)
				}
				return protocol.Output{Kind: "session", Data: current}, nil
			}}
			controller := client.New(transport)
			t.Cleanup(func() { _ = controller.Close() })
			result := core.CaseResult{ID: "E2E99"}
			w := &liveWindow{window: &window{ctx: t.Context(), result: &result}, controller: controller, config: config,
				progress: func(value core.CaseResult) { progress = append(progress, value) }}
			err := w.newSession()
			if w.session.ID != session.ID || result.Session != session.ID.String() || !w.owned {
				t.Fatal("setup discarded the session identity needed by cleanup")
			}
			if malformed {
				var issue *liveIssue
				if !errors.As(err, &issue) || issue.status != "failed" || !strings.Contains(issue.detail, "expected session") || w.session.Revision != 1 || len(commands) != 2 {
					t.Fatalf("malformed setup response lost its safe detail or owner: %v", err)
				}
			} else if err != nil || w.session.Revision != 3 || !slices.Equal(commands, []string{"create_session", "set_session_team", "rename_session"}) {
				t.Fatalf("typed setup = %v, revision %d, commands %v", err, w.session.Revision, commands)
			}
			if len(progress[0].Evidence) != 1 {
				t.Fatal("later mutations changed an already emitted progress value")
			}
		})
	}
}

func TestLiveCommandInspectsUncertainAdmissionWithoutReplay(t *testing.T) {
	for _, resolved := range []bool{false, true} {
		t.Run(map[bool]string{false: "unconfirmed", true: "completed"}[resolved], func(t *testing.T) {
			var original protocol.RequestID
			mutations, inspections := 0, 0
			transport := &liveFixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (protocol.Output, error) {
				if request.Command.Kind == "stop_commands" {
					mutations++
					original = request.ID
					return protocol.Output{}, &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "PRIVATE-RESPONSE"}}
				}
				if request.Command.Kind != "inspect_request" {
					t.Fatalf("uncertain request dispatched %q", request.Command.Kind)
				}
				inspections++
				input := request.Command.Data.(*protocol.CommandInspectRequestPayload)
				if input.ID != original || len(input.Digest) != 64 {
					t.Fatal("inspection changed the captured request identity")
				}
				outcome := protocol.RequestOutcome{Kind: "admitted"}
				if resolved {
					output := protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{}}
					outcome = protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &output}}
				}
				return protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: original, Outcome: outcome}}, nil
			}}
			controller := client.New(transport)
			t.Cleanup(func() { _ = controller.Close() })
			result := core.CaseResult{}
			w := &liveWindow{window: &window{ctx: t.Context(), result: &result}, controller: controller}
			output, err := w.command(t.Context(), "stop_commands", protocol.CommandStopCommandsPayload{Session: protocol.SessionID{2}})
			if resolved {
				if err != nil || output.Kind != "commands" || w.unconfirmed {
					t.Fatalf("stored completion was not recovered: %v", err)
				}
			} else {
				var issue *liveIssue
				if !errors.As(err, &issue) || issue.status != "blocked" || !w.unconfirmed {
					t.Fatalf("uncertain admission did not block more cases: %v", err)
				}
			}
			if mutations != 1 || inspections != 1 || len(result.Evidence) != 1 || !strings.Contains(result.Evidence[0].Value, original.String()) {
				t.Fatalf("uncertain admission was replayed or lost its evidence: mutations=%d inspections=%d", mutations, inspections)
			}
		})
	}
}

func TestLiveErrorsRetainAssertionsAndRedactRemoteBodies(t *testing.T) {
	const detail = "The fixture session did not preserve its model"
	if got := liveError(liveFail(detail)); got != detail {
		t.Fatalf("safe assertion detail was lost: %q", got)
	}
	for _, err := range []error{
		protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "PRIVATE-RESPONSE"},
		&protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "PRIVATE-RESPONSE"},
		&link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "PRIVATE-RESPONSE"}},
	} {
		if got := liveError(err); got != "Node request failed: unavailable" {
			t.Fatalf("fault code was lost or its private response escaped: %q", got)
		}
	}
}

func TestLiveTurnCaptureRequiresSelectedEffort(t *testing.T) {
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = protocol.ProviderID{1}, "fixture-model"
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	config.Effort = protocol.Effort{Kind: "medium"}
	for _, effort := range []string{"medium", "low"} {
		t.Run(effort, func(t *testing.T) {
			turn := protocol.DefaultQueuedTurn()
			turn.ID, turn.Session, turn.Request, turn.Kind = protocol.TurnID{4}, protocol.SessionID{2}, protocol.RequestID{5}, protocol.ConversationRunKindTask
			turn.Config, turn.Roles.Execution = config, protocol.ConversationExecutionModel
			turn.Config.Effort = protocol.Effort{Kind: effort}
			transport := &liveFixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (protocol.Output, error) {
				if request.Command.Kind != "read_turn_capture" {
					t.Fatalf("capture dispatched %q", request.Command.Kind)
				}
				input := request.Command.Data.(*protocol.CommandReadTurnCapturePayload)
				if input.Session != turn.Session || input.Turn != turn.ID {
					t.Fatal("capture read outside the UI-sent acceptance turn")
				}
				return protocol.Output{Kind: "queued_turn", Data: turn}, nil
			}}
			controller := client.New(transport)
			t.Cleanup(func() { _ = controller.Close() })
			result := core.CaseResult{}
			w := &liveWindow{window: &window{ctx: t.Context(), result: &result}, controller: controller, config: config, turn: turn.ID,
				session: protocol.Session{ID: turn.Session}}
			err := w.capture()
			if effort == "low" {
				if err == nil || !strings.Contains(liveError(err), "effort") {
					t.Fatalf("capture accepted a different effort: %v", err)
				}
				return
			}
			if err != nil || len(result.Evidence) != 2 || !strings.Contains(result.Evidence[0].Value, "effort medium") {
				t.Fatalf("selected effort lost its real capture evidence: %v, %#v", err, result.Evidence)
			}
		})
	}
}
