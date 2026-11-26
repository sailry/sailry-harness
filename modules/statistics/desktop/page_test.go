package statisticsdesktop

import (
	"context"
	"fmt"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewapi"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDemoUsagePageRenderFiltersBreakdownAndLocaleReset(t *testing.T) {
	clock := time.Date(2026, 4, 15, 14, 0, 0, 0, time.UTC)
	nowFn := func() time.Time { return clock }
	demoDataEN := preview.Demo("en", "local")
	ledgerEN := preview.DemoUsage(demoDataEN, "en", nowFn)
	demoDataZH := preview.Demo("zh-CN", "local")
	ledgerZH := preview.DemoUsage(demoDataZH, "zh-CN", nowFn)

	cat := i18n.New(Locales)
	activeLedger := ledgerEN
	var activeCtrl *demoUsageController

	renderView := func(c *ui.Context) {
		i18n.Bind(c, cat)
		ctrl := demoControllerFor(c, activeLedger, nowFn)
		activeCtrl = ctrl
		ctrl.renderer.render(c, ctrl)
	}

	tester := ui.NewTester(renderView, 1280, 900)
	tester.SetLocale("en")
	tester.Frame()
	tester.Frame()

	// Core cards render with DemoUsage data without touching any Node.
	for _, want := range []string{"30 days", "Token trend", "Activity heatmap", "Breakdown", "Requests", "Local time"} {
		if !tester.HasText(want) {
			t.Fatalf("expected %q in rendered Usage page", want)
		}
	}
	if activeCtrl == nil || activeCtrl.state.rangeKey != "30d" {
		t.Fatalf("expected default 30d range, got %+v", activeCtrl)
	}

	// Switch to 7d: Activity heatmap card must disappear.
	activeCtrl.SelectFilter("range", "7d")
	tester.Frame()
	if tester.HasText("Activity heatmap") {
		t.Fatal("Activity heatmap must be hidden for 7d range")
	}

	// Switch back to 90d: Activity heatmap reappears.
	activeCtrl.SelectFilter("range", "90d")
	tester.Frame()
	if !tester.HasText("Activity heatmap") {
		t.Fatal("Activity heatmap must be visible for 90d range")
	}

	// Switch breakdown to Project and verify rendering cleanly.
	activeCtrl.SelectBreakdown("project")
	tester.Frame()
	if activeCtrl.state.breakdown != "project" {
		t.Fatalf("expected project breakdown, got %q", activeCtrl.state.breakdown)
	}

	// Locale / Demo toggle resets filters back to default 30d + model breakdown cleanly.
	activeLedger = ledgerZH
	tester.SetLocale("zh-CN")
	tester.Frame()
	if activeCtrl.state.rangeKey != "30d" || activeCtrl.state.breakdown != "model" {
		t.Fatalf("expected Demo/locale change to reset state to 30d/model, got range=%q breakdown=%q",
			activeCtrl.state.rangeKey, activeCtrl.state.breakdown)
	}
	if !tester.HasText("Token 趋势") || !tester.HasText("活跃热力图") {
		t.Fatal("expected Simplified Chinese labels after locale switch")
	}
}

func TestUsagePageNarrowViewportRendersCleanly(t *testing.T) {
	clock := time.Date(2026, 4, 15, 14, 0, 0, 0, time.UTC)
	nowFn := func() time.Time { return clock }
	ledger := preview.DemoUsage(preview.Demo("en", "local"), "en", nowFn)
	cat := i18n.New(Locales)

	tester := ui.NewTester(func(c *ui.Context) {
		i18n.Bind(c, cat)
		ctrl := demoControllerFor(c, ledger, nowFn)
		ctrl.renderer.render(c, ctrl)
	}, 780, 900)
	tester.SetLocale("en")
	tester.Frame()
	tester.Frame()

	for _, want := range []string{"Cache hit rate", "Responses", "End-to-end TPS", "Estimated cost (USD)"} {
		if !tester.HasText(want) {
			t.Fatalf("narrow layout missing KPI %q", want)
		}
	}
}

type stubHost struct {
	st        *usagePageState
	inv       []usageInventorySource
	invRev    uint64
	resources []scopedResourceKey
	heat      *heatmapCache
	copied    string
}

