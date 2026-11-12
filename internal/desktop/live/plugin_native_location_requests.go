package live

import (
	"context"
	"encoding/json"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
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
	output, err := state.runtime.resolvedRequest(ctx, request)
	if err != nil {
		return protocol.Worktree{}, err
	}
	if output.Kind == "worktree" {
		tree, valid := detail[protocol.Worktree](output.Data)
		if valid && tree.ID == *expected && tree.Project != nil && *tree.Project == *project {
			return tree, nil
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
	output, err := state.runtime.resolvedRequest(ctx, request)
	if err != nil {
		return nil, err
	}
	value, valid := detail[protocol.OutputWorktreeRemovedPayload](output.Data)
	if output.Kind != "worktree_removed" || !valid || value.ID != tree {
		return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "worktree removal is not confirmed")
	}
	return state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
		location.releasePresentation(tree)
		return json.RawMessage("null"), nil
	})
}

// resolvedRequest answers a package that asks what became of the request it
// sent. A stored result is returned as it is, a stored failure as its fault,
// and an outcome the Node can never tell as outcome_unknown, so the package
// reports each once. A request the Node still runs, one that never arrived
// and a Node that cannot be asked are unavailable: the package asks again.
func (r *Runtime) resolvedRequest(ctx context.Context, request protocol.Request) (protocol.Output, error) {
	resolution, err := r.controller.Resolve(ctx, request)
	if err == nil && resolution.State != client.RequestPending && resolution.State != client.RequestAbsent {
		return resolution.Result()
	}
	return protocol.Output{}, pluginNativeFault(protocol.ErrorCodeUnavailable, "request outcome is not confirmed")
}
