package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"reflect"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/installations"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/program"
	"github.com/sailry/sailry-harness/internal/node/python"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type pluginInvocation struct {
	caller   protocol.NodeID
	request  protocol.RequestID
	info     protocol.PluginInfo
	scope    protocol.PluginContext
	cancel   context.CancelFunc
	readOnly bool
}

type pluginControls struct {
	manager      *plugins.Manager
	python       *python.Manager
	pythonWake   chan struct{}
	preparations []protocol.PythonPreparation
	mcp          *mcp.Pool
	// programs are the running programs of package adapters.
	programs      *program.Pool
	warm          chan struct{}
	installations *installations.Queue
	mu            sync.Mutex
	invocations   map[protocol.RequestID]*pluginInvocation
}

// A package runs at most programLimit programs at once; one unused for
// programIdle is stopped.
const (
	programLimit = 4
	programIdle  = 10 * time.Minute
)

func (node *Node) initPlugins(ctx context.Context, configured *mcp.Options) error {
	options := mcp.Options{}
	if configured != nil {
		options = *configured
	}
	controls := &pluginControls{
		manager:       plugins.NewManager(node.profile.Ownership.Path),
		mcp:           mcp.New(context.WithoutCancel(node.ctx), options),
		programs:      program.NewPool(programLimit, programIdle),
		installations: installations.New(node.tasks, node.ID(), node.closed),
		invocations:   map[protocol.RequestID]*pluginInvocation{},
	}
	controls.installations.Working = node.power.Hold
	if options.Warm {
		controls.warm = make(chan struct{}, 1)
	}
	node.plugins = controls
	node.collectPlugins(ctx)
	for _, entry := range plugins.BundledDefaults() {
		var current *protocol.PluginInfo
		var recorded bool
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			current, err = store.ReadPlugin(ctx, db, entry.Name)
			if err != nil {
				return err
			}
			recorded, err = store.PluginRecorded(ctx, db, entry.Name)
			return err
		})
		if err != nil {
			return err
		}
		if recorded && current == nil || current != nil && (current.Origin == nil || current.Origin.Kind != "bundled") {
			continue
		}
		inspected, err := controls.manager.InspectBundled(ctx, entry.Name)
		if err != nil {
			return err
		}
		if current != nil && !plugins.IsNewer(current.Summary.Version, inspected.Summary.Version) {
			continue
		}
		info, err := controls.manager.InstallBundled(ctx, entry.Name)
		if err != nil {
			return err
		}
		revision := uint64(0)
		if current != nil {
			revision = current.Summary.Revision
		}
		if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			_, err := store.InstallPlugin(ctx, db, info, revision, entry.Enabled)
			return err
		}); err != nil {
			return err
		}
	}
	for _, name := range plugins.RetiredBundled() {
		if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			current, err := store.ReadPlugin(ctx, db, name)
			if err != nil || current == nil || current.Origin == nil || current.Origin.Kind != "bundled" {
				return err
			}
			_, err = store.RemovePlugin(ctx, db, name, current.Summary.Revision)
			return err
		}); err != nil {
			return err
		}
	}
	var summaries []protocol.PluginSummary
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		summaries, err = store.ReadPlugins(ctx, db)
		return err
	}); err != nil {
		return err
	}
	for _, summary := range summaries {
		if err := controls.mcp.Observe(summary); err != nil {
			return err
		}
	}
	controls.mcp.FinishInventory()
	return node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		_, err := db.ExecContext(ctx, "UPDATE node SET plugin_defaults=1 WHERE singleton=1")
		return err
	})
}

// collectPlugins deletes the package versions and settings revisions nothing
// refers to. It runs before the Node admits its first command and after its
// last work has ended, so no installation or capture can race it. Maintenance
// never blocks startup or shutdown.
func (node *Node) collectPlugins(ctx context.Context) {
	if node.abandoned.Load() {
		return
	}
	var retained map[string]struct{}
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		retained, err = store.CollectPlugins(ctx, db)
		return err
	})
	if err == nil {
		err = node.plugins.manager.Collect(retained)
	}
	if err != nil {
		slog.Warn("plugin package collection failed", "code", nodeFault(err).(*protocol.Fault).Code)
	}
}

