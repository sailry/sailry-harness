package acceptance

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"testing"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestBackendCaseSelection(t *testing.T) {
	full, err := ListBackendCases("full")
	if err != nil || len(full) < 20 {
		t.Fatalf("full suite has %d cases: %v", len(full), err)
	}
	seen := map[string]bool{}
	for _, result := range full {
		if result.ID == "" || seen[result.ID] || result.Group != "backend" || result.Title == "" || result.Status != "" {
			t.Fatalf("invalid case description: %#v", result)
		}
		seen[result.ID] = true
	}
	quick, err := ListBackendCases("")
	if err != nil || len(quick) == 0 || len(quick) >= len(full) {
		t.Fatalf("quick suite has %d cases: %v", len(quick), err)
	}
	selected, err := selectCases("quick", []string{"P02"})
	if err != nil || len(selected) != 1 || selected[0].id != "P02" {
		t.Fatalf("explicit case selection = %#v, %v", selected, err)
	}
	if _, err := selectCases("full", []string{"not-a-case"}); err == nil {
		t.Fatal("unknown case was accepted")
	}
	if _, err := ListBackendCases("automatic"); err == nil {
		t.Fatal("unknown suite was accepted")
	}
}

func TestBackendConfigSelection(t *testing.T) {
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Enabled, provider.DefaultModel = protocol.ProviderID{1}, true, "fixture"
	provider.Models = []protocol.ConversationModel{{ID: "fixture"}}
	provider.Credential = &protocol.CredentialRef{Node: protocol.NodeID{1}, ID: protocol.CredentialID{1}}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Providers = []protocol.ConversationProvider{provider}
	config, err := selectConfig(snapshot, nil)
	if err != nil || config.Provider != provider.ID || config.Model != "fixture" || config.Credential == nil || *config.Credential != *provider.Credential {
		t.Fatalf("unique configured model = %#v, %v", config, err)
	}
	requested := protocol.SessionConfig{Provider: provider.ID, Model: "fixture", Assistant: &protocol.PluginConversationBinding{}, Resource: &protocol.ConnectionResource{}, Features: map[string]string{"fixture": "original"}}
	config, err = selectConfig(snapshot, &requested)
	if err != nil || config.Mode != protocol.WorkModeCode || config.Permission != protocol.PermissionProject || config.Assistant != nil || config.Resource != nil {
		t.Fatalf("scoped model configuration = %#v, %v", config, err)
	}
	config.Features["fixture"] = "changed"
	if requested.Features["fixture"] != "original" || requested.Assistant == nil || requested.Resource == nil || requested.Mode != "" {
		t.Fatal("acceptance changed the caller's configuration")
	}
	second := provider
	second.ID = protocol.ProviderID{2}
	snapshot.Providers = append(snapshot.Providers, second)
	if _, err := selectConfig(snapshot, nil); err == nil {
		t.Fatal("ambiguous providers were selected silently")
	}
	snapshot.Defaults.Config = &requested
	if _, err := selectConfig(snapshot, nil); err != nil {
		t.Fatalf("valid defaults were not selected: %v", err)
	}
	snapshot.Providers[0].Enabled = false
	if _, err := selectConfig(snapshot, &requested); err == nil {
		t.Fatal("disabled requested provider was accepted")
	}
}

func TestReportOKRequiresCompletedEvidence(t *testing.T) {
	for _, test := range []struct {
		statuses []string
		want     bool
	}{
		{nil, false},
		{[]string{"skipped"}, false},
		{[]string{"passed", "skipped"}, true},
		{[]string{"passed", "blocked"}, false},
		{[]string{"passed", "failed"}, false},
		{[]string{"passed", "canceled"}, false},
		{[]string{"passed", "unknown"}, false},
	} {
		report := Report{}
		for _, status := range test.statuses {
			report.Cases = append(report.Cases, CaseResult{Status: status})
		}
		if got := report.OK(); got != test.want {
			t.Fatalf("OK(%v) = %v", test.statuses, got)
		}
	}
}