func (h *stubHost) State() *usagePageState            { return h.st }
func (h *stubHost) Inventory() []usageInventorySource { return h.inv }
func (h *stubHost) InventoryRevision() uint64         { return h.invRev }
func (h *stubHost) Resources() []scopedResourceKey    { return h.resources }
func (h *stubHost) HeatmapCache() *heatmapCache       { return h.heat }
func (h *stubHost) Refreshing() bool                  { return false }
func (h *stubHost) PendingErrorKey() string           { return "" }
func (h *stubHost) ClearErrorKey()                    {}
func (h *stubHost) SelectFilter(string, string)       {}
func (h *stubHost) SelectBreakdown(string)            {}
func (h *stubHost) TurnPage(int)                      {}
func (h *stubHost) Refresh()                          {}
func (h *stubHost) OnActivityWidth(float32)           {}
func (h *stubHost) WriteClipboard(s string) bool      { h.copied = s; return true }

func TestBreakdownFoldingAndProjectionReuse(t *testing.T) {
	clock := time.Date(2026, 4, 15, 14, 0, 0, 0, time.UTC)
	var node protocol.NodeID
	node[0] = 1
	prov := protocol.ProviderID(uuid.New())

	groups := make([]scopedUsageGroup, 8)
	for i := 0; i < 8; i++ {
		groups[i] = scopedUsageGroup{
			Node: nodeHex(node),
			Group: protocol.UsageGroup{
				Key: protocol.UsageKey{
					Kind: "model",
					Data: protocol.UsageKeyModelPayload{Provider: prov, Model: fmt.Sprintf("model-%02d", i+1)},
				},
				Metrics: protocol.UsageMetrics{
					Responses: 1,
					Tokens:    &protocol.ConversationUsage{Input: uint64((10 - i) * 100), Output: 50},
				},
			},
		}
	}

	st := newUsagePageState(node, func() time.Time { return clock })
	st.rangeKey = "7d" // keep heatmap off for a fast focused breakdown test
	gen := st.nextGeneration()
	st.accept(usageViewState{
		connected: true,
		report: &usageData{
			node:   nodeHex(node),
			query:  st.query,
			totals: protocol.UsageMetrics{Responses: 8, Tokens: &protocol.ConversationUsage{Input: 5200, Output: 400}},
			groups: groups,
		},
	}, gen)

	host := &stubHost{st: st, invRev: 1}
	renderer := newUsagePageRenderer()
	cat := i18n.New(Locales)

	tester := ui.NewTester(func(c *ui.Context) {
		i18n.Bind(c, cat)
		renderer.render(c, host)
	}, 1100, 900)
	tester.SetLocale("en")
	tester.Frame()

	// Folded at 6 rows: model-01..model-06 visible, model-07 hidden, "Show all" visible.
	if !tester.HasText("model-01") || !tester.HasText("model-06") || tester.HasText("model-07") || !tester.HasText("Show all") {
		t.Fatal("expected breakdown to fold at 6 rows with Show all button")
	}

	// Render a second unchanged frame and verify cached projection slices are reused.
	firstSlicePtr := &renderer.breakCache.rows[0]
	tester.Frame()
	if &renderer.breakCache.rows[0] != firstSlicePtr {
		t.Fatal("unchanged frame recomputed breakdown projection instead of reusing cache")
	}

	// Expand via Show all click -> model-07 and model-08 appear, button flips to Show less.
	if err := tester.Click("Show all"); err != nil {
		t.Fatalf("click Show all: %v", err)
	}
	tester.Frame()
	if !tester.HasText("model-07") || !tester.HasText("model-08") || !tester.HasText("Show less") {
		t.Fatal("expected Show all to reveal remaining breakdown rows and switch to Show less")
	}
}

func TestRegisterStatisticsModuleContracts(t *testing.T) {
	var assembly shell.Assembly
	Register(&assembly)

	if len(assembly.Locales) != 1 || len(assembly.WorkspacePages) != 1 || len(assembly.MetricContributions) != 1 || len(assembly.PluginControlReplacements) != 5 {
		t.Fatalf("unexpected assembly: locales=%d pages=%d metrics=%d replacements=%d",
			len(assembly.Locales), len(assembly.WorkspacePages), len(assembly.MetricContributions), len(assembly.PluginControlReplacements))
	}
	pg := assembly.WorkspacePages[0]
	if pg.Page != UsagePage || pg.Feature.ID != "page:usage" || pg.Feature.Icon != icons.ChartBarNav || !pg.Feature.DefaultPinned || pg.Feature.LegacyPackage != "statistics" || !pg.ReplacesLegacyViews || !pg.SupportsDemo {
		t.Fatalf("unexpected Usage NativePage registration: %+v", pg)
	}
}

