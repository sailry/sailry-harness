package live

import (
	"context"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func permissionContributions(declarations []shell.PermissionContribution) []shell.PermissionContribution {
	var sources []shell.PermissionContribution
	for _, source := range declarations {
		if source.Subject == "" || source.Subject == permissionApp || source.Check == nil || slices.ContainsFunc(sources, func(existing shell.PermissionContribution) bool {
			return existing.Subject == source.Subject
		}) {
			continue
		}
		var kinds []protocol.PermissionsKind
		for _, kind := range source.Kinds {
			if _, known := permissionKinds[kind]; known && !slices.Contains(kinds, kind) {
				kinds = append(kinds, kind)
			}
		}
		if len(kinds) != 0 {
			source.Kinds = kinds
			sources = append(sources, source)
		}
	}
	return sources
}

type permissionScope struct {
	ctx       context.Context
	cancel    context.CancelFunc
	owner     *Runtime
	connected bool
	native    bool
	sources   []permissionSourceRead
}

type permissionSourceRead struct {
	revision  uint64
	available bool
	loaded    bool
	failed    bool
	cancel    context.CancelFunc
}

func (read *permissionSourceRead) clear() {
	if read.cancel != nil {
		read.cancel()
	}
	*read = permissionSourceRead{}
}

func (r *Runtime) permissionsVisible() bool {
	return r.ctx.Err() == nil && r.App.Page == shell.SettingsPage && r.App.Settings.Section == settings.Permissions && !r.App.DemoOn()
}

func (r *Runtime) permissionHostState(owner *Runtime) (connected, native bool) {
	connected = owner != nil && owner.controller.Connected()
	return connected, connected && len(r.permissions.contributions) != 0 && owner.machineSettingsStatus() == ""
}

func (r *Runtime) permissionScopeCurrent(scope *permissionScope) bool {
	if scope == nil || scope.ctx.Err() != nil || r.permissions.scope != scope || !r.permissionsVisible() {
		return false
	}
	owner := r.managedSettingsNode()
	connected, native := r.permissionHostState(owner)
	return owner == scope.owner && connected == scope.connected && native == scope.native
}

func (state *permissionState) endScope() {
	if state.scope != nil {
		state.scope.cancel()
		state.scope = nil
	}
	state.endWork()
}

func (state *permissionState) endWork() {
	if state.workCancel != nil {
		state.workCancel()
		state.workCancel = nil
	}
	state.checking, state.pending, state.failed = false, false, false
}

func (r *Runtime) syncPermissionScope(owner *Runtime) (scope *permissionScope, changed bool) {
	state := &r.permissions
	connected, native := r.permissionHostState(owner)
	if current := state.scope; current != nil && owner == current.owner && connected == current.connected && native == current.native {
		return current, false
	}
	state.endScope()
	ctx, cancel := context.WithCancel(r.ctx)
	scope = &permissionScope{ctx: ctx, cancel: cancel, owner: owner, connected: connected, native: native}
	if native {
		scope.sources = make([]permissionSourceRead, len(state.contributions))
	}
	state.scope = scope
	return scope, true
}

func (r *Runtime) retryPermissionContributions(owner *Runtime) {
	scope := r.permissions.scope
	if !r.permissionScopeCurrent(scope) || scope.owner != owner {
		return
	}
	for i := range scope.sources {
		scope.sources[i].failed = false
	}
}

func (r *Runtime) nativePermissionEntries(scope *permissionScope, entries []permissionEntry) []permissionEntry {
	for i := range scope.sources {
		source, read := &r.permissions.contributions[i], &scope.sources[i]
		if slices.ContainsFunc(entries, func(entry permissionEntry) bool { return entry.declaration.Subject == source.Subject }) {
			read.clear()
			continue
		}
		if source.Available != nil && !r.permissionSourceAvailable(scope, source, read) {
			continue
		}
		for _, kind := range source.Kinds {
			entries = append(entries, permissionEntry{owner: scope.owner, native: source, declaration: protocol.PermissionsContribution{
				Subject: source.Subject, Kind: kind,
			}})
		}
	}
	return entries
}

func permissionSourceRevision(owner *Runtime, source *shell.PermissionContribution) uint64 {
	if source.Revision == nil {
		return 0
	}
	snapshot := owner.snapshot()
	return source.Revision(&snapshot)
}

func (r *Runtime) permissionSourceAvailable(scope *permissionScope, source *shell.PermissionContribution, read *permissionSourceRead) bool {
	revision := permissionSourceRevision(scope.owner, source)
	if read.revision != revision {
		previous := read.available
		read.clear()
		read.revision, read.available = revision, previous
	}
	if read.loaded || read.failed || read.cancel != nil {
		return read.available
	}
	ctx, cancel := context.WithCancel(scope.ctx)
	read.cancel = cancel
	if !r.async(func() {
		if ctx.Err() != nil {
			return
		}
		available, err := source.Available(ctx, scope.owner.controller.Do)
		r.enqueue(func(*ui.Context) {
			if ctx.Err() != nil || !r.permissionScopeCurrent(scope) {
				return
			}
			read.cancel = nil
			cancel()
			if permissionSourceRevision(scope.owner, source) != revision {
				return
			}
			read.failed = err != nil
			if err == nil {
				read.available, read.loaded = available, true
			}
		})
	}) {
		read.cancel = nil
		cancel()
	}
	return read.available
}

func (r *Runtime) nativePermission(ctx context.Context, entry permissionEntry, request bool) ([]permissionChange, error) {
	source := entry.native
	execute := entry.owner.controller.Do
	report, err := source.Check(ctx, execute)
	if err != nil {
		return nil, err
	}
	kind := entry.declaration.Kind
	grant := report.Grants[kind]
	if request && ctx.Err() == nil && entry.owner == r.localRuntime() && source.Request != nil && report.CanRequest &&
		(grant == shell.PermissionRequired || grant == shell.PermissionUnknown) {
		report, err = source.Request(ctx, execute, kind)
		if err != nil {
			return nil, err
		}
	}
	changes := make([]permissionChange, 0, len(source.Kinds))
	for _, kind := range source.Kinds {
		status := report.Grants[kind]
		switch status {
		case shell.PermissionGranted, shell.PermissionRequired, shell.PermissionNotNeeded, shell.PermissionUnavailable:
		default:
			status = shell.PermissionUnknown
		}
		changes = append(changes, permissionChange{kind, string(status)})
	}
	return changes, nil
}
