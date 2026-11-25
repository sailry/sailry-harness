package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
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
)

type pluginInvocation struct {
	caller      protocol.NodeID
	request     protocol.RequestID
	user        protocol.User
	storageUser protocol.UserID
	info        protocol.PluginInfo
	scope       protocol.PluginContext
	cancel      context.CancelFunc
	readOnly    bool
}

type pluginControls struct {
	manager            *plugins.Manager
	inventoryMu        sync.Mutex
	unavailable        map[string]struct{}
	publishedOnStartup map[string]struct{}
	memory             *plugins.MemoryTracker
	python             *python.Manager
	pythonWake         chan struct{}
	preparations       []protocol.PythonPreparation
	mcp                *mcp.Pool
	// programs are the running programs of package adapters.
	programs      *program.Pool
	warm          chan struct{}
	installations *installations.Queue
	mu            sync.Mutex
	invocations   map[protocol.RequestID]*pluginInvocation
}

// A package runs at most programLimit programs at once, each serving as many
// requests as its model adapter's concurrency declares; one unused for
// programIdle is stopped.
const (
	programLimit = 4
	programIdle  = 10 * time.Minute
)

func (node *Node) initPlugins(configured *mcp.Options) {
	options := mcp.Options{}
	if configured != nil {
		options = *configured
	}
	controls := &pluginControls{
		manager:       plugins.NewManager(node.profile.Ownership.Path),
		memory:        plugins.NewMemoryTracker(),
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
}

// Inventory follows native service initialization so module requirements describe
// available services rather than the product's requested assembly.
func (node *Node) initPluginInventory(ctx context.Context) error {
	ctx = node.pluginContext(ctx)
	node.collectPlugins(ctx)
	var users []protocol.User
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		users, err = store.ListUsers(ctx, db)
		return err
	}); err != nil {
		return err
	}
	var activeUsers []protocol.User
	for _, user := range users {
		if user.State == "active" {
			activeUsers = append(activeUsers, user)
		}
	}
	plans := make([]userPluginPlan, 0, len(activeUsers))
	published := map[string]struct{}{}
	for _, user := range activeUsers {
		plan, err := node.prepareUserPlugins(ctx, user, true)
		if err != nil {
			return err
		}
		for _, digest := range plan.publishedDigests {
			published[digest] = struct{}{}
		}
		plans = append(plans, plan)
	}
	summariesByUser := make([][]protocol.PluginSummary, len(plans))
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		for index, plan := range plans {
			if plan.hasChanges() {
				summaries, err := applyUserPluginPlan(ctx, db, plan)
				if err != nil {
					return err
				}
				summariesByUser[index] = summaries
			} else {
				summariesByUser[index] = plan.summaries
			}
		}
		_, err := db.ExecContext(ctx, "UPDATE node SET plugin_defaults=1 WHERE singleton=1")
		return err
	}); err != nil {
		return err
	}
	for index, plan := range plans {
		if err := node.observeUserPluginInventory(plan.user, summariesByUser[index]); err != nil {
			return err
		}
	}
	if len(published) != 0 {
		node.plugins.mu.Lock()
		node.plugins.publishedOnStartup = published
		node.plugins.mu.Unlock()
	}
	return nil
}

// InitializeUserPlugins installs bundled defaults without copying another user's
// settings or restoring that user's explicit removal choices.
func (node *Node) InitializeUserPlugins(ctx context.Context, user protocol.User) error {
	return node.initializeUserPlugins(ctx, user, false)
}

type pendingPluginInstall struct {
	info     protocol.PluginInfo
	revision uint64
	enabled  bool
}

type userPluginPlan struct {
	user             protocol.User
	summaries        []protocol.PluginSummary
	installs         []pendingPluginInstall
	retirements      []string
	publishedDigests []string
}

func (plan userPluginPlan) hasChanges() bool {
	return len(plan.installs) != 0 || len(plan.retirements) != 0
}

func (node *Node) initializeUserPlugins(ctx context.Context, user protocol.User, startup bool) error {
	ctx = node.pluginContext(ctx)
	plan, err := node.prepareUserPlugins(ctx, user, startup)
	if err != nil {
		return err
	}
	summaries := plan.summaries
	if plan.hasChanges() {
		if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			var err error
			summaries, err = applyUserPluginPlan(ctx, db, plan)
			return err
		}); err != nil {
			return err
		}
	}
	return node.observeUserPluginInventory(user, summaries)
}

