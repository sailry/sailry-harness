package hostctl

import (
	"context"
	"encoding/json"
	"io"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func serveTask(ctx context.Context, line []byte, writer io.Writer, transport link.Transport) error {
	controller := client.New(transport)
	defer controller.Close()
	node := controller.Target()
	result := TaskResult{Version: 1, Node: &node, State: "admitted", Approvals: []protocol.ConversationApproval{}, Questions: []protocol.ConversationQuestion{}}
	write := func(result TaskResult) error { return writeLine(writer, result, MaxTaskResponse) }
	var task Task
	if err := json.Unmarshal(line, &task); err != nil {
		return write(result.failed(&protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid Host task request"}))
	}
	result = task.Result()
	result.Node = &node
	if task.Command == "submit" {
		command, err := taskCommand(ctx, controller, task)
		if err != nil {
			return write(result.failed(err))
		}
		request := controller.Prepare(command)
		request.ID = task.Request
		output, err := controller.Execute(ctx, request)
		if err != nil {
			if asFault(err).Code == protocol.ErrorCodeOutcomeUnknown && ctx.Err() == nil {
				outcome, inspectErr := controller.Outcome(ctx, request)
				if inspectErr == nil && outcome.Kind == "completed" {
					completed, payloadErr := payload[protocol.Result[protocol.Output, protocol.Fault]](outcome.Data)
					if payloadErr == nil {
						if completed.Err != nil {
							err = completed.Err
						} else if completed.Ok != nil {
							output, err = *completed.Ok, nil
						}
					}
				}
			}
			if err != nil {
				return write(result.failed(err))
			}
		}
		turn, err := payload[protocol.QueuedTurn](output.Data)
		if output.Kind != "queued_turn" || err != nil || turn.Request != task.Request || task.Target.Kind == "session" && turn.Session != task.Target.Session {
			return write(result.uncertain("Host task admission returned mismatched identifiers"))
		}
		result.Session, result.Turn, result.Revision = &turn.Session, &turn.ID, &turn.Revision
		if err := write(result); err != nil {
			return err
		}
		if !task.Wait {
			return nil
		}
	} else if err := write(result); err != nil {
		return err
	}
	scoped, cancel := context.WithTimeout(ctx, time.Duration(task.Timeout)*time.Second)
	defer cancel()
	observed := observeTask(scoped, controller, *result.Session, *result.Turn)
	result.State, result.Text, result.Error = observed.State, observed.Text, observed.Error
	result.Approvals, result.Questions = observed.Approvals, observed.Questions
	if observed.Revision != nil {
		result.Revision = observed.Revision
	}
	return write(result)
}

func taskCommand(ctx context.Context, controller *client.Client, task Task) (protocol.Command, error) {
	if task.Target.Kind == "session" {
		return protocol.Command{Kind: "submit_task", Data: protocol.CommandSubmitTaskPayload{Session: task.Target.Session, ExpectedRevision: task.Target.ExpectedRevision, Message: task.Message}}, nil
	}
	if task.Target.Association != nil {
		if err := validateAssociation(*task.Target.Association); err != nil {
			return protocol.Command{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "session association is invalid"}
		}
	}
	output, err := controller.Do(ctx, protocol.Command{Kind: "read_worktree_catalog", Data: protocol.CommandReadWorktreeCatalogPayload{Worktree: task.Target.Worktree}})
	if err != nil {
		return protocol.Command{}, err
	}
	catalog, err := payload[protocol.ProjectsCatalog](output.Data)
	if output.Kind != "project_catalog" || err != nil {
		return protocol.Command{}, &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "worktree catalog expected"}
	}
	var project *protocol.ProjectID
	for _, worktree := range catalog.Worktrees {
		if worktree.ID == task.Target.Worktree {
			project = worktree.Project
			break
		}
	}
	if project == nil {
		return protocol.Command{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "new task requires a registered project worktree"}
	}
	start := protocol.ConversationStart{
		Project: project, Worktree: &task.Target.Worktree, Config: task.Target.Config,
		Title: task.Title, Message: task.Message, Execution: task.Target.Execution,
		Team: task.Target.Team, Association: task.Target.Association, Models: map[protocol.RoleID]protocol.RoleModel{},
	}
	return protocol.Command{Kind: "start_session", Data: start}, nil
}
