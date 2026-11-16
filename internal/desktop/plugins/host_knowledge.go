package plugins

import (
	"context"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (owner *Host) knowledgeClient() *client.Client {
	scope := owner.scope()
	if scope.Worktree != nil || scope.Session != nil {
		return owner.binding.Client
	}
	if owner.binding.Resources != nil {
		return owner.binding.Resources
	}
	return owner.binding.Client
}

func (owner *Host) resourceClient() *client.Client {
	if owner.binding.Resources != nil {
		return owner.binding.Resources
	}
	return owner.binding.Client
}

func (owner *Host) prepareHostCommand(command protocol.Command) protocol.Request {
	if !protocol.KnowledgeCommand(command.Kind) {
		return owner.binding.Client.Prepare(command)
	}
	entry := owner.knowledgeClient()
	target := protocol.KnowledgeRequestTarget(command, owner.binding.Client.Target(), owner.resourceClient().Target())
	return entry.PrepareKnowledge(protocol.KnowledgeRef{Node: target}, command)
}

func (owner *Host) executeHostCommand(ctx context.Context, request protocol.Request) (protocol.Output, error) {
	if request.Command.Kind == "call_host_handler" {
		return owner.resourceClient().ExecuteHostCall(ctx, request)
	}
	if protocol.KnowledgeCommand(request.Command.Kind) {
		if request.Plugin != nil && (request.Plugin.Worktree != nil || request.Plugin.Session != nil) && request.Target != owner.binding.Client.Target() {
			return owner.binding.Client.ExecuteScopedKnowledge(ctx, request)
		}
		return owner.knowledgeClient().ExecuteKnowledge(ctx, request)
	}
	return owner.binding.Client.Execute(ctx, request)
}

func (owner *Host) hostCommandOutcome(ctx context.Context, request protocol.Request) (protocol.RequestOutcome, error) {
	if request.Command.Kind == "call_host_handler" {
		return owner.resourceClient().HostCallOutcome(ctx, request)
	}
	if protocol.KnowledgeCommand(request.Command.Kind) {
		if request.Plugin != nil && (request.Plugin.Worktree != nil || request.Plugin.Session != nil) && request.Target != owner.binding.Client.Target() {
			return owner.binding.Client.ScopedKnowledgeOutcome(ctx, request)
		}
		return owner.knowledgeClient().KnowledgeOutcome(ctx, request)
	}
	return owner.binding.Client.Outcome(ctx, request)
}

func knowledgeReferenceArguments(arguments []json.RawMessage) (map[string]any, error) {
	if len(arguments) == 0 {
		return nil, hostInvalid("knowledge collection reference is required")
	}
	var id protocol.KnowledgeID
	if json.Unmarshal(arguments[0], &id) == nil {
		return map[string]any{"id": id}, nil
	}
	var reference protocol.KnowledgeRef
	if json.Unmarshal(arguments[0], &reference) != nil {
		return nil, hostInvalid("invalid knowledge collection reference")
	}
	return map[string]any{"id": reference.ID, "node": reference.Node}, nil
}

func (owner *Host) prepareKnowledgeChange(kind string, fields ...string) HostOperation {
	return func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		input, err := knowledgeReferenceArguments(arguments)
		if err != nil {
			return nil, err
		}
		for index, field := range fields {
			if index+1 >= len(arguments) {
				return nil, hostInvalid("knowledge collection change is incomplete")
			}
			input[field] = arguments[index+1]
		}
		return owner.prepareData(ctx, kind, input)
	}
}
