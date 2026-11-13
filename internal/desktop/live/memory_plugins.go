package live

import (
	"context"
	"errors"
	"fmt"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const pluginMemoryInterval = 5 * time.Second

type pluginMemoryMount struct {
	id, name string
	mounted  *desktopplugins.Mounted
}

type pluginMemoryValue struct {
	name, kind string
	bytes      *uint64
	at         time.Time
	busy       bool
}

type pluginMemoryState struct {
	next    time.Time
	pending bool
	cancel  context.CancelFunc
	backend *protocol.PluginMemorySnapshot
	error   string
	refresh []pluginMemoryMount
	index   int
	views   map[*desktopplugins.Mounted]pluginMemoryValue
	dirty   bool
	locale  string
}

func (r *Runtime) closePluginMemory() {
	if r.parent != nil {
		return
	}
	if state := r.pluginMemory; state != nil && state.cancel != nil {
		state.cancel()
	}
	r.pluginMemory = nil
	if r.App != nil {
		r.App.Settings.Model.PluginMemory = nil
		r.App.Settings.Model.PluginMemoryRows = nil
	}
}

// pluginMemoryFrame runs between plugin calls on the window's UI goroutine.
// One UI realm is walked per frame, once per diagnostic interval. Node reads
// have the same cadence and never overlap. Leaving the page releases both
// caches and the in-flight read; it never mounts an otherwise inactive plugin.
func (r *Runtime) pluginMemoryFrame(c *ui.Context) {
	state := r.pluginMemory
	if state == nil {
		state = &pluginMemoryState{views: map[*desktopplugins.Mounted]pluginMemoryValue{}, dirty: true}
		r.pluginMemory = state
	}
	now := c.Now()
	if !now.Before(state.next) && state.index >= len(state.refresh) {
		state.next = now.Add(pluginMemoryInterval)
		state.refresh, state.index = r.memoryMounts(), 0
		alive := map[*desktopplugins.Mounted]bool{}
		for _, mount := range state.refresh {
			alive[mount.mounted] = true
			if _, exists := state.views[mount.mounted]; !exists {
				state.views[mount.mounted] = pluginMemoryValue{name: mount.name, kind: "view"}
			}
		}
		for mounted := range state.views {
			if !alive[mounted] {
				delete(state.views, mounted)
			}
		}
		state.dirty = true
		if r.controller != nil && !r.controller.Connected() {
			state.backend = nil
		}
		if !state.pending && r.controller != nil && r.controller.Connected() {
			r.readPluginMemory(state)
		}
	}
	if state.index < len(state.refresh) {
		mount := state.refresh[state.index]
		state.index++
		if bytes, complete := mount.mounted.MemoryBytes(); complete {
			state.views[mount.mounted] = pluginMemoryValue{name: mount.name, kind: "view", bytes: &bytes, at: now}
		}
		if state.index < len(state.refresh) {
			c.Invalidate()
		} else {
			state.refresh = nil
			state.index = 0
			state.dirty = true
		}
	}
	if locale := c.Locale(); locale != state.locale {
		state.locale, state.dirty = locale, true
	}
	if state.dirty {
		r.projectPluginMemory(c, state)
		state.dirty = false
	}
	c.After(max(0, state.next.Sub(now)))
}

func (r *Runtime) memoryMounts() []pluginMemoryMount {
	var mounts []pluginMemoryMount
	seen := map[*desktopplugins.Mounted]bool{}
	add := func(id, name string, mounted *desktopplugins.Mounted) {
		if mounted != nil && !seen[mounted] {
			seen[mounted] = true
			mounts = append(mounts, pluginMemoryMount{id: id, name: name, mounted: mounted})
		}
	}
	if r.pluginViews != nil {
		for key, view := range r.pluginViews.mounts {
			add("view/"+fmt.Sprint(key), view.scope.Package.Name, view.mounted)
		}
	}
	if r.pluginCards != nil {
		for key, card := range r.pluginCards.cards {
			add("card/"+fmt.Sprint(key), card.summary.Package.Name, card.mounted)
		}
	}
	slices.SortFunc(mounts, func(a, b pluginMemoryMount) int { return strings.Compare(a.id, b.id) })
	return mounts
}

func (r *Runtime) readPluginMemory(state *pluginMemoryState) {
	ctx, cancel := context.WithCancel(r.ctx)
	state.cancel, state.pending = cancel, true
	r.async(func() {
		defer cancel()
		out, err := r.controller.Do(ctx, protocol.Command{Kind: "read_plugin_memory", Data: protocol.CommandReadPluginMemoryPayload{}})
		snapshot, valid := detail[protocol.PluginMemorySnapshot](out.Data)
		if err == nil && (out.Kind != "plugin_memory" || !valid) {
			err = errors.New("invalid plugin memory response")
		}
		r.enqueue(func(c *ui.Context) {
			if r.pluginMemory != state || r.App.Page != shell.SettingsPage || r.App.Settings.Section != settings.Experimental {
				return
			}
			state.pending, state.cancel = false, nil
			if err != nil {
				if !errors.Is(err, context.Canceled) && state.error != err.Error() {
					kit.Error(c, i18n.T(c, "ram_plugins_error"), err.Error())
				}
				state.backend = nil
				state.error = err.Error()
			} else {
				state.backend, state.error = &snapshot, ""
			}
			state.dirty = true
		})
	})
}

func (r *Runtime) projectPluginMemory(c *ui.Context, state *pluginMemoryState) {
	values := make([]pluginMemoryValue, 0, len(state.views))
	for _, value := range state.views {
		values = append(values, value)
	}
	if state.backend != nil {
		for _, sample := range state.backend.Samples {
			values = append(values, pluginMemoryValue{name: sample.Package.Name, kind: sample.Kind, bytes: sample.Bytes,
				at: time.UnixMilli(int64(sample.SampledAtMs)), busy: sample.Busy})
		}
	}
	names := map[string]string{}
	for _, plugin := range r.snapshot().Plugins {
		names[plugin.Name] = plugin.Name
	}
	for _, value := range values {
		names[value.name] = value.name
	}
	if r.packages != nil {
		for name := range names {
			if info, exists := r.packages.infos[name]; exists {
				names[name] = packageView(info, c.Locale()).Name
			}
		}
	}
	r.App.Settings.Model.PluginMemory, r.App.Settings.Model.PluginMemoryRows = pluginMemoryGroups(c, names, values, state.backend)
}

// pluginMemoryGroups never adds estimates to Go memory classes: the realm
// walker uses logical sizes, excludes shared/native allocations, and cannot
// partition runtime/metrics. Rows keep zero and unavailable values visible.
func pluginMemoryGroups(c *ui.Context, names map[string]string, values []pluginMemoryValue, backend *protocol.PluginMemorySnapshot) ([]kit.TreemapGroup, []settings.PluginMemoryRow) {
	byName := map[string][]pluginMemoryValue{}
	for name := range names {
		byName[name] = nil
	}
	for _, value := range values {
		byName[value.name] = append(byName[value.name], value)
	}
	order := make([]string, 0, len(byName))
	for name := range byName {
		order = append(order, name)
	}
	slices.Sort(order)
	var groups []kit.TreemapGroup
	rows := make([]settings.PluginMemoryRow, 0, len(order))
	for _, name := range order {
		label := names[name]
		if label == "" {
			label = name
		}
		group := kit.TreemapGroup{ID: name, Label: label}
		complete, total, busy := backend != nil, uint64(0), false
		var parts []string
		var oldest time.Time
		if backend != nil {
			oldest = time.UnixMilli(int64(backend.CapturedAtMs))
		}
		for _, kind := range []string{"view", "callback", "tool", "model"} {
			bytes, present, known := uint64(0), false, true
			for _, value := range byName[name] {
				if value.kind != kind {
					continue
				}
				present, busy = true, busy || value.busy
				if value.bytes == nil {
					known = false
					continue
				}
				bytes += *value.bytes
				if !value.at.IsZero() && (oldest.IsZero() || value.at.Before(oldest)) {
					oldest = value.at
				}
			}
			if !present {
				continue
			}
			complete = complete && known
			title := i18n.T(c, "ram_plugins_"+kind)
			detail := host.Bytes(float64(bytes))
			if !known {
				detail = i18n.T(c, "ram_plugins_pending")
			}
			parts = append(parts, title+" "+detail)
			total += bytes
			if bytes > 0 {
				group.Items = append(group.Items, kit.TreemapItem{ID: kind, Label: title, Detail: host.Bytes(float64(bytes)), Value: float64(bytes)})
			}
		}
		value := host.Bytes(float64(total))
		if !complete {
			if total == 0 {
				value = i18n.T(c, "ram_plugins_pending")
			} else {
				value = i18n.T(c, "ram_plugins_partial", "bytes", value)
			}
		}
		if len(parts) == 0 && complete {
			parts = append(parts, i18n.T(c, "ram_plugins_idle"))
		}
		if busy {
			parts = append(parts, i18n.T(c, "ram_plugins_busy"))
		}
		if !oldest.IsZero() {
			parts = append(parts, i18n.T(c, "ram_plugins_sampled", "time", oldest.Local().Format("15:04:05")))
		}
		group.Detail = value
		// Zero groups keep the other plugins' series colors stable as VMs
		// open and close; Treemap omits their tiles and legend entries.
		groups = append(groups, group)
		rows = append(rows, settings.PluginMemoryRow{ID: name, Name: label, Detail: strings.Join(parts, " · "), Total: value})
	}
	return groups, rows
}
