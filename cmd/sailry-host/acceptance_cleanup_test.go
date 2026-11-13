package main

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/acceptance"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type acceptanceCleanupTransport struct {
	dispatch func(context.Context, protocol.Request) (protocol.Output, error)
	admit    func(context.Context, protocol.Request) (link.Admission, error)
}

func (*acceptanceCleanupTransport) Target() protocol.NodeID { return protocol.NodeID{1} }
func (*acceptanceCleanupTransport) Open(context.Context, protocol.StreamID) (link.Stream, error) {
	return nil, errors.New("fixture has no binary streams")
}
func (*acceptanceCleanupTransport) Subscribe(context.Context, protocol.Topic) (link.Subscription, error) {
	return nil, errors.New("fixture has no subscriptions")
}
func (*acceptanceCleanupTransport) WatchConnection(ctx context.Context, _ func(link.ConnectionInfo)) error {
	<-ctx.Done()
	return ctx.Err()
}
func (transport *acceptanceCleanupTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	// Keep the same typed boundary as the real Host control transport.
	encoded, err := json.Marshal(request)
	if err != nil {
		return link.Admission{}, err
	}
	var received protocol.Request
	if err := protocol.DecodeAs(protocol.PeerCommand, encoded, &received); err != nil {
		return link.Admission{}, err
	}
	if transport.admit != nil {
		return transport.admit(ctx, received)
	}
	output, err := transport.dispatch(ctx, received)
	if err != nil {
		return link.Admission{}, err
	}
	return acceptanceFixtureResponse(request, output), nil
}

func acceptanceFixtureResponse(request protocol.Request, output protocol.Output) link.Admission {
	completed := make(chan link.Response, 1)
	completed <- link.Response{Output: output}
	close(completed)
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completed}
}

func acceptanceFixtureSession(id protocol.SessionID) protocol.Session {
	session := protocol.DefaultSession()
	session.ID, session.Worktree, session.Revision = id, protocol.WorktreeID{1}, 7
	session.Config.Provider, session.Config.Model = protocol.ProviderID{1}, "fixture"
	session.Config.Mode, session.Config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	return session
}

func TestAcceptanceEmergencyCleanupUsesLedgerWithoutChildEvents(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "acceptance")
	owned := protocol.SessionID{1}
	session := acceptanceFixtureSession(owned)
	stops, archived := 0, false
	transport := &acceptanceCleanupTransport{dispatch: func(ctx context.Context, request protocol.Request) (protocol.Output, error) {
		if ctx.Err() != nil {
			t.Fatal("emergency cleanup inherited canceled observation")
		}
		switch input := request.Command.Data.(type) {
		case *protocol.CommandCreateSessionPayload:
			return protocol.Output{Kind: "session", Data: session}, nil
		case *protocol.CommandReadSessionPayload:
			if input.Session != owned {
				t.Fatal("cleanup read an unowned session")
			}
			return protocol.Output{Kind: "session", Data: session}, nil
		case *protocol.CommandStopCommandsPayload:
			if input.Session != owned {
				t.Fatal("cleanup stopped an unowned session")
			}
			stops++
			return protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{}}, nil
		case *protocol.CommandListCommandsPayload:
			if input.Session != owned || stops != 2 {
				t.Fatal("process verification escaped its owner or preceded both stops")
			}
			return protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{}}, nil
		case *protocol.CommandReadConversationPayload:
			if input.Session != owned {
				t.Fatal("cleanup read a fixture or replay identity")
			}
			history := protocol.DefaultConversationHistory()
			history.Page.Session = owned
			return protocol.Output{Kind: "conversation", Data: history}, nil
		case *protocol.CommandSetSessionArchivedPayload:
			if input.Session != owned || input.ExpectedRevision != session.Revision || !input.Archived {
				t.Fatal("cleanup did not revision-archive its owner")
			}
			if _, err := acceptance.ReadRecording(directory, owned); err != nil {
				t.Fatal("archive preceded durable recording")
			}
			archived, session.Archived = true, true
			return protocol.Output{Kind: "session", Data: session}, nil
		default:
			t.Fatalf("unexpected emergency operation %q", request.Command.Kind)
			return protocol.Output{}, nil
		}
	}}
	controller := client.New(transport)
	t.Cleanup(func() { _ = controller.Close() })
	// This identity was journaled before the child managed to emit any case.
	if _, err := acceptance.Execute(t.Context(), controller, directory, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Config: &session.Config}}); err != nil {
		t.Fatal(err)
	}
	if _, err := acceptance.WriteReport(directory, acceptance.Report{Version: 1, Suite: "full", Cases: []acceptance.CaseResult{
		{Group: "ui", Session: (protocol.SessionID{2}).String(), OwnedSessions: []string{(protocol.SessionID{2}).String()}},
		{Group: "replay", Session: (protocol.SessionID{3}).String()},
	}}); err != nil {
		t.Fatal(err)
	}
	canceled, cancel := context.WithCancel(t.Context())
	cancel()
	if err := cleanupAcceptanceSessions(canceled, controller, directory); err != nil {
		t.Fatal(err)
	}
	if !archived || stops != 2 {
		t.Fatalf("owned session not cleaned: archived=%v stops=%d", archived, stops)
	}
}