func TestLiveUsageControllerWatchLifecycleFilterHideDemoAndClose(t *testing.T) {
	var node protocol.NodeID
	node[0] = 0x11
	clock := time.Date(2026, 4, 15, 14, 0, 0, 0, time.UTC)

	var (
		mu              sync.Mutex
		watchesOpened   int
		watchesClosed   int
		readCalls       atomic.Int32
		activeDemo      bool
		demoRev         uint64
		demoLedger      = preview.DemoUsage(preview.Demo("en", "local"), "en", func() time.Time { return clock })
		lastOnUpdate    func(viewapi.UsageOverview, error)
		onChangeNotify  func()
		visible         = false
		refreshStarted  = make(chan struct{}, 4)
		refreshCanceled = make(chan struct{}, 4)
	)

	woke := make(chan struct{}, 32)
	services := viewapi.Services{
		Node: node,
		Wake: func() { woke <- struct{}{} },
		OnChange: func(cb func()) func() {
			mu.Lock()
			onChangeNotify = cb
			mu.Unlock()
			return func() {
				mu.Lock()
				onChangeNotify = nil
				mu.Unlock()
			}
		},
		Usage: &viewapi.UsageServices{
			Sources: func() []viewapi.UsageSource {
				return []viewapi.UsageSource{{ID: node, Label: "local", Local: true}}
			},
			Inventory: func(id protocol.NodeID) (viewapi.UsageInventory, bool) {
				return viewapi.UsageInventory{Node: id, Label: "local"}, true
			},
			Sample: func() (*viewapi.UsageSample, bool) {
				mu.Lock()
				defer mu.Unlock()
				if !activeDemo {
					return nil, false
				}
				return &viewapi.UsageSample{
					Revision:  demoRev,
					Report:    demoLedger.Report,
					Inventory: demoLedger.Inventory,
				}, true
			},
			ReadUsage: func(_ context.Context, target protocol.NodeID, q protocol.UsageQuery) (protocol.UsageReport, error) {
				readCalls.Add(1)
				mu.Lock()
				demo := activeDemo
				mu.Unlock()
				if demo {
					return demoLedger.Report(target, q)
				}
				r := protocol.DefaultUsageReport()
				r.Node, r.Query = target, q
				return r, nil
			},
			WatchOverview: func(ctx context.Context, _ protocol.NodeID, q protocol.UsageQuery, onUpdate func(viewapi.UsageOverview, error)) (viewapi.UsageWatch, error) {
				mu.Lock()
				watchesOpened++
				lastOnUpdate = onUpdate
				demo := activeDemo
				mu.Unlock()
				if demo {
					rep, err := demoLedger.Report(node, q)
					if err == nil {
						onUpdate(viewapi.UsageOverview{
							Sources:  []viewapi.UsageNodeSource{{Node: node, View: viewapi.UsageNodeView{Report: &rep, Connected: true}}},
							Summary:  &viewapi.UsageSummary{Totals: rep.Totals, Days: rep.Days, Recent: rep.Recent},
							Complete: true,
						}, nil)
					}
				}
				var closeOnce sync.Once
				return viewapi.UsageWatch{
					Refresh: func(rctx context.Context) error {
						refreshStarted <- struct{}{}
						<-rctx.Done()
						refreshCanceled <- struct{}{}
						return rctx.Err()
					},
					Close: func() {
						closeOnce.Do(func() {
							mu.Lock()
							watchesClosed++
							mu.Unlock()
						})
					},
				}, nil
			},
		},
	}

	ctrl := newLiveUsageController(shell.WindowContext{
		Context: viewapi.WithServices(t.Context(), services),
		Visible: func() bool { return visible },
	}, services, func() time.Time { return clock })

	// While hidden, sync must NOT open any WatchOverview.
	ctrl.sync()
	mu.Lock()
	if watchesOpened != 0 {
		t.Fatalf("hidden page opened %d watches, want 0", watchesOpened)
	}
	mu.Unlock()

	// Showing page opens watch #1.
	visible = true
	ctrl.sync()
	mu.Lock()
	if watchesOpened != 1 || watchesClosed != 0 {
		t.Fatalf("expected 1 open watch, got opened=%d closed=%d", watchesOpened, watchesClosed)
	}
	staleCallback := lastOnUpdate
	mu.Unlock()

	// Changing filter closes watch #1 and opens watch #2; stale reply from watch #1 is ignored.
	ctrl.SelectFilter("range", "7d")
	mu.Lock()
	if watchesOpened != 2 || watchesClosed != 1 {
		t.Fatalf("filter change should close old watch and open new one: opened=%d closed=%d", watchesOpened, watchesClosed)
	}
	mu.Unlock()
	staleCallback(viewapi.UsageOverview{
		Summary:  &viewapi.UsageSummary{Totals: protocol.UsageMetrics{Responses: 999}},
		Complete: true,
	}, nil)
	ctrl.sync()
	if ctrl.state.view != nil {
		t.Fatalf("stale watch reply should be discarded, got %+v", ctrl.state.view)
	}

	// Start an in-flight Refresh, then hide the page: both the watch AND the refresh context must cancel immediately.
	ctrl.Refresh()
	<-refreshStarted
	visible = false
	ctrl.sync()
	<-refreshCanceled
	mu.Lock()
	if watchesClosed != 2 {
		t.Fatalf("hiding page should close active watch, closed=%d", watchesClosed)
	}
	mu.Unlock()

	// Toggling Demo mode resets filters back to 30d and re-opens watch when visible.
	visible = true
	mu.Lock()
	activeDemo = true
	demoRev = 1
	notify := onChangeNotify
	mu.Unlock()
	if notify != nil {
		notify()
	}
	ctrl.sync()
	if ctrl.state.rangeKey != "30d" {
		t.Fatalf("Demo toggle should reset range back to 30d, got %q", ctrl.state.rangeKey)
	}

	// Close must release subscriptions, watches, and all goroutines cleanly.
	ctrl.Close()
	ctrl.wait()
	mu.Lock()
	if watchesClosed != watchesOpened || onChangeNotify != nil {
		t.Fatalf("Close leaked resources: opened=%d closed=%d sub=%v", watchesOpened, watchesClosed, onChangeNotify != nil)
	}
	mu.Unlock()
}

