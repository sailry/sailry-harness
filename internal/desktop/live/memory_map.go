package live

import (
	"runtime/metrics"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

// memoryInterval is how often Settings › Experimental samples memory.
const memoryInterval = 2 * time.Second

// memoryClasses are this Desktop process's memory as the Go runtime
// classifies it (runtime/metrics /memory/classes). This covers Go-managed
// memory, not native allocations or every mapping of the process. Memory
// returned to the system (heap/released) is left out. Plugin JS estimates
// are separate diagnostics, not additional Go memory classes.
var memoryClasses = []struct {
	group string
	items []struct{ key, metric string }
}{
	{"ram_heap", []struct{ key, metric string }{
		{"ram_objects", "/memory/classes/heap/objects:bytes"},
		{"ram_fragmented", "/memory/classes/heap/unused:bytes"},
		{"ram_free", "/memory/classes/heap/free:bytes"},
	}},
	{"ram_stacks", []struct{ key, metric string }{
		{"ram_goroutine_stacks", "/memory/classes/heap/stacks:bytes"},
		{"ram_system_stacks", "/memory/classes/os-stacks:bytes"},
	}},
	{"ram_runtime", []struct{ key, metric string }{
		{"ram_allocator", "/memory/classes/metadata/mspan/inuse:bytes"},
		{"ram_allocator", "/memory/classes/metadata/mspan/free:bytes"},
		{"ram_allocator", "/memory/classes/metadata/mcache/inuse:bytes"},
		{"ram_allocator", "/memory/classes/metadata/mcache/free:bytes"},
		{"ram_collector", "/memory/classes/metadata/other:bytes"},
		{"ram_profiling", "/memory/classes/profiling/buckets:bytes"},
	}},
	{"ram_other", []struct{ key, metric string }{
		{"ram_other", "/memory/classes/other:bytes"},
	}},
}

// memoryMapFrame shows the process's memory on Settings › Experimental,
// sampled while the page shows and dropped when it closes.
func (r *Runtime) memoryMapFrame(c *ui.Context) {
	if r.parent != nil {
		return
	}
	m := r.App.Settings.Model
	if r.App.Page != shell.SettingsPage || r.App.Settings.Section != settings.Experimental {
		m.Memory, r.memoryNext = nil, time.Time{}
		r.closePluginMemory()
		return
	}
	now := c.Now()
	if m.Memory == nil || !now.Before(r.memoryNext) {
		m.Memory, r.memoryNext = memoryGroups(c), now.Add(memoryInterval)
	}
	r.pluginMemoryFrame(c)
	c.After(r.memoryNext.Sub(now))
}

// memoryGroups reads the classes once, an item's classes of one key summed.
func memoryGroups(c *ui.Context) []kit.TreemapGroup {
	var samples []metrics.Sample
	for _, class := range memoryClasses {
		for _, item := range class.items {
			samples = append(samples, metrics.Sample{Name: item.metric})
		}
	}
	metrics.Read(samples)
	groups := make([]kit.TreemapGroup, 0, len(memoryClasses))
	at := 0
	for _, class := range memoryClasses {
		group := kit.TreemapGroup{ID: class.group, Label: i18n.T(c, class.group)}
		total := 0.0
		for _, item := range class.items {
			sample := samples[at]
			at++
			if sample.Value.Kind() != metrics.KindUint64 {
				continue
			}
			value := float64(sample.Value.Uint64())
			total += value
			if n := len(group.Items); n > 0 && group.Items[n-1].ID == item.key {
				group.Items[n-1].Value += value
				continue
			}
			group.Items = append(group.Items, kit.TreemapItem{ID: item.key, Label: i18n.T(c, item.key), Value: value})
		}
		for i := range group.Items {
			group.Items[i].Detail = host.Bytes(group.Items[i].Value)
		}
		group.Detail = host.Bytes(total)
		groups = append(groups, group)
	}
	return groups
}
