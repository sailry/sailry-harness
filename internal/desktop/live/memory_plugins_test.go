package live

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestPluginMemoryGroup asserts the plugins' tiles sum each plugin's
// runtimes, leave unmeasured and idle plugins out, mark a missing sample
// partial, and come out of live objects on the one map.
func TestPluginMemoryGroup(t *testing.T) {
	bytes := func(n uint64) *uint64 { return &n }
	at := time.Unix(1234567890, 0)
	backend := &protocol.PluginMemorySnapshot{CapturedAtMs: uint64(at.UnixMilli())}
	values := []pluginMemoryValue{
		{name: "alpha", kind: "view", bytes: bytes(512), at: at},
		{name: "alpha", kind: "view", bytes: bytes(256), at: at},
		{name: "alpha", kind: "tool", bytes: bytes(1024), at: at},
		{name: "beta", kind: "callback", busy: true},
		{name: "captured", kind: "model", bytes: bytes(128), at: at},
	}
	tester := ui.NewTester(func(c *ui.Context) {
		group := pluginMemoryGroup(c, map[string]string{"idle": "Idle", "alpha": "Alpha", "beta": "Beta"}, values, backend)
		if len(group.Items) != 2 || group.Items[0].ID != "alpha" || group.Items[0].Label != "Alpha" || group.Items[0].Value != 1792 ||
			group.Items[1].ID != "captured" || group.Items[1].Label != "captured" || group.Detail != host.Bytes(1920) {
			t.Fatalf("plugin tiles: %+v", group)
		}
		partial := append(values, pluginMemoryValue{name: "alpha", kind: "tool", busy: true})
		if mixed := pluginMemoryGroup(c, nil, partial, backend); mixed.Items[0].Detail != i18n.T(c, "ram_plugins_partial", "bytes", host.Bytes(1792)) {
			t.Fatalf("a missing sample concealed partial accounting: %+v", mixed.Items[0])
		}
		base := []kit.TreemapGroup{{ID: "ram_heap", Items: []kit.TreemapItem{{ID: "ram_objects", Value: 1000}, {ID: "ram_free", Value: 50}}}}
		merged := memoryWithPlugins(base, group)
		if len(merged) != 2 || merged[1].ID != "plugins" || merged[0].Items[0].Value != 0 || merged[0].Items[1].Value != 50 || base[0].Items[0].Value != 1000 {
			t.Fatalf("plugins were not taken out of live objects on one map: %+v", merged)
		}
		if same := memoryWithPlugins(base, kit.TreemapGroup{ID: "plugins"}); len(same) != 1 {
			t.Fatalf("an empty plugins group was drawn: %+v", same)
		}
	}, 200, 200)
	tester.SetLocale("en")
	tester.Frame()
}