func (node *Node) prepareUserPlugins(ctx context.Context, user protocol.User, startup bool) (userPluginPlan, error) {
	plan := userPluginPlan{user: user}
	userCtx := store.WithCapturedUser(store.WithUser(node.pluginContext(ctx), user), user.ID)
	var infos []protocol.PluginInfo
	var recorded map[string]bool
	if err := node.profile.Store.Node.Read(userCtx, func(db *sql.Conn) error {
		var err error
		infos, err = store.ReadPluginInfos(userCtx, db)
		if err != nil {
			return err
		}
		recorded, err = store.RecordedPluginNames(userCtx, db)
		return err
	}); err != nil {
		return plan, err
	}
	installed := make(map[string]protocol.PluginInfo, len(infos))
	plan.summaries = make([]protocol.PluginSummary, 0, len(infos))
	for _, info := range infos {
		installed[info.Summary.Name] = info
		plan.summaries = append(plan.summaries, info.Summary)
	}
	defaults := node.modules.BundledDefaults
	if defaults == nil {
		defaults = plugins.BundledDefaults()
	}
	defaultNames := make(map[string]bool, len(defaults))
	for _, entry := range defaults {
		defaultNames[entry.Name] = true
		current, hasCurrent := installed[entry.Name]
		if hasCurrent && current.Summary.Pinned {
			continue
		}
		if recorded[entry.Name] && !hasCurrent || hasCurrent && (current.Origin == nil || current.Origin.Kind != "bundled") {
			continue
		}
		inspected, err := node.plugins.manager.InspectBundled(userCtx, entry.Name)
		if err != nil {
			if !startup || userCtx.Err() != nil {
				return plan, err
			}
			logUnavailablePackage(protocol.PluginReference{Name: entry.Name}, "bundled plugin inspection failed", err)
			continue
		}
		if plugins.IsWithdrawn(inspected) || inspected.Summary.Compatibility.Kind != "compatible" {
			continue
		}
		if hasCurrent && !plugins.IsNewer(current.Summary.Version, inspected.Summary.Version) {
			continue
		}
		var info protocol.PluginInfo
		if startup {
			info, err = node.plugins.manager.RestoreBundled(userCtx, plugins.PackageReference(inspected))
		} else {
			info, err = node.plugins.manager.InstallBundled(userCtx, entry.Name)
		}
		if err != nil {
			if !startup || userCtx.Err() != nil {
				return plan, err
			}
			logUnavailablePackage(plugins.PackageReference(inspected), "bundled plugin installation failed", err)
			continue
		}
		revision := uint64(0)
		enabled := entry.Enabled
		if hasCurrent {
			revision = current.Summary.Revision
			enabled = current.Summary.Enabled
		}
		plan.installs = append(plan.installs, pendingPluginInstall{info: info, revision: revision, enabled: enabled})
		plan.publishedDigests = append(plan.publishedDigests, info.Summary.Digest)
	}
	retired := plugins.RetiredBundled()
	retiredNames := make(map[string]bool, len(retired))
	for _, name := range retired {
		retiredNames[name] = true
		if current, hasCurrent := installed[name]; hasCurrent && current.Origin != nil && current.Origin.Kind == "bundled" {
			plan.retirements = append(plan.retirements, name)
		}
	}
	// Upgrade already-installed unpinned optional bundled packages (for example,
	// packages removed from BundledDefaults) without installing them on new profiles.
	for _, current := range infos {
		name := current.Summary.Name
		if current.Summary.Pinned || retiredNames[name] || defaultNames[name] {
			continue
		}
		if current.Origin == nil || current.Origin.Kind != "bundled" {
			continue
		}
		inspected, err := node.plugins.manager.InspectBundled(userCtx, name)
		if err != nil {
			if !startup || userCtx.Err() != nil {
				continue
			}
			continue
		}
		if plugins.IsWithdrawn(inspected) || inspected.Summary.Compatibility.Kind != "compatible" || !plugins.IsNewer(current.Summary.Version, inspected.Summary.Version) {
			continue
		}
		var info protocol.PluginInfo
		if startup {
			info, err = node.plugins.manager.RestoreBundled(userCtx, plugins.PackageReference(inspected))
		} else {
			info, err = node.plugins.manager.InstallBundled(userCtx, name)
		}
		if err != nil {
			if !startup || userCtx.Err() != nil {
				return plan, err
			}
			logUnavailablePackage(plugins.PackageReference(inspected), "bundled plugin upgrade failed", err)
			continue
		}
		plan.installs = append(plan.installs, pendingPluginInstall{info: info, revision: current.Summary.Revision, enabled: current.Summary.Enabled})
		plan.publishedDigests = append(plan.publishedDigests, info.Summary.Digest)
	}
	return plan, nil
}

