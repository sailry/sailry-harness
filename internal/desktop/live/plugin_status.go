package live

import (
	"context"
	"encoding/json"
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginStatusView struct {
	Entries   []protocol.PluginUiStatusEntry   `json:"entries"`
	Failures  []protocol.PluginUiStatusFailure `json:"failures"`
	Connected bool                             `json:"connected"`
	Error     *protocol.Fault                  `json:"error"`
}

type pluginStatusPending struct {
	request protocol.Request
	active  bool
}

type pluginStatusFeed struct {
	scope      protocol.PluginUiStatusScope
	view       pluginStatusView
	open       map[string]bool
	pending    map[string]*pluginStatusPending
	cancel     context.CancelFunc
	generation uint64
	seen       bool
}

func pluginStatusIdentity(entry protocol.PluginUiStatusEntry) string {
	return entry.Package.Name + "/" + entry.Package.Digest + "/" + strconv.FormatUint(entry.Package.SettingsRevision, 10) + "/" + entry.Declaration.ID
}

func (r *Runtime) statusFeed(scope protocol.PluginUiStatusScope) *pluginStatusFeed {
	if !scope.Valid() {
		return nil
	}
	body, _ := json.Marshal(scope)
	key := string(body)
	if r.pluginStatus == nil {
		r.pluginStatus = map[string]*pluginStatusFeed{}
	}
	feed := r.pluginStatus[key]
	if feed == nil {
		feed = &pluginStatusFeed{scope: scope, open: map[string]bool{}, pending: map[string]*pluginStatusPending{}}
		r.pluginStatus[key] = feed
	}
	feed.seen = true
	if feed.cancel == nil {
		ctx, cancel := context.WithCancel(r.ctx)
		feed.cancel = cancel
		feed.generation++
		generation := feed.generation
		r.observe(func() {
			observer, err := r.controller.Watch(ctx, "watch_status", body)
			if err != nil {
				r.enqueue(func(*ui.Context) {
					if feed.generation == generation {
						feed.view.Connected = false
					}
				})
				return
			}
			defer observer.Close()
			for {
				value, err := observer.Next(ctx)
				if err != nil {
					r.enqueue(func(*ui.Context) {
						if feed.generation == generation {
							feed.view.Connected = false
						}
					})
					return
				}
				var view pluginStatusView
				if json.Unmarshal(value, &view) != nil {
					continue
				}
				r.enqueue(func(*ui.Context) {
					if feed.generation != generation {
						return
					}
					feed.view = view
					keep := make(map[string]bool, len(view.Entries))
					for _, entry := range view.Entries {
						keep[pluginStatusIdentity(entry)] = true
					}
					for id := range feed.open {
						if !keep[id] {
							delete(feed.open, id)
						}
					}
					for id := range feed.pending {
						if !keep[id] {
							delete(feed.pending, id)
						}
					}
				})
			}
		})
	}
	return feed
}

func (r *Runtime) pluginStatusAfterFrame() {
	for key, feed := range r.pluginStatus {
		if !pluginViewScopeExists(r, protocol.PluginContext{Worktree: feed.scope.Worktree, Session: feed.scope.Session}) {
			if feed.cancel != nil {
				feed.cancel()
			}
			feed.generation++
			delete(r.pluginStatus, key)
			continue
		}
		if !feed.seen && feed.cancel != nil {
			feed.cancel()
			feed.cancel = nil
			feed.generation++
			feed.view.Connected = false
		}
		feed.seen = false
	}
}

func (r *Runtime) closePluginStatus() {
	for _, feed := range r.pluginStatus {
		if feed.cancel != nil {
			feed.cancel()
		}
		feed.generation++
	}
	r.pluginStatus = nil
}

func (r *Runtime) projectPluginStatus(comp *composer.State, context *composer.Context, scope protocol.PluginContext, registry *plugins.Registry) {
	feed := r.statusFeed(protocol.PluginUiStatusScope{Worktree: scope.Worktree, Session: scope.Session})
	context.Activity.Badges = nil
	if feed == nil {
		comp.SetStatusCallbacks(nil)
		return
	}
	sources := map[string]protocol.PluginUiStatusEntry{}
	enabled := map[string]bool{}
	keys := map[string]plugins.ContributionKey{}
	closedPackages := map[protocol.PluginReference]bool{}
	for _, entry := range feed.view.Entries {
		if unmountedPlugin(entry.Package.Name) {
			continue
		}
		id := pluginStatusIdentity(entry)
		if registry != nil {
			if key, ok := registry.Key(entry.Package, entry.Declaration.ID); ok {
				keys[id] = key
			}
			if !closedPackages[entry.Package] {
				closedPackages[entry.Package] = true
				for _, closed := range registry.TakeClosed(entry.Package) {
					captured := entry
					captured.Declaration.ID = closed
					delete(feed.open, pluginStatusIdentity(captured))
				}
			}
		}
		pending := feed.pending[id]
		visible := entry.State.Visible || feed.open[id] || pending != nil
		if !visible {
			continue
		}
		ready := entry.Declaration.Kind == "button" && entry.Invoke != nil ||
			registry != nil && registry.Ready(entry.Package)
		available := entry.State.Enabled
		if pending != nil {
			available = !pending.active
		}
		enabled[id] = feed.view.Connected && ready && available
		sources[id] = entry
		label := entry.Declaration.Label
		if entry.State.Label != nil {
			label = *entry.State.Label
		}
		badge := composer.StatusBadge{ID: id, Label: composer.Text{Label: label.Label, Locales: label.Locales}, Icon: icons.Settings2}
		name := entry.Declaration.Icon
		if entry.State.Icon != nil {
			name = entry.State.Icon
		}
		if name != nil {
			if icon, err := icons.Plugin(*name); err == nil {
				badge.Icon = icon
			}
		}
		for _, segment := range entry.State.Segments {
			tone := kit.Neutral
			switch segment.Tone {
			case "accent":
				tone = kit.Info
			case "success":
				tone = kit.Positive
			case "warning":
				tone = kit.Caution
			case "danger":
				tone = kit.Negative
			}
			badge.Segment = append(badge.Segment, composer.Segment{
				Text: composer.Text{Label: segment.Label.Label, Locales: segment.Label.Locales}, Tone: tone,
			})
		}
		for _, detail := range entry.State.Details {
			badge.Details = append(badge.Details, composer.Detail{
				Label: composer.Text{Label: detail.Label.Label, Locales: detail.Label.Locales}, Value: detail.Value,
			})
		}
		if entry.Declaration.Kind == "popover" {
			badge.Panel = &composer.Panel{}
		}
		context.Activity.Badges = append(context.Activity.Badges, badge)
	}
	comp.SetStatusCallbacks(&composer.StatusCallbacks{
		Enabled: func(id string) bool { return enabled[id] },
		Open:    func(id string) bool { return feed.open[id] },
		SetOpen: func(id string, open bool) bool {
			if open && !enabled[id] {
				return false
			}
			_, exists := sources[id]
			if !exists || registry == nil {
				return false
			}
			key, exists := keys[id]
			if !exists {
				return false
			}
			value := json.RawMessage("false")
			if open {
				value = json.RawMessage("true")
			}
			err := registry.ForwardStatus(key, "change", value)
			if err != nil && open {
				r.Report(err)
				return false
			}
			feed.open[id] = open
			if !open {
				delete(feed.open, id)
			}
			r.localRuntime().wake()
			return true
		},
		Render: func(view *ui.Context, id string) bool {
			if key, exists := keys[id]; exists && feed.open[id] {
				r.renderPluginContribution(view, key)
			}
			return true
		},
	})
}

func (r *Runtime) invokePluginStatus(scope protocol.PluginContext, id string) bool {
	feed := r.statusFeed(protocol.PluginUiStatusScope{Worktree: scope.Worktree, Session: scope.Session})
	if feed == nil {
		return false
	}
	for _, entry := range feed.view.Entries {
		if pluginStatusIdentity(entry) != id {
			continue
		}
		if entry.Declaration.Kind != "button" || entry.Invoke == nil {
			return false
		}
		pending := feed.pending[id]
		if !feed.view.Connected || pending != nil && pending.active {
			return false
		}
		if pending == nil {
			scopeBody, _ := json.Marshal(feed.scope)
			entryBody, _ := json.Marshal(entry)
			request, err := r.controller.PrepareStatusJSON(scopeBody, entryBody)
			if err != nil {
				r.Report(err)
				return true
			}
			pending = &pluginStatusPending{request: request}
			feed.pending[id] = pending
		}
		pending.active = true
		request := pending.request
		if !r.async(func() {
			_, err := r.controller.Execute(r.ctx, request)
			r.enqueue(func(view *ui.Context) {
				current := feed.pending[id]
				if current != pending || current.request.ID != request.ID {
					return
				}
				current.active = false
				if faultCode(err) != protocol.ErrorCodeOutcomeUnknown {
					delete(feed.pending, id)
				}
				if err != nil {
					r.packageReport(view, err)
				}
			})
		}) {
			pending.active = false
		}
		r.localRuntime().wake()
		return true
	}
	return false
}