func TestLiveUsageControllerCanonicalHostLabelFallbacks(t *testing.T) {
	var node protocol.NodeID
	node[0] = 0xab
	node[1] = 0xcd
	clock := time.Date(2026, 4, 15, 14, 0, 0, 0, time.UTC)

	label := "Local"
	var onChange func()
	services := viewapi.Services{
		Node:      node,
		NodeLabel: func(id protocol.NodeID) string { return label },
		OnChange: func(cb func()) func() {
			onChange = cb
			return func() { onChange = nil }
		},
		Usage: &viewapi.UsageServices{
			Sources: func() []viewapi.UsageSource { return nil }, // exercises fallback path
			Inventory: func(id protocol.NodeID) (viewapi.UsageInventory, bool) {
				return viewapi.UsageInventory{Node: id}, true // empty label exercises NodeLabel fallback
			},
			WatchOverview: func(context.Context, protocol.NodeID, protocol.UsageQuery, func(viewapi.UsageOverview, error)) (viewapi.UsageWatch, error) {
				return viewapi.UsageWatch{Close: func() {}}, nil
			},
		},
	}

	ctrl := newLiveUsageController(shell.WindowContext{
		Context: viewapi.WithServices(t.Context(), services),
		Visible: func() bool { return true },
	}, services, func() time.Time { return clock })
	defer ctrl.Close()

	ctrl.sync()
	if len(ctrl.sources) != 1 || ctrl.sources[0].Label != "Local" {
		t.Fatalf("expected canonical NodeLabel fallback, got %+v", ctrl.sources)
	}
	if len(ctrl.inventory) != 1 || ctrl.inventory[0].label != "Local" {
		t.Fatalf("expected canonical inventory label, got %+v", ctrl.inventory)
	}

	// Dynamic locale switch updates canonical host label via OnChange.
	label = "本地"
	onChange()
	ctrl.sync()
	if ctrl.inventory[0].label != "本地" {
		t.Fatalf("expected updated localized label %q, got %q", "本地", ctrl.inventory[0].label)
	}

	// Without NodeLabel, fallback uses 8-char short ID, never raw 64-char hex.
	ctrlNoLabel := newLiveUsageController(shell.WindowContext{
		Context: t.Context(),
		Visible: func() bool { return true },
	}, viewapi.Services{Node: node}, func() time.Time { return clock })
	defer ctrlNoLabel.Close()
	if got := ctrlNoLabel.nodeLabel(node); len(got) != 8 || got != "abcd0000" {
		t.Fatalf("expected 8-char short ID fallback, got %q", got)
	}
}