func TestBackendConfigUsesAdvertisedReasoningDefault(t *testing.T) {
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Enabled, provider.DefaultModel = protocol.ProviderID{1}, true, "reasoner"
	model := protocol.DefaultConversationModel()
	model.ID, model.Reasoning = "reasoner", true
	model.Efforts = []protocol.Effort{{Kind: "low"}, {Kind: "medium"}, {Kind: "high"}}
	model.DefaultEffort = protocol.Effort{Kind: "medium"}
	provider.Models = []protocol.ConversationModel{model}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Providers = []protocol.ConversationProvider{provider}
	config, err := selectConfig(snapshot, nil)
	if err != nil || config.Effort.Kind != "medium" {
		t.Fatalf("unique reasoning model = effort %q, %v", config.Effort.Kind, err)
	}
	requested := protocol.DefaultSessionConfig()
	requested.Provider, requested.Model = provider.ID, model.ID
	config, err = selectConfig(snapshot, &requested)
	if err != nil || config.Effort.Kind != "medium" || requested.Effort.Kind != "default" {
		t.Fatalf("provider/model-only request = effort %q, %v", config.Effort.Kind, err)
	}
	requested.Effort.Kind = "high"
	snapshot.Defaults.Config = &requested
	config, err = selectConfig(snapshot, nil)
	if err != nil || config.Effort.Kind != "high" {
		t.Fatalf("explicit saved effort was replaced: %q, %v", config.Effort.Kind, err)
	}
	requested.Effort.Kind = "default"
	snapshot.Providers[0].Models[0].Efforts = append(model.Efforts, protocol.Effort{Kind: "default"})
	config, err = selectConfig(snapshot, &requested)
	if err != nil || config.Effort.Kind != "default" {
		t.Fatalf("advertised generic default was replaced: %q, %v", config.Effort.Kind, err)
	}
}

func TestAdmissionInspectionDoesNotReplay(t *testing.T) {
	for _, completed := range []bool{false, true} {
		t.Run(fmt.Sprint(completed), func(t *testing.T) {
			var original protocol.RequestID
			mutations, inspections := 0, 0
			transport := &fixtureTransport{}
			transport.dispatch = func(_ context.Context, request protocol.Request) (link.Admission, error) {
				if request.Command.Kind == "stop_commands" {
					mutations++
					original = request.ID
					done := make(chan link.Response)
					close(done)
					return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: true}, Completion: done}, nil
				}
				if request.Command.Kind != "inspect_request" {
					t.Fatalf("unexpected request %q", request.Command.Kind)
				}
				inspections++
				query, err := decode[protocol.CommandInspectRequestPayload](request.Command.Data)
				if err != nil || query.ID != original || len(query.Digest) != 64 {
					t.Fatalf("inspection changed the original request identity: %#v, %v", query, err)
				}
				outcome := protocol.RequestOutcome{Kind: "admitted"}
				if completed {
					stored := protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{}}
					outcome = protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &stored}}
				}
				return fixtureResponse(request, protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: original, Outcome: outcome}}), nil
			}
			controller := client.New(transport)
			t.Cleanup(func() { _ = controller.Close() })
			run := &caseRun{ctx: t.Context(), controller: controller, result: &CaseResult{}}
			output, err := run.command(t.Context(), "stop_commands", protocol.CommandStopCommandsPayload{Session: protocol.SessionID{1}})
			if completed {
				if err != nil || output.Kind != "commands" || run.unconfirmed != nil {
					t.Fatalf("recorded completion = %#v, %v", output, err)
				}
			} else {
				var issue *caseIssue
				if !errors.As(err, &issue) || issue.status != "blocked" {
					t.Fatalf("unconfirmed admission = %v", err)
				}
				if run.unconfirmed == nil || run.unconfirmed.ID != original || run.unconfirmed.Command.Kind != "stop_commands" {
					t.Fatal("unconfirmed admission lost its captured request")
				}
			}
			if mutations != 1 || inspections != 1 || len(run.result.Evidence) != 1 || !strings.Contains(run.result.Evidence[0].Value, original.String()) {
				t.Fatalf("mutation=%d inspection=%d evidence=%#v", mutations, inspections, run.result.Evidence)
			}
		})
	}
}

