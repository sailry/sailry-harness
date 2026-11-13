package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type acceptanceCleanupTransport struct {
	dispatch func(context.Context, protocol.Request) (protocol.Output, error)
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
	// The real control socket encodes both sides. Retain that boundary here so
	// the fixture cannot silently accept undeclared command payloads.
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

func cleanupRun(session protocol.SessionID, turn protocol.TurnID, status protocol.ConversationStatus) protocol.ConversationRun {
	run := protocol.DefaultConversationRun()
	run.Worktree = protocol.WorktreeID{7}
	run.Session, run.Turn, run.Kind, run.Status = session, turn, protocol.ConversationRunKindTask, status
	return run
}

func cleanupHistory(session protocol.SessionID, runs ...protocol.ConversationRun) protocol.Output {
	history := protocol.DefaultConversationHistory()
	history.Page.Session, history.Page.Runs = session, runs
	return protocol.Output{Kind: "conversation", Data: history}
}

func cleanupCommands(session protocol.SessionID, status string) protocol.Output {
	command := protocol.DefaultProcessInfo()
	command.Turn = protocol.TurnID{4}
	command.ID, command.Session, command.Status.Kind = protocol.RequestID{8}, session, status
	if status == "finished" {
		command.Status.Data = protocol.ProcessOutcome{Kind: "cancelled"}
	}
	foreign := command
	foreign.ID, foreign.Session, foreign.Status = protocol.RequestID{9}, protocol.SessionID{99}, protocol.ProcessStatus{Kind: "running"}
	return protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{command, foreign}}
}

func TestAcceptanceCleanupConfirmsOwnedTurnsAndCommands(t *testing.T) {
	observer, stopObserver := context.WithCancel(t.Context())
	stopObserver()
	ctx, cancel := context.WithTimeout(context.WithoutCancel(observer), 3*time.Second)
	defer cancel()
	deadline, _ := ctx.Deadline()
	sessions := []protocol.SessionID{{2}, {3}}
	turns := map[protocol.SessionID]protocol.TurnID{sessions[0]: {4}, sessions[1]: {5}}
	reads, commands, stops := map[protocol.SessionID]int{}, map[protocol.SessionID]int{}, map[protocol.SessionID]int{}
	turnStops := map[protocol.TurnID]int{}
	transport := &acceptanceCleanupTransport{dispatch: func(current context.Context, request protocol.Request) (protocol.Output, error) {
		if got, ok := current.Deadline(); current.Err() != nil || !ok || !got.Equal(deadline) {
			t.Fatal("cleanup changed the caller's independent cancellation boundary")
		}
		switch input := request.Command.Data.(type) {
		case *protocol.CommandStopCommandsPayload:
			if _, owned := turns[input.Session]; !owned {
				t.Fatal("cleanup stopped commands outside its owned sessions")
			}
			stops[input.Session]++
			if stops[input.Session] == 2 && reads[input.Session] < 3 {
				t.Fatal("final command stop ran before the turn was confirmed terminal")
			}
			return cleanupCommands(input.Session, "stopping"), nil
		case *protocol.CommandReadConversationPayload:
			turn, owned := turns[input.Session]
			if !owned || input.Limit != 100 {
				t.Fatal("cleanup read outside its owned conversation")
			}
			reads[input.Session]++
			status := protocol.ConversationStatusCancelled
			if reads[input.Session] == 1 {
				status = protocol.ConversationStatusRunning
			} else if reads[input.Session] == 2 {
				status = protocol.ConversationStatusStopping
			}
			foreign := cleanupRun(protocol.SessionID{99}, protocol.TurnID{98}, protocol.ConversationStatusRunning)
			return cleanupHistory(input.Session, cleanupRun(input.Session, turn, status), foreign), nil
		case *protocol.CommandStopTurnPayload:
			for session, turn := range turns {
				if input.Turn == turn {
					if stops[session] == 0 {
						t.Fatal("cleanup stopped a turn before stopping its commands")
					}
					turnStops[turn]++
					return protocol.Output{Kind: "run", Data: cleanupRun(session, turn, protocol.ConversationStatusStopping)}, nil
				}
			}
			t.Fatal("cleanup stopped an unowned turn")
		case *protocol.CommandListCommandsPayload:
			if stops[input.Session] != 2 {
				t.Fatal("command observation preceded the final stop")
			}
			commands[input.Session]++
			status := "finished"
			if commands[input.Session] == 1 {
				status = "stopping"
			}
			return cleanupCommands(input.Session, status), nil
		default:
			t.Fatalf("cleanup dispatched unexpected command %q", request.Command.Kind)
		}
		return protocol.Output{}, errors.New("unexpected fixture command")
	}}
	controller := client.New(transport)
	t.Cleanup(func() { _ = controller.Close() })
	if err := cleanupAcceptanceSessions(ctx, controller, append(sessions, sessions[0])); err != nil {
		t.Fatalf("owned cleanup failed: %v", err)
	}
	for _, session := range sessions {
		if reads[session] != 4 || commands[session] != 2 || stops[session] != 2 || turnStops[turns[session]] != 1 {
			t.Fatalf("cleanup did not confirm cessation without replay: reads=%d commands=%d stops=%d turnStops=%d", reads[session], commands[session], stops[session], turnStops[turns[session]])
		}
	}
}