func (node *Node) closePlugins() error {
	if node.plugins == nil {
		return nil
	}
	node.plugins.mu.Lock()
	for _, invocation := range node.plugins.invocations {
		invocation.cancel()
	}
	node.plugins.mu.Unlock()
	err := node.plugins.mcp.Close()
	node.plugins.programs.Close()
	node.computerOverlay.Shutdown()
	return err
}

func (node *Node) validatePluginRequest(ctx context.Context, caller protocol.NodeID, request protocol.Request) error {
	return node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		if scope := request.Plugin; scope != nil && scope.Turn != nil && scope.Invocation == nil && caller == node.ID() && request.Command.Durable() {
			body, err := json.Marshal(request)
			if err != nil {
				return err
			}
			outcome, err := admission.Inspect(ctx, db, caller, request.ID, fmt.Sprintf("%x", blake3.Sum256(body)))
			if err != nil {
				return err
			}
			if outcome.Kind != "not_admitted" {
				return nil
			}
		}
		return node.validatePluginRequestDB(ctx, db, caller, request)
	})
}

func (node *Node) validatePluginRequestDB(ctx context.Context, db *sql.Conn, caller protocol.NodeID, request protocol.Request) error {
	if request.Plugin == nil {
		return nil
	}
	if request.Plugin.Surface.Kind == "card" {
		return store.CheckPluginCardRequest(ctx, db, node.ID(), caller, request)
	}
	var captured *protocol.PluginInfo
	if request.Plugin.Invocation != nil {
		node.plugins.mu.Lock()
		invocation := node.plugins.invocations[*request.Plugin.Invocation]
		if invocation != nil {
			if caller != node.ID() || !reflect.DeepEqual(invocation.scope, *request.Plugin) || invocation.readOnly && request.Command.Durable() {
				node.plugins.mu.Unlock()
				return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "plugin invocation does not own this action"}
			}
			value := invocation.info
			value.Summary.Enabled = true
			captured = &value
		}
		node.plugins.mu.Unlock()
		if captured == nil {
			return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "plugin invocation is unavailable"}
		}
	}
	info, err := store.PluginRequestPackage(ctx, db, node.ID(), caller, request, captured)
	if err != nil {
		return err
	}
	return store.CheckPluginRequest(ctx, db, request, info)
}

