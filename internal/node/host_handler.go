package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"reflect"
	"slices"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// admitHostHandler captures a plugin's identity on its execution Node before
// selecting the handler's real host. It admits no arbitrary nested command.
func (node *Node) admitHostHandler(ctx context.Context, caller protocol.NodeID, request protocol.Request) (link.Admission, error) {
	if request.Version != protocol.VERSION || request.Target != node.ID() && caller != node.ID() || request.Plugin == nil {
		return link.Admission{}, permissionDenied()
	}
	body, err := json.Marshal(request)
	if err != nil || len(body) > protocol.MAX_FRAME_BYTES {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid host handler request"}
	}
	if err := json.Unmarshal(body, &request); err != nil {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid host handler request"}
	}
	// Foreign runtime scopes arrive only through Resource, after their
	// execution host captured them. Ordinary shells have no runtime scope.
	if caller != node.ID() && (request.Plugin.Turn != nil || request.Plugin.Invocation != nil) {
		return link.Admission{}, permissionDenied()
	}
	verified, err := node.authorizeCommand(ctx, caller, request)
	if err != nil {
		return link.Admission{}, err
	}
	if caller != node.ID() {
		actor, ok := store.UserFromContext(verified)
		if !ok || !actor.Local && (actor.Controller == nil || *actor.Controller != caller) {
			return link.Admission{}, permissionDenied()
		}
	}
	if err := node.validatePluginRequest(verified, caller, request); err != nil {
		return link.Admission{}, nodeFault(err)
	}
	scope, err := node.captureResourceScope(verified, request)
	if err != nil {
		return link.Admission{}, err
	}
	if err := checkHostHandlerScope(request, scope); err != nil {
		return link.Admission{}, err
	}
	var controller protocol.NodeID
	err = node.profile.Store.Node.Read(verified, func(db *sql.Conn) error {
		var err error
		controller, err = store.CurrentController(verified, db, node.ID())
		return err
	})
	if err != nil {
		return link.Admission{}, nodeFault(err)
	}
	call := protocol.ResourceCall{Request: request, Scope: scope}
	if request.Target != node.ID() && controller != node.ID() {
		return node.sendResource(verified, controller, call, false)
	}
	return node.routeHostHandler(verified, caller, call)
}

func checkHostHandlerScope(request protocol.Request, scope *protocol.ResourceScope) error {
	input, ok := protocol.HostHandlerPayload(request.Command)
	if !ok || request.Plugin == nil || scope == nil || scope.Plugin == nil || scope.Origin == nil ||
		!reflect.DeepEqual(request.Plugin, scope.Origin) || plugins.PackageReference(*scope.Plugin) != request.Plugin.Package {
		return permissionDenied()
	}
	info := scope.Plugin
	if info.Extension == nil || info.Extension.Host == nil || info.Summary.Compatibility.Kind != "compatible" ||
		!slices.Contains(info.Extension.Host.Handlers, input.Handler) ||
		input.ReadOnly && !slices.Contains(info.Extension.Host.ReadOnly, input.Handler) {
		return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "plugin host handler is not declared"}
	}
	return nil
}

func (node *Node) routeHostHandler(ctx context.Context, caller protocol.NodeID, call protocol.ResourceCall) (link.Admission, error) {
	original := call.Request
	if call.Original != nil {
		original = *call.Original
	}
	if err := checkHostHandlerScope(original, call.Scope); err != nil {
		return link.Admission{}, err
	}
	if call.Request.Target != node.ID() {
		return node.sendResource(ctx, call.Request.Target, call, true)
	}
	if call.Request.Command.Kind == "inspect_request" {
		return node.resourceReadAdmission(ctx, call.Request, func(ctx context.Context) (protocol.Output, error) {
			return node.readKnowledgeOutcome(ctx, caller, call)
		})
	}
	return node.admitHostHandlerLocal(ctx, caller, call)
}

func (node *Node) admitHostHandlerLocal(ctx context.Context, caller protocol.NodeID, call protocol.ResourceCall) (link.Admission, error) {
	request := call.Request
	input, ok := protocol.HostHandlerPayload(request.Command)
	if !ok || request.Target != node.ID() {
		return link.Admission{}, permissionDenied()
	}
	user, ok := store.UserFromContext(ctx)
	if !ok || user.State != "active" {
		return link.Admission{}, permissionDenied()
	}
	var storage protocol.UserID
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		storage, err = store.CapturedUser(ctx, db)
		return err
	})
	if err != nil {
		return link.Admission{}, nodeFault(err)
	}
	ctx = store.WithCapturedUser(ctx, storage)
	body, err := json.Marshal(request)
	if err != nil || len(body) > protocol.MAX_FRAME_BYTES {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid host handler request"}
	}
	if err := node.begin(); err != nil {
		return link.Admission{}, err
	}
	completion := make(chan link.Response, 1)
	accepted := link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}
	if accepted.Receipt.Durable {
		var previous *link.Response
		err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			var err error
			previous, err = admission.Admit(ctx, db, caller, request, body)
			return err
		})
		if err != nil {
			node.work.Done()
			return link.Admission{}, nodeFault(err)
		}
		if previous != nil {
			node.work.Done()
			completion <- *previous
			close(completion)
			return accepted, nil
		}
	}
	err = node.tasks.Submit(func(owner context.Context) {
		defer node.work.Done()
		defer close(completion)
		var result link.Response
		defer func() {
			if recover() != nil {
				result = response(protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "host handler did not settle; inspect the original request"})
			}
			if accepted.Receipt.Durable {
				result = node.settle(caller, request.ID, result)
			}
			completion <- result
		}()
		operation := ctx
		if accepted.Receipt.Durable {
			var cancel context.CancelFunc
			operation, cancel = context.WithCancel(context.WithoutCancel(ctx))
			stop := context.AfterFunc(owner, cancel)
			defer func() { stop(); cancel() }()
		}
		// Foreign worktree, session and turn IDs remain provenance. Commands
		// emitted by the handler belong to a fresh invocation on this Node.
		scope := protocol.DefaultPluginContext()
		scope.Package = request.Plugin.Package
		output, err := node.runPluginHandler(operation, caller, request, scope, *call.Scope.Plugin, user, storage, input.Handler, input.Input, input.ReadOnly)
		result = response(output, err)
	})
	if err != nil {
		node.work.Done()
		result := response(protocol.Output{}, err)
		if accepted.Receipt.Durable {
			result = node.settle(caller, request.ID, result)
		}
		completion <- result
		close(completion)
	}
	return accepted, nil
}
