package live

import (
	"context"
	"encoding/json"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (location *pluginNativeLocation) confirm(ctx context.Context, cursor string, expected *protocol.WorktreeID, id protocol.RequestID) (protocol.Worktree, error) {
	state := location.state
	if state.native.PreparedRequest == nil {
		return protocol.Worktree{}, pluginNativeFault(protocol.ErrorCodeUnavailable, "worktree handoff is not confirmed")
	}
	request, err := state.native.PreparedRequest(ctx, id.String())
	if err != nil {
		return protocol.Worktree{}, err
	}
	var project *protocol.ProjectID
	_, err = state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
		location.refresh()
		if err := location.check(cursor); err != nil {
			return nil, err
		}
		if location.project != nil {
			project = new(*location.project)
		}
		admitted := false
		switch request.Command.Kind {
		case "register_worktree":
			value, valid := detail[protocol.CommandRegisterWorktreePayload](request.Command.Data)
			admitted = valid && project != nil && value.Project == *project
		case "create_worktree":
			value, valid := detail[protocol.CommandCreateWorktreePayload](request.Command.Data)
			admitted = valid && project != nil && value.Project == *project
		case "create_managed_worktree":
			value, valid := detail[protocol.CommandCreateManagedWorktreePayload](request.Command.Data)
			admitted = valid && project != nil && value.Project == *project &&
				state.scope.Worktree != nil && value.Source == *state.scope.Worktree
		}
		if !admitted || expected == nil {
			return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "worktree handoff is outside the captured project")
		}
		return json.RawMessage("null"), nil
	})
	if err != nil {
		return protocol.Worktree{}, err
	}
	outcome, err := state.runtime.controller.Outcome(ctx, request)
	if err == nil && outcome.Kind == "completed" {
		result, valid := detail[protocol.Result[protocol.Output, protocol.Fault]](outcome.Data)
		if valid && result.Err == nil && result.Ok != nil && result.Ok.Kind == "worktree" {
			tree, valid := detail[protocol.Worktree](result.Ok.Data)
			if valid && tree.ID == *expected && tree.Project != nil && *tree.Project == *project {
				return tree, nil
			}
		}
	}
	return protocol.Worktree{}, pluginNativeFault(protocol.ErrorCodeUnavailable, "worktree handoff is not confirmed")
}

func (location *pluginNativeLocation) remember(tree protocol.Worktree) {
	location.known[tree.ID], location.confirmed[tree.ID] = tree, tree
	if location.state.runtime.workspace == nil {
		location.state.runtime.initWorkspace()
	}
	location.state.runtime.workspace.admitted[tree.ID] = tree
}

func (location *pluginNativeLocation) release(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	tree, err := pluginNativeArgument[protocol.WorktreeID](arguments, 0)
	if err != nil {
		return nil, err
	}
	id, err := pluginNativeArgument[protocol.RequestID](arguments, 1)
	if err != nil {
		return nil, err
	}
	state := location.state
	if state.native.PreparedRequest == nil {
		return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "worktree removal is not confirmed")
	}
	request, err := state.native.PreparedRequest(ctx, id.String())
	if err != nil {
		return nil, err
	}
	_, err = state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
		location.refresh()
		if err := location.check(""); err != nil {
			return nil, err
		}
		value, valid := detail[protocol.CommandRemoveWorktreePayload](request.Command.Data)
		if !location.write || request.Command.Kind != "remove_worktree" || !valid || value.Worktree != tree {
			return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "worktree removal is outside the captured project")
		}
		return json.RawMessage("null"), nil
	})
	if err != nil {
		return nil, err
	}
	outcome, err := state.runtime.controller.Outcome(ctx, request)
	confirmed := false
	if err == nil && outcome.Kind == "completed" {
		result, valid := detail[protocol.Result[protocol.Output, protocol.Fault]](outcome.Data)
		if valid && result.Err == nil && result.Ok != nil && result.Ok.Kind == "worktree_removed" {
			value, valid := detail[protocol.OutputWorktreeRemovedPayload](result.Ok.Data)
			confirmed = valid && value.ID == tree
		}
	}
	if !confirmed {
		return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "worktree removal is not confirmed")
	}
	return state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
		location.releasePresentation(tree)
		return json.RawMessage("null"), nil
	})
}