func TestAcceptanceEmergencyCleanupDoesNotReplayUnknownStop(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "acceptance")
	owned := protocol.SessionID{1}
	stops, inspections := 0, 0
	var original protocol.RequestID
	transport := &acceptanceCleanupTransport{admit: func(_ context.Context, request protocol.Request) (link.Admission, error) {
		switch request.Command.Kind {
		case "stop_commands":
			stops++
			original = request.ID
			completed := make(chan link.Response)
			close(completed)
			return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: true}, Completion: completed}, nil
		case "inspect_request":
			inspections++
			input := request.Command.Data.(*protocol.CommandInspectRequestPayload)
			if input.ID != original {
				t.Fatal("emergency cleanup changed the uncertain request identity")
			}
			return acceptanceFixtureResponse(request, protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: original, Outcome: protocol.RequestOutcome{Kind: "admitted"}}}), nil
		default:
			t.Fatalf("an unconfirmed stop allowed another operation: %q", request.Command.Kind)
			return link.Admission{}, nil
		}
	}}
	controller := client.New(transport)
	t.Cleanup(func() { _ = controller.Close() })
	if err := acceptance.Own(directory, controller.Target(), owned); err != nil {
		t.Fatal(err)
	}
	if _, err := acceptance.Execute(t.Context(), controller, directory, protocol.Command{Kind: "stop_commands", Data: protocol.CommandStopCommandsPayload{Session: owned}}); err == nil {
		t.Fatal("fixture stop must have an uncertain outcome")
	}
	if err := cleanupAcceptanceSessions(t.Context(), controller, directory); err == nil {
		t.Fatal("unknown stop must remain blocked")
	}
	remaining, err := acceptance.OwnedSessions(directory, controller.Target())
	if err != nil || !slices.Equal(remaining, []protocol.SessionID{owned}) || stops != 1 || inspections != 2 {
		t.Fatalf("unknown stop was lost or repeated: owners=%v stops=%d inspections=%d err=%v", remaining, stops, inspections, err)
	}
}

func TestAcceptanceEmergencyCleanupRejectsOtherNode(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "acceptance")
	if err := acceptance.Own(directory, protocol.NodeID{2}, protocol.SessionID{1}); err != nil {
		t.Fatal(err)
	}
	controller := client.New(&acceptanceCleanupTransport{dispatch: func(context.Context, protocol.Request) (protocol.Output, error) {
		t.Fatal("another Node's ledger authorized cleanup")
		return protocol.Output{}, nil
	}})
	t.Cleanup(func() { _ = controller.Close() })
	if err := cleanupAcceptanceSessions(t.Context(), controller, directory); err == nil {
		t.Fatal("another Node's ledger was accepted")
	}
}

func TestAcceptanceEmergencyCleanupRecoversUnreportedCreation(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "acceptance")
	session := acceptanceFixtureSession(protocol.SessionID{1})
	creates, inspections, archived := 0, 0, false
	var original protocol.RequestID
	controller := client.New(&acceptanceCleanupTransport{admit: func(_ context.Context, request protocol.Request) (link.Admission, error) {
		switch request.Command.Kind {
		case "create_session":
			creates++
			original = request.ID
			completed := make(chan link.Response)
			close(completed)
			return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: true}, Completion: completed}, nil
		case "inspect_request":
			inspections++
			input := request.Command.Data.(*protocol.CommandInspectRequestPayload)
			if input.ID != original {
				t.Fatal("recovery used another creation identity")
			}
			outcome := protocol.RequestOutcome{Kind: "admitted"}
			if inspections > 1 {
				output := protocol.Output{Kind: "session", Data: session}
				outcome = protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &output}}
			}
			return acceptanceFixtureResponse(request, protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: original, Outcome: outcome}}), nil
		case "read_session":
			return acceptanceFixtureResponse(request, protocol.Output{Kind: "session", Data: session}), nil
		case "stop_commands", "list_commands":
			return acceptanceFixtureResponse(request, protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{}}), nil
		case "read_conversation":
			history := protocol.DefaultConversationHistory()
			history.Page.Session = session.ID
			return acceptanceFixtureResponse(request, protocol.Output{Kind: "conversation", Data: history}), nil
		case "set_session_archived":
			archived, session.Archived = true, true
			return acceptanceFixtureResponse(request, protocol.Output{Kind: "session", Data: session}), nil
		default:
			t.Fatalf("unexpected recovered creation request %q", request.Command.Kind)
			return link.Admission{}, nil
		}
	}})
	t.Cleanup(func() { _ = controller.Close() })
	if _, err := acceptance.Execute(t.Context(), controller, directory, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Config: &session.Config}}); err == nil {
		t.Fatal("creation must initially be uncertain")
	}
	if err := cleanupAcceptanceSessions(t.Context(), controller, directory); err != nil {
		t.Fatal(err)
	}
	if creates != 1 || inspections != 2 || !archived {
		t.Fatalf("unreported creation was lost or repeated: creates=%d inspections=%d archived=%v", creates, inspections, archived)
	}
}
