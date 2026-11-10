package node

import (
	"context"
	"database/sql"
	"errors"
	"strings"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/node/hooks"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) turnHooks(ctx context.Context, inv *store.AgentInvocation) *hooks.Binding {
	return hooks.New(hooks.Capture{Node: node.ID(), Worktree: inv.Run.Worktree, Session: inv.Session.ID, Turn: inv.Run.Turn, Settings: inv.Hooks, Admitted: inv.Run.Kind == protocol.ConversationRunKindTask && inv.Turn.Config.Mode != protocol.WorkModePlan, Stop: ctx,
		Claim: func(ctx context.Context, turn protocol.TurnID, phase protocol.HooksPhase, ordinal uint64) error {
			return node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error { return store.ClaimHook(ctx, db, turn, phase, ordinal) })
		},
		Finish: func(ctx context.Context, turn protocol.TurnID, phase protocol.HooksPhase, ordinal uint64, outcome protocol.ProcessOutcome, err error) error {
			return node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error { return store.FinishHook(ctx, db, turn, phase, ordinal, outcome, err) })
		},
		Launch: func(ctx context.Context, command string, timeout uint64, environment map[string]string) (protocol.ProcessCompletion, error) {
			return node.processes.Execute(ctx, process.Launch{ID: protocol.NewRequestID(), Session: inv.Session.ID, Turn: inv.Run.Turn, Worktree: inv.Run.Worktree, Root: inv.Worktree.Path, Profile: node.profile.Ownership.Path, Command: command, Access: protocol.ProcessAccessWorkspace, Cwd: "", TimeoutMs: timeout, Environment: environment})
		},
	})
}
func (node *Node) workspaceProcess(ctx context.Context, caller protocol.NodeID, request protocol.Request, effect *workspaceCompletion) (protocol.Output, error) {
	switch input := request.Command.Data.(type) {
	case *protocol.CommandRunCommandPayload:
		return node.launchWorkspaceCommand(ctx, caller, request, *input, effect)
	case *protocol.CommandListCommandsPayload:
		if err := node.requireWorkspaceSession(ctx, input.Session); err != nil {
			return protocol.Output{}, err
		}
		return protocol.Output{Kind: "commands", Data: node.processes.List(input.Session)}, nil
	case *protocol.CommandReadCommandPayload:
		until := ""
		if input.Until != nil {
			until = *input.Until
		}
		if err := node.processes.Wait(ctx, input.Session, input.ID, input.WaitMs, until); err != nil {
			return protocol.Output{}, err
		}
		value, err := node.processes.Read(input.Session, input.ID)
		return protocol.Output{Kind: "command_output", Data: value}, err
	case *protocol.CommandReadCommandPagePayload:
		until := ""
		if input.Until != nil {
			until = *input.Until
		}
		if err := node.processes.Wait(ctx, input.Session, input.ID, input.WaitMs, until); err != nil {
			return protocol.Output{}, err
		}
		value, err := node.processes.Page(input.Session, input.ID, input.Stream, input.Offset, input.Limit)
		return protocol.Output{Kind: "command_output_page", Data: value}, err
	case *protocol.CommandStopCommandPayload:
		value, err := node.processes.Stop(input.Session, input.ID)
		return protocol.Output{Kind: "command_output", Data: value}, err
	case *protocol.CommandStopCommandsPayload:
		return protocol.Output{Kind: "commands", Data: node.processes.StopAll(input.Session)}, nil
	case *protocol.CommandWriteCommandPayload:
		value, err := node.processes.Write(input.Session, input.ID, input.Input)
		return protocol.Output{Kind: "command_input_accepted", Data: protocol.OutputCommandInputAcceptedPayload{ID: input.ID, Bytes: value}}, err
	}
	return protocol.Output{}, errors.New("process command required")
}
func (node *Node) requireWorkspaceSession(ctx context.Context, id protocol.SessionID) error {
	return node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error { _, err := store.ReadSession(ctx, db, id); return err })
}
func (node *Node) launchWorkspaceCommand(ctx context.Context, caller protocol.NodeID, request protocol.Request, input protocol.CommandRunCommandPayload, effect *workspaceCompletion) (protocol.Output, error) {
	var run protocol.ConversationRun
	var root string
	var attachments []protocol.Attachment
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		run, err = store.ReadAgentRun(ctx, db, input.Turn)
		if err != nil {
			return err
		}
		if run.Status != protocol.ConversationStatusRunning || run.Kind != protocol.ConversationRunKindTask {
			return &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "task turn is not running"}
		}
		root, err = store.ReadWorkspaceRoot(ctx, db, run.Worktree, false)
		if err != nil {
			return err
		}
		seen := map[protocol.AttachmentID]bool{}
		for _, id := range input.Attachments {
			if seen[id] {
				return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "duplicate command attachment"}
			}
			seen[id] = true
			value, err := store.ResolveTurnAttachment(ctx, db, input.Turn, id)
			if err != nil {
				return err
			}
			attachments = append(attachments, value)
		}
		return nil
	})
	if err != nil {
		return protocol.Output{}, err
	}
	root, err = node.workspaceRoot(ctx, run.Worktree, false)
	if err != nil {
		return protocol.Output{}, err
	}
	if input.Description != nil && (strings.TrimSpace(*input.Description) == "" || strings.ContainsRune(*input.Description, 0)) {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid command description"}
	}
	launch := process.Launch{ID: request.ID, Session: run.Session, Turn: input.Turn, Worktree: run.Worktree, Root: root, Profile: node.profile.Ownership.Path, Command: input.Command, Access: input.Access, Cwd: input.Cwd, TimeoutMs: input.TimeoutMs, Pty: input.Pty, Background: input.Background}
	if err = launch.Validate(); err != nil {
		return protocol.Output{}, err
	}
	if len(attachments) > 0 {
		launch.Inputs, launch.Release, err = node.transfers.StageInputs(ctx, attachments)
		if err != nil {
			return protocol.Output{}, err
		}
	}
	python, err := node.agent.CommandPython(ctx, input.Turn)
	if err != nil {
		if launch.Release != nil {
			launch.Release()
		}
		return protocol.Output{}, err
	}
	if python != nil {
		launch.Environment = python.Variables()
		release := launch.Release
		launch.Release = func() {
			_ = python.Close()
			if release != nil {
				release()
			}
		}
	}
	if launch.Release != nil {
		release := launch.Release
		var once sync.Once
		launch.Release = func() { once.Do(release) }
	}
	operation, finishOperation, ok := node.agent.BeginOperation(input.Turn)
	if !ok {
		if launch.Release != nil {
			launch.Release()
		}
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "turn is no longer running"}
	}
	effect.release = finishOperation
	if err := operation.Err(); err != nil {
		if launch.Release != nil {
			launch.Release()
		}
		return protocol.Output{}, err
	}
	gate := make(chan struct{})
	launch.CompletionGate = gate
	effect.after = func() { close(gate) }
	if input.Background {
		value, err := node.processes.Start(launch)
		if err != nil && launch.Release != nil {
			launch.Release()
		}
		if err == nil {
			effect.external = true
			effect.commit = func(ctx context.Context, db *sql.Conn, out protocol.Output) (protocol.Output, []protocol.Event, error) {
				changes, err := store.RegisterCommand(ctx, db, request, time.Now())
				return out, changes, err
			}
		}
		return protocol.Output{Kind: "command_started", Data: value}, err
	}
	promotion, release := node.commandPromotion(operation, input.Turn)
	defer release()
	output, err := node.processes.ExecutePromotable(operation, launch, promotion)
	if err != nil && launch.Release != nil {
		launch.Release()
	}
	if err == nil && output.Kind == "command_started" {
		effect.external = true
		effect.commit = func(ctx context.Context, db *sql.Conn, out protocol.Output) (protocol.Output, []protocol.Event, error) {
			changes, err := store.RegisterPromotedCommand(ctx, db, request, time.Now())
			return out, changes, err
		}
	}
	return output, err

}
func (node *Node) activeWorkspaceCommands(ctx context.Context, worktree protocol.WorktreeID) bool {
	var snapshot protocol.Snapshot
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		snapshot, err = store.Snapshot(ctx, db, node.ID())
		return err
	})
	if err != nil {
		return true
	}
	for _, session := range snapshot.Sessions {
		if session.Worktree != worktree {
			continue
		}
		for _, command := range node.processes.List(session.ID) {
			if command.Status.Kind == "running" || command.Status.Kind == "stopping" {
				return true
			}
		}
	}
	return false
}

func (node *Node) commandPromotion(ctx context.Context, turn protocol.TurnID) (<-chan struct{}, func()) {
	owner, cancel := context.WithCancel(ctx)
	ready := make(chan struct{})
	done := make(chan struct{})
	signal := node.agent.GuidanceSignal(turn)
	delay := node.processAfter(30 * time.Second)
	go func() {
		defer close(done)
		select {
		case <-owner.Done():
			return
		case <-delay:
		}
		for {
			var pending bool
			err := node.profile.Store.Node.Read(owner, func(db *sql.Conn) error {
				return db.QueryRowContext(owner, "SELECT EXISTS(SELECT 1 FROM agent_pending WHERE recipient=?1)", turn.String()).Scan(&pending)
			})
			if err != nil {
				return
			}
			if pending {
				close(ready)
				return
			}
			select {
			case <-owner.Done():
				return
			case <-signal:
			}
		}
	}()
	return ready, func() { cancel(); <-done }
}