func TestCleanupOwnsOnlyItsSessionAndVerifiesProcesses(t *testing.T) {
	session, turn := protocol.SessionID{1}, protocol.TurnID{1}
	stoppedTurn, stoppedCommands := false, 0
	transport := &fixtureTransport{}
	transport.dispatch = func(ctx context.Context, request protocol.Request) (link.Admission, error) {
		if ctx.Err() != nil {
			t.Fatal("cleanup reused its canceled observation context")
		}
		var output protocol.Output
		switch request.Command.Kind {
		case "stop_commands":
			input, err := decode[protocol.CommandStopCommandsPayload](request.Command.Data)
			if err != nil || input.Session != session {
				t.Fatalf("cleanup escaped its session: %#v, %v", input, err)
			}
			stoppedCommands++
			output = protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{}}
		case "read_conversation":
			input, err := decode[protocol.CommandReadConversationPayload](request.Command.Data)
			if err != nil || input.Session != session {
				t.Fatalf("cleanup read another session: %#v, %v", input, err)
			}
			page := protocol.DefaultConversationPage()
			page.Session = session
			run := protocol.DefaultConversationRun()
			run.Session, run.Turn, run.Kind, run.Status = session, turn, protocol.ConversationRunKindTask, protocol.ConversationStatusRunning
			run.Worktree = protocol.WorktreeID{1}
			if stoppedTurn {
				run.Status = protocol.ConversationStatusCancelled
			}
			page.Runs = []protocol.ConversationRun{run}
			output = protocol.Output{Kind: "conversation", Data: protocol.ConversationHistory{Page: page}}
		case "stop_turn":
			input, err := decode[protocol.CommandStopTurnPayload](request.Command.Data)
			if err != nil || input.Turn != turn {
				t.Fatalf("cleanup stopped another turn: %#v, %v", input, err)
			}
			stoppedTurn = true
			output = protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{}}
		case "list_commands":
			input, err := decode[protocol.CommandListCommandsPayload](request.Command.Data)
			if err != nil || input.Session != session || stoppedCommands != 2 || !stoppedTurn {
				t.Fatalf("process verification preceded final turn/process stops: %#v, %v", input, err)
			}
			output = protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{}}
		default:
			t.Fatalf("unexpected cleanup request %q", request.Command.Kind)
		}
		return fixtureResponse(request, output), nil
	}
	controller := client.New(transport)
	t.Cleanup(func() { _ = controller.Close() })
	canceled, cancel := context.WithCancel(t.Context())
	cancel()
	run := &caseRun{ctx: canceled, controller: controller, owned: []ownedSession{{session}}, result: &CaseResult{}}
	if err := run.cleanup(); err != nil {
		t.Fatal(err)
	}
	if !stoppedTurn || stoppedCommands != 2 {
		t.Fatalf("cleanup turn=%v command stops=%d", stoppedTurn, stoppedCommands)
	}
}

func TestApprovalGuardAndSafeEvidence(t *testing.T) {
	detail := protocol.DefaultToolDetail()
	detail.Arguments = json.RawMessage(`{"path":"approval.txt","text":"fixture","expected_revision":"known"}`)
	if !approvalWriteMatches("write_file", detail, "approval.txt", "fixture", "known") || approvalWriteMatches("write_file", detail, "elsewhere.txt", "fixture", "known") || approvalWriteMatches("run_command", detail, "approval.txt", "fixture", "known") {
		t.Fatal("approval guard did not require the exact tool and fixture")
	}
	for _, err := range []error{protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "PRIVATE-RESPONSE"}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "PRIVATE-RESPONSE"}, &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "PRIVATE-RESPONSE"}}} {
		if message := safeError(fmt.Errorf("wrapped: %w", err)); strings.Contains(message, "PRIVATE") || !strings.Contains(message, "unavailable") {
			t.Fatalf("unsafe or unclassified diagnostic: %q", message)
		}
	}
	value := bounded(strings.Repeat("界", 1000), 769)
	if !utf8.ValidString(value) || len(value) > 772 {
		t.Fatal("bounded evidence broke a UTF-8 boundary")
	}
}

type fixtureTransport struct {
	dispatch  func(context.Context, protocol.Request) (link.Admission, error)
	subscribe func(context.Context, protocol.Topic) (link.Subscription, error)
}

func (*fixtureTransport) Target() protocol.NodeID { return protocol.NodeID{1} }
func (transport *fixtureTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	// The in-memory fixture follows the actual control socket's strict command
	// encoding path so a map cannot impersonate a declared union payload.
	encoded, err := json.Marshal(request)
	if err != nil {
		return link.Admission{}, err
	}
	var decoded protocol.Request
	if err := protocol.DecodeAs(protocol.PeerCommand, encoded, &decoded); err != nil {
		return link.Admission{}, err
	}
	return transport.dispatch(ctx, decoded)
}
func (*fixtureTransport) Open(context.Context, protocol.StreamID) (link.Stream, error) {
	return nil, errors.New("fixture does not open streams")
}
func (transport *fixtureTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	if transport.subscribe != nil {
		return transport.subscribe(ctx, topic)
	}
	return nil, errors.New("fixture does not subscribe")
}
func (*fixtureTransport) WatchConnection(ctx context.Context, _ func(link.ConnectionInfo)) error {
	<-ctx.Done()
	return ctx.Err()
}

func fixtureResponse(request protocol.Request, output protocol.Output) link.Admission {
	done := make(chan link.Response, 1)
	done <- link.Response{Output: output}
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: done}
}
