package live

import (
	"reflect"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginMemoryGroups(t *testing.T) {
	bytes := func(n uint64) *uint64 { return &n }
	at := time.Unix(1234567890, 0)
	backend := &protocol.PluginMemorySnapshot{CapturedAtMs: uint64(at.Add(time.Second).UnixMilli())}
	values := []pluginMemoryValue{
		{name: "alpha", kind: "view", bytes: bytes(512), at: at},
		{name: "alpha", kind: "view", bytes: bytes(256), at: at},
		{name: "alpha", kind: "tool", bytes: bytes(1024), at: at},
		{name: "alpha", kind: "model", bytes: bytes(2048), at: at},
		{name: "beta", kind: "callback", busy: true},
		{name: "captured", kind: "model", bytes: bytes(128), at: at},
	}
	tester := ui.NewTester(func(c *ui.Context) {
		groups, rows := pluginMemoryGroups(c, map[string]string{"idle": "Idle", "alpha": "Alpha", "beta": "Beta"}, values, backend)
		var ids []string
		for _, row := range rows {
			ids = append(ids, row.ID)
		}
		if want := []string{"alpha", "beta", "captured", "idle"}; !reflect.DeepEqual(ids, want) {
			t.Fatalf("plugin order = %v, want %v", ids, want)
		}
		if len(groups) != 4 || groups[0].ID != "alpha" || len(groups[0].Items) != 3 || groups[0].Items[0].Value != 768 || len(groups[1].Items) != 0 {
			t.Fatalf("view instances or runtime kinds were not combined: %+v", groups)
		}
		if rows[0].Total != host.Bytes(3840) || !strings.Contains(rows[0].Detail, at.Local().Format("15:04:05")) {
			t.Fatalf("plugin total or sample age is missing: %+v", rows[0])
		}
		if rows[1].Total != i18n.T(c, "ram_plugins_pending") || !strings.Contains(rows[1].Detail, i18n.T(c, "ram_plugins_busy")) {
			t.Fatalf("a busy unmeasured VM became zero: %+v", rows[1])
		}
		if rows[3].Total != host.Bytes(0) || !strings.Contains(rows[3].Detail, i18n.T(c, "ram_plugins_idle")) {
			t.Fatalf("a plugin with no runtime is not zero: %+v", rows[3])
		}
		partial := append(slices.Clone(values), pluginMemoryValue{name: "alpha", kind: "tool", busy: true})
		_, mixed := pluginMemoryGroups(c, map[string]string{"alpha": "Alpha"}, partial, backend)
		if mixed[0].Total != i18n.T(c, "ram_plugins_partial", "bytes", host.Bytes(3840)) {
			t.Fatalf("a missing instance concealed partial accounting: %+v", mixed[0])
		}
		_, unknown := pluginMemoryGroups(c, map[string]string{"idle": "Idle"}, nil, nil)
		if unknown[0].Total != i18n.T(c, "ram_plugins_pending") {
			t.Fatalf("an unavailable backend was assumed idle: %+v", unknown[0])
		}
	}, 200, 200)
	tester.SetLocale("en")
	tester.Frame()
}