func applyUserPluginPlan(ctx context.Context, db *sql.Conn, plan userPluginPlan) ([]protocol.PluginSummary, error) {
	userCtx := store.WithCapturedUser(store.WithUser(ctx, plan.user), plan.user.ID)
	for _, item := range plan.installs {
		if _, err := store.InstallPlugin(userCtx, db, item.info, item.revision, item.enabled); err != nil {
			return nil, err
		}
	}
	for _, name := range plan.retirements {
		current, err := store.ReadPlugin(userCtx, db, name)
		if err != nil {
			return nil, err
		}
		if current == nil || current.Origin == nil || current.Origin.Kind != "bundled" {
			continue
		}
		// Preserve explicit user enable/disable choices when retiring a
		// bundled plugin into its native module equivalent.
		switch name {
		case "computer":
			if current.Summary.Enabled {
				if cfg, cfgErr := store.ReadComputerSettings(userCtx, db); cfgErr == nil && cfg.Revision == 0 {
					if _, _, saveErr := store.SaveComputerSettings(userCtx, db, protocol.ComputerSettings{Revision: 0, Enabled: true}); saveErr != nil {
						return nil, saveErr
					}
				}
			}
		case "external-browser":
			if current.Summary.Enabled {
				if cfg, cfgErr := store.ReadExternalBrowserSettings(userCtx, db); cfgErr == nil && cfg.Revision == 0 {
					if _, _, saveErr := store.SaveExternalBrowserSettings(userCtx, db, protocol.ExternalBrowserSettings{Revision: 0, Enabled: true}); saveErr != nil {
						return nil, saveErr
					}
				}
			}
		case "memory":
			if !current.Summary.Enabled {
				if migErr := store.MigrateRetiredMemoryEnabled(userCtx, db, false); migErr != nil {
					return nil, migErr
				}
			}
		}
		if _, err := store.RemovePlugin(userCtx, db, name, current.Summary.Revision); err != nil {
			return nil, err
		}
	}
	return store.ReadPlugins(userCtx, db)
}

func (node *Node) observeUserPluginInventory(user protocol.User, summaries []protocol.PluginSummary) error {
	for _, summary := range summaries {
		if err := node.plugins.mcp.Observe(summary, user.ID.String()); err != nil {
			return err
		}
	}
	node.plugins.mcp.FinishInventory(user.ID.String())
	return nil
}

func (node *Node) pluginUnavailable(digest string) bool {
	node.plugins.mu.Lock()
	defer node.plugins.mu.Unlock()
	_, unavailable := node.plugins.unavailable[digest]
	return unavailable
}

func (node *Node) checkPluginAvailability(reference protocol.PluginReference) error {
	if node.pluginUnavailable(reference.Digest) {
		return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "plugin package is unavailable: " + reference.Name}
	}
	return nil
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
			previous, err := admission.Existing(ctx, db, caller, request, body)
			if err != nil {
				return err
			}
			if previous != nil {
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
	if request.Plugin.Package == store.NativeMemoryReference() {
		if caller != node.ID() || request.Plugin.Invocation != nil {
			return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "native memory storage is internal to this Node"}
		}
		if legacy, err := store.ReadPlugin(ctx, db, store.NativeMemoryNamespace); err != nil {
			return err
		} else if legacy != nil {
			return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "memory storage is owned by the installed memory plugin"}
		}
		info := store.NativeMemoryStorageInfo()
		return store.CheckPluginRequest(ctx, db, request, info)
	}
	if request.Plugin.Surface.Kind == "card" {
		if err := store.CheckPluginCardRequest(ctx, db, node.ID(), caller, request); err != nil {
			return err
		}
		return node.checkPluginAvailability(request.Plugin.Package)
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
			actor, actorErr := store.CurrentUser(ctx, db)
			owner, ownerErr := store.ResolveUser(ctx, db, invocation.user.ID)
			if actorErr != nil || ownerErr != nil || actor.ID != owner.ID {
				node.plugins.mu.Unlock()
				return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "plugin invocation belongs to another user"}
			}
			ctx = store.WithCapturedUser(ctx, invocation.storageUser)
			value := invocation.info
			value.Summary.Enabled = true
			captured = &value
		}
		node.plugins.mu.Unlock()
		if captured == nil {
			return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "plugin invocation is unavailable"}
		}
	}
	if _, err := store.AuthorizePluginRequest(ctx, db, node.ID(), caller, request, captured); err != nil {
		return err
	}
	return node.checkPluginAvailability(request.Plugin.Package)
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
	var user protocol.User
	var storageUser protocol.UserID
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var captureErr error
		user, captureErr = store.CurrentUser(ctx, db)
		if captureErr != nil {
			return captureErr
		}
		storageUser, captureErr = store.CapturedUser(ctx, db)
		if captureErr != nil {
			return captureErr
		}
		var captured *protocol.PluginInfo
		if request.Plugin.Invocation != nil {
			node.plugins.mu.Lock()
			invocation := node.plugins.invocations[*request.Plugin.Invocation]
			if invocation != nil && caller == node.ID() && reflect.DeepEqual(invocation.scope, *request.Plugin) {
				owner, ownerErr := store.ResolveUser(ctx, db, invocation.user.ID)
				if ownerErr != nil || owner.ID != user.ID {
					node.plugins.mu.Unlock()
					return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "plugin invocation belongs to another user"}
				}
				storageUser = invocation.storageUser
				ctx = store.WithCapturedUser(ctx, storageUser)
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
	return node.runPluginHandler(ctx, caller, request, *request.Plugin, info, user, storageUser, handler, input, readOnly)
}

