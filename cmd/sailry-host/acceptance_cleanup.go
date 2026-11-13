package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// cleanupAcceptanceSessions runs after the child has exited. The caller owns a
// bounded context independent of child cancellation and supplies only sessions
// the child reported owning. Retained records are never removed.
func cleanupAcceptanceSessions(ctx context.Context, controller *client.Client, sessions []protocol.SessionID) error {
	if len(sessions) == 0 {
		return nil
	}
	if controller == nil {
		return errors.New("Acceptance cleanup requires an existing Client")
	}
	var pending []protocol.SessionID
	canStopCommands := map[protocol.SessionID]bool{}
	finalStop := map[protocol.SessionID]bool{}
	stoppedTurns := map[protocol.TurnID]bool{}
	var problems []error
	for _, session := range sessions {
		if _, seen := canStopCommands[session]; seen {
			continue
		}
		_, err := acceptanceCleanupCommand(ctx, controller, protocol.Command{Kind: "stop_commands", Data: protocol.CommandStopCommandsPayload{Session: session}})
		canStopCommands[session] = err == nil
		problems = append(problems, err)
		pending = append(pending, session)
	}
	ticker := time.NewTicker(100 * time.Millisecond)
	defer ticker.Stop()
	for len(pending) > 0 {
		var remaining []protocol.SessionID
		for _, session := range pending {
			output, err := acceptanceCleanupCommand(ctx, controller, protocol.Command{Kind: "read_conversation", Data: protocol.CommandReadConversationPayload{Session: session, Limit: 100}})
			if err != nil {
				problems = append(problems, err)
				continue
			}
			history, err := acceptanceCleanupOutput[protocol.ConversationHistory](output, "conversation")
			if err != nil || history.Page.Session != session {
				problems = append(problems, errors.New("Acceptance cleanup could not confirm its session history"))
				continue
			}
			active := false
			for _, run := range history.Page.Runs {
				if run.Session != session || !acceptanceCleanupActive(run.Status) {
					continue
				}
				active = true
				if !stoppedTurns[run.Turn] {
					stoppedTurns[run.Turn] = true
					_, err := acceptanceCleanupCommand(ctx, controller, protocol.Command{Kind: "stop_turn", Data: protocol.CommandStopTurnPayload{Turn: run.Turn}})
					problems = append(problems, err)
				}
			}
			if active {
				remaining = append(remaining, session)
				continue
			}
			if canStopCommands[session] && !finalStop[session] {
				// A turn can start a command after the first stop. Once all its
				// turns are terminal, stop that last set, then observe cessation.
				// A failed first stop is never repeated under a new request ID.
				finalStop[session] = true
				_, err := acceptanceCleanupCommand(ctx, controller, protocol.Command{Kind: "stop_commands", Data: protocol.CommandStopCommandsPayload{Session: session}})
				problems = append(problems, err)
			}
			output, err = acceptanceCleanupCommand(ctx, controller, protocol.Command{Kind: "list_commands", Data: protocol.CommandListCommandsPayload{Session: session}})
			if err != nil {
				problems = append(problems, err)
				continue
			}
			commands, err := acceptanceCleanupOutput[[]protocol.ProcessInfo](output, "commands")
			if err != nil {
				problems = append(problems, err)
				continue
			}
			for _, command := range commands {
				if command.Session == session && (command.Status.Kind == "running" || command.Status.Kind == "stopping") {
					remaining = append(remaining, session)
					break
				}
			}
		}
		pending = remaining
		if len(pending) == 0 {
			break
		}
		select {
		case <-ctx.Done():
			return errors.Join(append(problems, errors.New("Acceptance cleanup deadline expired before cessation was confirmed"))...)
		case <-ticker.C:
		}
	}
	return errors.Join(problems...)
}

func acceptanceCleanupActive(status protocol.ConversationStatus) bool {
	return status == protocol.ConversationStatusQueued || status == protocol.ConversationStatusRunning || status == protocol.ConversationStatusStopping
}

func acceptanceCleanupCommand(ctx context.Context, controller *client.Client, command protocol.Command) (protocol.Output, error) {
	request := controller.Prepare(command)
	var output protocol.Output
	var err error
	if command.Durable() {
		// Settle executes once and only inspects the captured request if its
		// result is unknown. Cleanup never resubmits an uncertain mutation.
		output, err = controller.Settle(ctx, request)
	} else {
		output, err = controller.Execute(ctx, request)
	}
	if err == nil {
		return output, nil
	}
	detail := "Node operation failed"
	var value protocol.Fault
	var pointer *protocol.Fault
	switch {
	case errors.Is(err, context.DeadlineExceeded), errors.Is(err, context.Canceled):
		detail = "Cleanup observation ended"
	case errors.As(err, &value):
		detail = string(value.Code)
	case errors.As(err, &pointer) && pointer != nil:
		detail = string(pointer.Code)
	}
	return protocol.Output{}, fmt.Errorf("Acceptance cleanup %s request %s: %s", command.Kind, request.ID.String(), detail)
}

func acceptanceCleanupOutput[T any](output protocol.Output, kind string) (T, error) {
	var value T
	if output.Kind == kind {
		encoded, err := json.Marshal(output.Data)
		if err == nil && json.Unmarshal(encoded, &value) == nil {
			return value, nil
		}
	}
	return value, errors.New("Acceptance cleanup received an invalid " + kind + " result")
}