func TestAcceptanceCleanupInspectsUnknownWithoutReplay(t *testing.T) {
	for _, resolved := range []bool{false, true} {
		t.Run(map[bool]string{false: "unconfirmed", true: "stored_completion"}[resolved], func(t *testing.T) {
			session := protocol.SessionID{2}
			var original protocol.RequestID
			mutations, inspections, observations := 0, 0, 0
			transport := &acceptanceCleanupTransport{dispatch: func(_ context.Context, request protocol.Request) (protocol.Output, error) {
				switch input := request.Command.Data.(type) {
				case *protocol.CommandStopCommandsPayload:
					mutations++
					if input.Session != session {
						t.Fatal("cleanup changed ownership")
					}
					if mutations == 1 {
						original = request.ID
						return protocol.Output{}, fmt.Errorf("private wrapper: %w", &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "PRIVATE-RESPONSE"}})
					}
					if request.ID == original {
						t.Fatal("cleanup replayed the uncertain mutation")
					}
					return protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{}}, nil
				case *protocol.CommandInspectRequestPayload:
					inspections++
					if input.ID != original || len(input.Digest) != 64 {
						t.Fatal("cleanup inspected a different request identity")
					}
					outcome := protocol.RequestOutcome{Kind: "admitted"}
					if resolved {
						output := protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{}}
						outcome = protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &output}}
					}
					return protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: original, Outcome: outcome}}, nil
				case *protocol.CommandReadConversationPayload:
					return cleanupHistory(session), nil
				case *protocol.CommandListCommandsPayload:
					observations++
					return protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{}}, nil
				default:
					t.Fatalf("cleanup dispatched unexpected command %q", request.Command.Kind)
				}
				return protocol.Output{}, errors.New("unexpected fixture command")
			}}
			controller := client.New(transport)
			t.Cleanup(func() { _ = controller.Close() })
			err := cleanupAcceptanceSessions(t.Context(), controller, []protocol.SessionID{session})
			wantMutations := 1
			if resolved {
				wantMutations = 2
				if err != nil {
					t.Fatalf("stored completion was not accepted: %v", err)
				}
			} else if err == nil || strings.Contains(err.Error(), "PRIVATE") || !strings.Contains(err.Error(), string(protocol.ErrorCodeOutcomeUnknown)) || !strings.Contains(err.Error(), original.String()) {
				t.Fatalf("uncertain mutation lost its safe identity: %v", err)
			}
			if mutations != wantMutations || inspections != 1 || observations != 1 {
				t.Fatalf("unknown cleanup was replayed or not observed: mutations=%d inspections=%d observations=%d", mutations, inspections, observations)
			}
		})
	}
}

func TestAcceptanceCleanupContinuesOtherOwnersAfterReadFailure(t *testing.T) {
	broken, owned := protocol.SessionID{2}, protocol.SessionID{3}
	observed := false
	transport := &acceptanceCleanupTransport{dispatch: func(_ context.Context, request protocol.Request) (protocol.Output, error) {
		switch input := request.Command.Data.(type) {
		case *protocol.CommandStopCommandsPayload:
			return protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{}}, nil
		case *protocol.CommandReadConversationPayload:
			if input.Session == broken {
				return protocol.Output{}, &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "PRIVATE-RESPONSE"}}
			}
			return cleanupHistory(owned), nil
		case *protocol.CommandListCommandsPayload:
			if input.Session != owned {
				t.Fatal("failed observation was reported as confirmed")
			}
			observed = true
			return protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{}}, nil
		default:
			t.Fatalf("cleanup dispatched unexpected command %q", request.Command.Kind)
		}
		return protocol.Output{}, errors.New("unexpected fixture command")
	}}
	controller := client.New(transport)
	t.Cleanup(func() { _ = controller.Close() })
	err := cleanupAcceptanceSessions(t.Context(), controller, []protocol.SessionID{broken, owned})
	if err == nil || !observed || !strings.Contains(err.Error(), "read_conversation") || !strings.Contains(err.Error(), "unavailable") || strings.Contains(err.Error(), "PRIVATE") {
		t.Fatalf("one failed owner prevented safe cleanup of another: %v, observed=%v", err, observed)
	}
}

func TestAcceptanceCleanupRequiresConfirmedProcessCessation(t *testing.T) {
	session := protocol.SessionID{2}
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	transport := &acceptanceCleanupTransport{dispatch: func(_ context.Context, request protocol.Request) (protocol.Output, error) {
		switch request.Command.Kind {
		case "stop_commands":
			return cleanupCommands(session, "stopping"), nil
		case "read_conversation":
			return cleanupHistory(session), nil
		case "list_commands":
			cancel()
			return cleanupCommands(session, "stopping"), nil
		default:
			t.Fatalf("cleanup dispatched unexpected command %q", request.Command.Kind)
		}
		return protocol.Output{}, errors.New("unexpected fixture command")
	}}
	controller := client.New(transport)
	t.Cleanup(func() { _ = controller.Close() })
	if err := cleanupAcceptanceSessions(ctx, controller, []protocol.SessionID{session}); err == nil {
		t.Fatal("a command stop receipt was accepted without observing a terminal process")
	}
	if err := cleanupAcceptanceSessions(t.Context(), nil, nil); err != nil {
		t.Fatalf("empty ownership required a Node: %v", err)
	}
}