func (node *Node) runPluginHandler(ctx context.Context, caller protocol.NodeID, request protocol.Request, scope protocol.PluginContext, info protocol.PluginInfo, user protocol.User, storageUser protocol.UserID, handler string, input any, readOnly bool) (protocol.Output, error) {
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
	files, err := node.plugins.manager.ReadBundle(ctx, scope.Package, host.Entry, host.Resources)
	if err != nil {
		return protocol.Output{}, err
	}
	ctx = store.WithCapturedUser(store.WithUser(ctx, user), storageUser)
	operation, cancel := context.WithCancel(ctx)
	defer cancel()
	identity := protocol.NewRequestID()
	scope.Invocation = &identity
	invocation := &pluginInvocation{caller: caller, request: request.ID, user: user, storageUser: storageUser, info: info, scope: scope, cancel: cancel, readOnly: readOnly}
	node.plugins.mu.Lock()
	if len(node.plugins.invocations) >= 64 {
		node.plugins.mu.Unlock()
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "plugin callback workers are busy"}
	}
	node.plugins.invocations[identity] = invocation
	node.plugins.mu.Unlock()
	defer func() { node.plugins.mu.Lock(); delete(node.plugins.invocations, identity); node.plugins.mu.Unlock() }()
	operation = node.plugins.memory.Context(operation, scope.Package, "callback", pluginMemoryUser(operation))
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
	operation, cancel := context.WithCancel(context.WithoutCancel(ctx))
	stopOwner := context.AfterFunc(owner, cancel)
	stop := context.AfterFunc(ctx, cancel)
	return operation, func() { stop(); stopOwner(); cancel(); finish() }, nil
}

func (node *Node) pluginCommitted(changes []protocol.Event, ctx context.Context, preservePrograms map[string]bool) {
	user, err := node.pluginRuntimeUser(ctx)
	if err != nil {
		return
	}
	packagesChanged := false
	for _, change := range changes {
		if change.Kind == "plugin_changed" {
			packagesChanged = true
			if summary, ok := dispatchPayload[protocol.PluginSummary](change.Data); ok {
				_ = node.plugins.mcp.Observe(summary, user.String())
				// Captured launch identities already separate old and new code.
				// A replacement must not kill a program serving an admitted turn;
				// its existing lease and idle expiry still bound its lifetime.
				if !preservePrograms[summary.Name] || !summary.Enabled || summary.Compatibility.Kind != "compatible" {
					node.plugins.programs.Invalidate(pluginProgramOwner(user, summary.Name))
				}
				node.retainComputerOverlay(summary.Name, ctx)
			}
		}
		if change.Kind == "plugin_removed" {
			packagesChanged = true
			if removed, ok := dispatchPayload[protocol.EventPluginRemovedPayload](change.Data); ok {
				_ = node.plugins.mcp.Invalidate(removed.Name, user.String())
				node.plugins.programs.Invalidate(pluginProgramOwner(user, removed.Name))
				node.retainComputerOverlay(removed.Name, ctx)
			}
		}
	}
	if packagesChanged && node.plugins.python != nil {
		_ = node.retainPackagePython(context.WithoutCancel(ctx), false)
	}
	if packagesChanged {
		node.authChanged()
	}
	for _, change := range changes {
		if store.RuntimeCredentialEvent(change.Kind) {
			node.RuntimeCredentialsChanged()
		}
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
