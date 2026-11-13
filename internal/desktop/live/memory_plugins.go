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
	// group is the plugins' tiles on the memory map.
	group kit.TreemapGroup
}

func (r *Runtime) closePluginMemory() {
	if r.parent != nil {
		return
	}
	if state := r.pluginMemory; state != nil && state.cancel != nil {
		state.cancel()
	}
	r.pluginMemory = nil
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
	state.group = pluginMemoryGroup(c, names, values, state.backend)
}

// pluginMemoryGroup is a tile per plugin with a measured runtime, its
// realms and the Node's runtimes summed; a sample still missing marks the
// total partial. The estimates are logical sizes (memoryWithPlugins).
func pluginMemoryGroup(c *ui.Context, names map[string]string, values []pluginMemoryValue, backend *protocol.PluginMemorySnapshot) kit.TreemapGroup {
	byName := map[string][]pluginMemoryValue{}
	for _, value := range values {
		byName[value.name] = append(byName[value.name], value)
	}
	order := make([]string, 0, len(byName))
	for name := range byName {
		order = append(order, name)
	}
	slices.Sort(order)
	group := kit.TreemapGroup{ID: "plugins", Label: i18n.T(c, "ram_plugins")}
	all := uint64(0)
	for _, name := range order {
		label := names[name]
		if label == "" {
			label = name
		}
		complete, total := backend != nil, uint64(0)
		for _, value := range byName[name] {
			if value.bytes == nil {
				complete = false
				continue
			}
			total += *value.bytes
		}
		if total == 0 {
			continue
		}
		detail := host.Bytes(float64(total))
		if !complete {
			detail = i18n.T(c, "ram_plugins_partial", "bytes", detail)
		}
		group.Items = append(group.Items, kit.TreemapItem{ID: name, Label: label, Detail: detail, Value: float64(total)})
		all += total
	}
	group.Detail = host.Bytes(float64(all))
	return group
}