func (node *Node) pluginCallback(ctx context.Context, caller protocol.NodeID, request protocol.Request, readOnly bool) (protocol.Output, error) {
	ctx, release, err := node.beginPluginExecution(ctx, request)
	if err != nil {
		return protocol.Output{}, err
	}
	defer release()
	var handler string
	var input any
	if readOnly {
		value, ok := dispatchPayload[protocol.CommandReadPluginCallbackPayload](request.Command.Data)
		if !ok {
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin callback is invalid"}
		}
		handler, input = value.Handler, value.Input
	} else {
		value, ok := dispatchPayload[protocol.CommandCallPluginPayload](request.Command.Data)
		if !ok {
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin callback is invalid"}
		}
		handler, input = value.Handler, value.Input
	}
	if request.Plugin == nil {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "plugin provenance is required"}
	}
	var info protocol.PluginInfo
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var captured *protocol.PluginInfo
		if request.Plugin.Invocation != nil {
			node.plugins.mu.Lock()
			invocation := node.plugins.invocations[*request.Plugin.Invocation]
			if invocation != nil && caller == node.ID() && reflect.DeepEqual(invocation.scope, *request.Plugin) {
				value := invocation.info
				value.Summary.Enabled = true
				captured = &value
			}
			node.plugins.mu.Unlock()
		}
		var err error
		info, err = store.PluginRequestPackage(ctx, db, node.ID(), caller, request, captured)
		return err
	})
	if err != nil {
		return protocol.Output{}, err
	}
	if info.Extension == nil || info.Extension.Host == nil {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "plugin has no host callbacks"}
	}
	host := info.Extension.Host
	allowed := false
	for _, declared := range host.Handlers {
		allowed = allowed || handler == declared
	}
	if !allowed {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "plugin callback is not declared"}
	}
	if readOnly {
		allowed = false
		for _, declared := range host.ReadOnly {
			allowed = allowed || handler == declared
		}
		if !allowed {
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "plugin callback is not declared read-only"}
		}
	}
	files, err := node.plugins.manager.ReadBundle(ctx, request.Plugin.Package, host.Entry, host.Resources)
	if err != nil {
		return protocol.Output{}, err
	}
	operation, cancel := context.WithCancel(ctx)
	defer cancel()
	scope := *request.Plugin
	identity := protocol.NewRequestID()
	scope.Invocation = &identity
	invocation := &pluginInvocation{caller: caller, request: request.ID, info: info, scope: scope, cancel: cancel, readOnly: readOnly}
	node.plugins.mu.Lock()
	if len(node.plugins.invocations) >= 64 {
		node.plugins.mu.Unlock()
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "plugin callback workers are busy"}
	}
	node.plugins.invocations[identity] = invocation
	node.plugins.mu.Unlock()
	defer func() { node.plugins.mu.Lock(); delete(node.plugins.invocations, identity); node.plugins.mu.Unlock() }()
	value, err := plugins.RunCallback(operation, plugins.ScriptBundle{Entry: host.Entry, Files: files}, handler, input,
		plugins.ScriptEnvironment{Context: scope, Target: node.ID(), Deadline: time.Now().Add(time.Minute)}, readOnly,
		func(ctx context.Context, command protocol.Request) (protocol.Output, error) {
			return node.dispatchCallbackResult(ctx, command)
		})
	if !readOnly && host.Status != nil && host.Status.Invoke != nil && *host.Status.Invoke == handler {
		node.invalidatePluginStatus(info.Summary.Name)
	}
	return protocol.Output{Kind: "plugin_result", Data: value}, err
}

func (node *Node) beginPluginExecution(ctx context.Context, request protocol.Request) (context.Context, func(), error) {
	if request.Plugin == nil || request.Plugin.Turn == nil {
		return ctx, func() {}, nil
	}
	owner, finish, active := node.agent.BeginOperation(*request.Plugin.Turn)
	if !active {
		return nil, nil, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "plugin turn is no longer running"}
	}
	operation, cancel := context.WithCancel(owner)
	stop := context.AfterFunc(ctx, cancel)
	return operation, func() { stop(); cancel(); finish() }, nil
}

func (node *Node) pluginCommitted(changes []protocol.Event) {
	packagesChanged := false
	for _, change := range changes {
		if change.Kind == "plugin_changed" {
			packagesChanged = true
			if summary, ok := dispatchPayload[protocol.PluginSummary](change.Data); ok {
				_ = node.plugins.mcp.Observe(summary)
				// A disabled, updated or reconfigured package starts its programs anew.
				node.plugins.programs.Invalidate(summary.Name)
				node.retainComputerOverlay(summary.Name)
			}
		}
		if change.Kind == "plugin_removed" {
			packagesChanged = true
			if removed, ok := dispatchPayload[protocol.EventPluginRemovedPayload](change.Data); ok {
				_ = node.plugins.mcp.Invalidate(removed.Name)
				node.plugins.programs.Invalidate(removed.Name)
				node.retainComputerOverlay(removed.Name)
			}
		}
	}
	if packagesChanged && node.plugins.python != nil {
		_ = node.retainPackagePython(context.WithoutCancel(node.ctx))
	}
	if packagesChanged {
		node.authChanged()
	}
	node.wakeDispatch()
	node.publish()
	node.agent.Committed()
}

func pluginFailure(err error) *protocol.Fault {
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return fault
	}
	return nodeFault(err).(*protocol.Fault)
}
