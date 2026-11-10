package live

import (
	"context"
	"encoding/json"
	"errors"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginIntentResult struct {
	ready bool
	value bool
	err   error
}

func (r *Runtime) pluginIntent(ctx context.Context, scope protocol.PluginContext, presentation string, intent string, value *json.RawMessage) (bool, error) {
	if scope.Surface.Kind != "project" && scope.Surface.Kind != "provider" {
		scope.Surface = protocol.PluginDesktopSurface{Kind: "composer"}
	}
	root := r.localRuntime()
	var captured *pluginContributionOwner
	var draft string
	clock := clockwork.NewRealClock()
	deadline := clock.NewTimer(10 * time.Second)
	defer deadline.Stop()
	for {
		reply := make(chan pluginIntentResult, 1)
		if !root.enqueuePluginIntent(func(*ui.Context) {
			if ctx.Err() != nil {
				reply <- pluginIntentResult{err: ctx.Err()}
				return
			}
			if !pluginViewScopeExists(r, scope) {
				reply <- pluginIntentResult{err: errors.New("contribution context changed")}
				return
			}
			registry := r.pluginRegistry(scope, presentation)
			var owner *pluginContributionOwner
			for _, current := range root.pluginViews.registries {
				if current.registry == registry {
					owner = current
					break
				}
			}
			if owner == nil || registry == nil {
				reply <- pluginIntentResult{err: errors.New("contribution window is closed")}
				return
			}
			if !rootPluginRegistryCurrent(root, owner) {
				root.closePluginRegistry(owner)
				reply <- pluginIntentResult{err: errors.New("contribution context changed")}
				return
			}
			if captured == nil {
				captured, draft = owner, owner.draft
			} else if captured != owner || owner.draft != draft {
				reply <- pluginIntentResult{err: errors.New("contribution context changed")}
				return
			}
			reply <- r.tryPluginIntent(scope, registry, intent, value)
		}) {
			return false, errors.New("contribution window is closed")
		}
		select {
		case <-ctx.Done():
			return false, ctx.Err()
		case <-r.ctx.Done():
			return false, errors.New("contribution window is closed")
		case <-deadline.Chan():
			return false, errors.New("contribution did not become ready")
		case result := <-reply:
			if result.err != nil || result.ready {
				return result.value, result.err
			}
		}
		poll := clock.NewTimer(20 * time.Millisecond)
		select {
		case <-ctx.Done():
			poll.Stop()
			return false, ctx.Err()
		case <-r.ctx.Done():
			poll.Stop()
			return false, errors.New("contribution window is closed")
		case <-deadline.Chan():
			poll.Stop()
			return false, errors.New("contribution did not become ready")
		case <-poll.Chan():
		}
	}
}

func (r *Runtime) enqueuePluginIntent(operation func(*ui.Context)) bool {
	r.mu.Lock()
	if r.closed {
		r.mu.Unlock()
		return false
	}
	r.queue = append(r.queue, operation)
	r.mu.Unlock()
	r.wake()
	return true
}

func (r *Runtime) pluginCatalogSettled() bool {
	if r.packages == nil {
		return false
	}
	for _, summary := range r.controller.Snapshot().Plugins {
		version := packageVersion(summary)
		info, exists := r.packages.infos[summary.Name]
		if (!exists || packageVersion(info.Summary) != version) && r.packages.failures[summary.Name] != version {
			return false
		}
	}
	return true
}

func (r *Runtime) tryPluginIntent(scope protocol.PluginContext, registry *plugins.Registry, intent string, value *json.RawMessage) pluginIntentResult {
	if scope.Surface.Kind != "provider" && scope.Surface.Kind != "project" {
		feed := r.statusFeed(protocol.PluginUiStatusScope{Worktree: scope.Worktree, Session: scope.Session})
		if feed != nil {
			for _, entry := range feed.view.Entries {
				if entry.Declaration.Intent == nil || *entry.Declaration.Intent != intent || unmountedPlugin(entry.Package.Name) {
					continue
				}
				key, exists := registry.Key(entry.Package, entry.Declaration.ID)
				_, mounted := r.pluginContribution(key)
				active := exists && mounted != nil && mounted.mounted != nil && mounted.failure == nil
				ready := entry.Declaration.Kind == "button" && entry.Invoke != nil || active && registry.Ready(entry.Package)
				if !feed.view.Connected || !ready && (mounted == nil || mounted.failure == nil) {
					return pluginIntentResult{}
				}
				available := ready && entry.State.Enabled
				if pending := feed.pending[pluginStatusIdentity(entry)]; pending != nil {
					available = !pending.active
				}
				if value == nil {
					return pluginIntentResult{ready: true, value: available}
				}
				if !available {
					return pluginIntentResult{err: errors.New("status contribution is unavailable")}
				}
				if entry.Declaration.Kind == "button" && entry.Invoke != nil {
					if string(*value) != "null" || !entry.State.Visible || !r.invokePluginStatus(scope, pluginStatusIdentity(entry)) {
						return pluginIntentResult{err: errors.New("invalid status interaction")}
					}
					return pluginIntentResult{ready: true, value: true}
				}
				body := *value
				if entry.Declaration.Kind == "popover" && string(body) == "true" {
					body = json.RawMessage("null")
				}
				if err := registry.ForwardStatus(key, "invoke", body); err != nil {
					return pluginIntentResult{err: err}
				}
				if entry.Declaration.Kind == "popover" {
					feed.open[pluginStatusIdentity(entry)] = true
				}
				r.localRuntime().wake()
				return pluginIntentResult{ready: true, value: true}
			}
			for _, info := range r.localRuntime().pluginViewCatalog(r) {
				if info.Extension == nil || !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" || unmountedPlugin(info.Summary.Name) {
					continue
				}
				for _, declaration := range info.Extension.Ui {
					if declaration.Slot == "status" && declaration.Intent != nil && *declaration.Intent == intent {
						if !feed.view.Connected || !r.pluginCatalogSettled() {
							return pluginIntentResult{}
						}
						if value == nil {
							return pluginIntentResult{ready: true}
						}
						return pluginIntentResult{err: errors.New("contribution is unavailable")}
					}
				}
			}
		}
	}
	declared, ready := registry.IntentReady(intent)
	if declared && !ready || !declared && !r.pluginCatalogSettled() {
		return pluginIntentResult{}
	}
	entry, available := registry.Intent(intent)
	if value == nil {
		return pluginIntentResult{ready: true, value: available}
	}
	if !available {
		return pluginIntentResult{err: errors.New("contribution is unavailable")}
	}
	if err := registry.InvokeIntent(entry.Key, *value); err != nil {
		return pluginIntentResult{err: err}
	}
	r.localRuntime().wake()
	return pluginIntentResult{ready: true, value: true}
}

func (r *Runtime) pluginCommandIntent(scope protocol.PluginContext, presentation string, output protocol.Output) {
	if output.Kind != "plugin_result" {
		return
	}
	// A plugin result is the callback's own JSON value, not a protocol payload.
	var result struct {
		Intent string `json:"ui_intent"`
	}
	body, err := json.Marshal(output.Data)
	if err != nil || json.Unmarshal(body, &result) != nil || result.Intent == "" {
		return
	}
	intent := result.Intent
	r.async(func() {
		body := json.RawMessage("true")
		_, _ = r.pluginIntent(r.ctx, scope, presentation, intent, &body)
	})
}
