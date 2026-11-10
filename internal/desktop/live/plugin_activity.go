package live

import (
	"context"
	"encoding/json"
	"reflect"
	"strconv"
	"sync"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginActivityHost struct {
	Node  string `json:"node"`
	Label string `json:"label"`
}

type pluginActivityHosts struct {
	Hosts           []pluginActivityHost  `json:"hosts"`
	UnreadTerminals []protocol.TerminalID `json:"unread_terminals"`
	Visible         bool                  `json:"visible"`
}

type pluginActivityState struct {
	root       *Runtime
	mounted    *pluginMountedView
	mu         sync.Mutex
	visible    bool
	page       context.Context
	cancelPage context.CancelFunc
	changed    chan struct{}
}

func (r *Runtime) bindPluginActivity(mounted *pluginMountedView, native *plugins.NativeBinding) func(bool) {
	state := &pluginActivityState{root: r.localRuntime(), mounted: mounted, changed: make(chan struct{})}
	native.Operations["sailry/sdk.readActivityCatalog"] = state.readCatalog
	native.Operations["sailry/sdk.readActivityPreviews"] = state.readPreviews
	native.Operations["sailry/sdk.nextActivityHosts"] = state.nextHosts
	return state.setVisible
}

func (state *pluginActivityState) check(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := state.mounted.ctx.Err(); err != nil {
		return err
	}
	scope := state.mounted.scope
	if scope.Surface.Kind != "workspace" || scope.Worktree != nil || scope.Session != nil || scope.Turn != nil || scope.Invocation != nil {
		return pluginNativeFault(protocol.ErrorCodeInvalidRequest, "activity requires an unscoped workspace view")
	}
	return nil
}

func (state *pluginActivityState) setVisible(visible bool) {
	state.mu.Lock()
	defer state.mu.Unlock()
	if state.visible == visible {
		return
	}
	state.visible = visible
	if state.cancelPage != nil {
		state.cancelPage()
		state.page, state.cancelPage = nil, nil
	}
	if visible {
		state.page, state.cancelPage = context.WithCancel(state.mounted.ctx)
	}
	close(state.changed)
	state.changed = make(chan struct{})
}

func (state *pluginActivityState) hosts() (pluginActivityHosts, <-chan struct{}) {
	state.mu.Lock()
	value := pluginActivityHosts{
		Hosts: []pluginActivityHost{}, UnreadTerminals: []protocol.TerminalID{}, Visible: state.visible,
	}
	changed := state.changed
	state.mu.Unlock()
	for _, source := range state.root.pluginUsageSources() {
		if source.Client == nil {
			continue
		}
		value.Hosts = append(value.Hosts, pluginActivityHost{Node: source.Client.Target().String(), Label: source.Label})
		if source.Client.Target() == state.mounted.owner.controller.Target() {
			value.UnreadTerminals = source.Client.UnreadTerminals()
		}
	}
	return value, changed
}

func (state *pluginActivityState) catalog(ctx context.Context) (protocol.ActivityCatalog, error) {
	if err := state.check(ctx); err != nil {
		return protocol.ActivityCatalog{}, err
	}
	request, cancel := context.WithCancel(ctx)
	release := context.AfterFunc(state.mounted.ctx, cancel)
	defer release()
	defer cancel()
	return state.mounted.owner.controller.ReadActivityCatalog(request, state.mounted.scope)
}

func (state *pluginActivityState) readCatalog(ctx context.Context, _ []json.RawMessage) (json.RawMessage, error) {
	catalog, err := state.catalog(ctx)
	if err != nil {
		return nil, err
	}
	hosts, _ := state.hosts()
	projection := client.ProjectActivity(catalog, hosts.UnreadTerminals)
	return pluginNativeJSON(struct {
		Node      string                  `json:"node"`
		Cursor    string                  `json:"cursor"`
		Projects  []protocol.Project      `json:"projects"`
		Worktrees []protocol.Worktree     `json:"worktrees"`
		Sessions  []protocol.Session      `json:"sessions"`
		Terminals []protocol.TerminalInfo `json:"terminals"`
		client.ActivityProjection
		Hosts           []pluginActivityHost  `json:"hosts"`
		UnreadTerminals []protocol.TerminalID `json:"unread_terminals"`
	}{
		Node: catalog.Node.String(), Cursor: strconv.FormatUint(catalog.Cursor, 10),
		Projects: catalog.Projects, Worktrees: catalog.Worktrees, Sessions: catalog.Sessions, Terminals: catalog.Terminals,
		ActivityProjection: projection, Hosts: hosts.Hosts, UnreadTerminals: hosts.UnreadTerminals,
	})
}

func (state *pluginActivityState) readPreviews(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	if err := state.check(ctx); err != nil {
		return nil, err
	}
	sessions, err := pluginNativeArgument[[]protocol.SessionID](arguments, 0)
	if err != nil {
		return nil, err
	}
	state.mu.Lock()
	page, visible := state.page, state.visible
	state.mu.Unlock()
	if !visible || page == nil || page.Err() != nil {
		return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "activity page is hidden")
	}
	request, cancel := context.WithCancel(ctx)
	release := context.AfterFunc(page, cancel)
	defer release()
	defer cancel()
	previews, err := state.mounted.owner.controller.ReadActivityPreviews(request, state.mounted.scope, sessions)
	if failure := ctx.Err(); failure != nil {
		return nil, failure
	}
	if failure := state.mounted.ctx.Err(); failure != nil {
		return nil, failure
	}
	if page.Err() != nil {
		return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "activity page is hidden")
	}
	if err != nil {
		return nil, err
	}
	return pluginNativeJSON(previews)
}

func (state *pluginActivityState) nextHosts(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	if err := state.check(ctx); err != nil {
		return nil, err
	}
	seen, err := pluginNativeArgument[pluginActivityHosts](arguments, 0)
	if err != nil {
		return nil, err
	}
	watch, cancel := context.WithCancel(ctx)
	defer cancel()
	sources := state.root.pluginUsageChanges(watch)
	for {
		current, changed := state.hosts()
		if !reflect.DeepEqual(current, seen) {
			if _, err := state.catalog(ctx); err != nil {
				return nil, err
			}
			return pluginNativeJSON(current)
		}
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-state.mounted.ctx.Done():
			return nil, state.mounted.ctx.Err()
		case <-changed:
		case <-sources:
		}
	}
}
