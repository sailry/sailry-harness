package acceptance

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestFixtureSessionCommandsUseProtocolPayloads(t *testing.T) {
	for _, invalid := range []string{"", "set_session_team", "rename_session", "read_session"} {
		t.Run("invalid_"+invalid, func(t *testing.T) {
			config := protocol.DefaultSessionConfig()
			config.Provider, config.Model = protocol.ProviderID{1}, "fixture"
			config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
			session := protocol.DefaultSession()
			session.ID, session.Worktree, session.Revision, session.Config = protocol.SessionID{1}, protocol.WorktreeID{1}, 1, config
			listed := false
			transport := &fixtureTransport{}
			transport.dispatch = func(_ context.Context, request protocol.Request) (link.Admission, error) {
				switch request.Command.Kind {
				case "create_session":
					if _, ok := request.Command.Data.(*protocol.CommandCreateSessionPayload); !ok {
						t.Fatal("create session did not cross the protocol decoder")
					}
				case "set_session_team":
					input, ok := request.Command.Data.(*protocol.CommandSetSessionExecutionPayload)
					if !ok || input.Session != session.ID || input.ExpectedRevision != session.Revision || input.Execution != protocol.ConversationExecutionModel {
						t.Fatal("invalid typed execution payload")
					}
					session.Roles.Execution = input.Execution
					session.Revision++
				case "rename_session":
					input, ok := request.Command.Data.(*protocol.CommandRenameSessionPayload)
					if !ok || input.Session != session.ID || input.ExpectedRevision != session.Revision || input.Title != "Acceptance fixture" {
						t.Fatal("invalid typed rename payload")
					}
					session.Revision++
				case "read_session":
					input, ok := request.Command.Data.(*protocol.CommandReadSessionPayload)
					if !ok || input.Session != session.ID {
						t.Fatal("invalid typed session read payload")
					}
				case "list_commands":
					input, ok := request.Command.Data.(*protocol.CommandListCommandsPayload)
					if !ok || input.Session != session.ID {
						t.Fatal("invalid typed process list payload")
					}
					listed = true
					return fixtureResponse(request, protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{}}), nil
				default:
					t.Fatalf("unexpected fixture command %q", request.Command.Kind)
				}
				if request.Command.Kind == invalid {
					return fixtureResponse(request, protocol.Output{Kind: "session", Data: nil}), nil
				}
				return fixtureResponse(request, protocol.Output{Kind: "session", Data: session}), nil
			}
			controller := client.New(transport)
			t.Cleanup(func() { _ = controller.Close() })
			run := &caseRun{ctx: t.Context(), controller: controller, config: config, result: &CaseResult{ID: "fixture"}, wake: make(chan struct{}, 1)}
			err := run.standard()
			if err == nil {
				err = run.refreshSession()
			}
			if (err != nil) != (invalid != "") {
				t.Fatalf("fixture setup error = %v, invalid command %q", err, invalid)
			}
			if run.session.ID != session.ID || len(run.owned) != 1 || run.owned[0].id != session.ID || run.result.Session != session.ID.String() {
				t.Fatal("a malformed session update lost the owned session identity")
			}
			if _, err := run.processes(); err != nil || !listed {
				t.Fatalf("typed process list failed: %v", err)
			}
			other := session
			other.ID = protocol.SessionID{2}
			if err := run.updateSession(protocol.Output{Kind: "session", Data: other}); err == nil || run.session.ID != session.ID {
				t.Fatal("another session replaced the owned fixture")
			}
		})
	}
}

func TestCapturePreservesSelectedEffort(t *testing.T) {
	for _, test := range []struct {
		name                         string
		selected, admitted, captured protocol.Effort
		valid                        bool
	}{
		{"medium", protocol.Effort{Kind: "medium"}, protocol.Effort{Kind: "medium"}, protocol.Effort{Kind: "medium"}, true},
		{"captured_changed", protocol.Effort{Kind: "medium"}, protocol.Effort{Kind: "medium"}, protocol.Effort{Kind: "high"}, false},
		{"admitted_changed", protocol.Effort{Kind: "medium"}, protocol.Effort{Kind: "low"}, protocol.Effort{Kind: "medium"}, false},
		{"budget", protocol.Effort{Kind: "budget", Data: int32(512)}, protocol.Effort{Kind: "budget", Data: int32(512)}, protocol.Effort{Kind: "budget", Data: int32(512)}, true},
		{"budget_changed", protocol.Effort{Kind: "budget", Data: int32(512)}, protocol.Effort{Kind: "budget", Data: int32(512)}, protocol.Effort{Kind: "budget", Data: int32(1024)}, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			config := protocol.DefaultSessionConfig()
			config.Provider, config.Model, config.Effort = protocol.ProviderID{1}, "fixture", test.selected
			config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
			turn := protocol.DefaultQueuedTurn()
			turn.ID, turn.Session, turn.Request = protocol.TurnID{1}, protocol.SessionID{1}, protocol.RequestID{1}
			turn.Kind, turn.Config, turn.Roles.Execution = protocol.ConversationRunKindTask, config, protocol.ConversationExecutionModel
			turn.Config.Effort = test.admitted
			capture := turn
			capture.Config.Effort = test.captured
			transport := &fixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (link.Admission, error) {
				input, ok := request.Command.Data.(*protocol.CommandReadTurnCapturePayload)
				if request.Command.Kind != "read_turn_capture" || !ok || input.Session != turn.Session || input.Turn != turn.ID {
					t.Fatal("capture read escaped its admitted turn")
				}
				return fixtureResponse(request, protocol.Output{Kind: "queued_turn", Data: capture}), nil
			}}
			controller := client.New(transport)
			t.Cleanup(func() { _ = controller.Close() })
			run := &caseRun{ctx: t.Context(), controller: controller, config: config, session: protocol.Session{ID: turn.Session}, result: &CaseResult{}}
			if err := run.assertCapture(turn); (err == nil) != test.valid {
				t.Fatalf("capture validity = %v, want %v", err, test.valid)
			}
			if test.valid && (len(run.result.Evidence) != 2 || run.result.Evidence[1].Kind != "capture_effort" || !strings.Contains(run.result.Evidence[1].Value, "kind="+test.selected.Kind)) {
				t.Fatal("selected effort has no explicit capture evidence")
			}
		})
	}
}

func TestMissingRequiredPackages(t *testing.T) {
	for _, test := range []struct {
		name             string
		present, enabled bool
		compatibility    string
		want             string
	}{
		{"missing", false, false, "compatible", "files"},
		{"disabled", true, false, "compatible", "files"},
		{"incompatible", true, true, "requires_newer", "files"},
		{"available", true, true, "compatible", ""},
	} {
		t.Run(test.name, func(t *testing.T) {
			snapshot := protocol.DefaultSnapshot()
			if test.present {
				installed := protocol.DefaultPluginSummary()
				installed.Name, installed.Enabled, installed.Compatibility.Kind = "files", test.enabled, test.compatibility
				snapshot.Plugins = []protocol.PluginSummary{installed}
			}
			if got := missingPackage(snapshot, []string{"files"}); got != test.want {
				t.Fatalf("missing package = %q, want %q", got, test.want)
			}
		})
	}
}

func TestUnknownAdmissionStopsLaterCasesWithoutOwnedSession(t *testing.T) {
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Enabled, provider.DefaultModel = protocol.ProviderID{1}, true, "fixture"
	model := protocol.DefaultConversationModel()
	model.ID, model.Reasoning, model.DefaultEffort = "fixture", true, protocol.Effort{Kind: "medium"}
	model.Efforts = []protocol.Effort{{Kind: "medium"}}
	provider.Models = []protocol.ConversationModel{model}
	snapshot.Providers = []protocol.ConversationProvider{provider}
	subscription := &fixtureSnapshotSubscription{initial: &snapshot}
	creates, inspections := 0, 0
	var original protocol.RequestID
	transport := &fixtureTransport{}
	transport.subscribe = func(_ context.Context, topic protocol.Topic) (link.Subscription, error) {
		if topic.Kind != "node" {
			return nil, errors.New("fixture has no conversation")
		}
		return subscription, nil
	}
	transport.dispatch = func(_ context.Context, request protocol.Request) (link.Admission, error) {
		switch request.Command.Kind {
		case "create_session":
			creates++
			original = request.ID
			done := make(chan link.Response)
			close(done)
			return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: true}, Completion: done}, nil
		case "inspect_request":
			inspections++
			input, ok := request.Command.Data.(*protocol.CommandInspectRequestPayload)
			if !ok || input.ID != original {
				t.Fatal("inspection lost the original admission identity")
			}
			output := protocol.OutputRequestOutcomePayload{ID: original, Outcome: protocol.RequestOutcome{Kind: "admitted"}}
			return fixtureResponse(request, protocol.Output{Kind: "request_outcome", Data: output}), nil
		default:
			t.Fatalf("another request escaped the unconfirmed creation: %q", request.Command.Kind)
			return link.Admission{}, nil
		}
	}
	controller := client.New(transport)
	t.Cleanup(func() { _ = controller.Close() })
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	report, err := RunBackend(t.Context(), controller, Options{Suite: "quick", Cases: []string{"A01", "A03"}}, nil)
	assertBlocked(t, err)
	if creates != 1 || inspections != 1 || len(report.Cases) != 2 || !report.Cases[0].Uncertain || report.Cases[0].Status != "blocked" || report.Cases[1].Status != "blocked" || report.Cases[1].Session != "" || report.OK() {
		t.Fatalf("unconfirmed creation continued model work: creates=%d inspections=%d cases=%#v", creates, inspections, report.Cases)
	}
	if report.Effort == nil || report.Effort.Kind != "medium" {
		t.Fatal("report lost its selected reasoning effort")
	}
}

type fixtureSnapshotSubscription struct {
	initial *protocol.Snapshot
}

func (subscription *fixtureSnapshotSubscription) Next(ctx context.Context) (protocol.Update, error) {
	if subscription.initial != nil {
		snapshot := *subscription.initial
		subscription.initial = nil
		return protocol.Update{Kind: "snapshot", Data: snapshot}, nil
	}
	<-ctx.Done()
	return protocol.Update{}, ctx.Err()
}

func (*fixtureSnapshotSubscription) Close() error { return nil }

func assertBlocked(t *testing.T, err error) {
	var issue *caseIssue
	if !errors.As(err, &issue) || issue.status != "blocked" {
		t.Fatalf("expected blocked outcome, got %v", err)
	}
}
