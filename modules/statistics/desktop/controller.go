package statisticsdesktop

import (
	"context"
	"slices"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewapi"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pendingOverviewUpdate struct {
	generation uint64
	overview   viewapi.UsageOverview
	err        error
}

type pendingChoiceUpdate struct {
	generation uint64
	cursor     string
	node       string
	resources  []protocol.UsageKey
}

type pendingHeatUpdate struct {
	generation uint64
	request    uint64
	cursor     string
	cache      *heatmapCache
}

// liveUsageController manages WatchOverview subscriptions, choice/heatmap reads,
// Demo mode switching, and strict cancellation/cleanup for one window's Usage page.
type liveUsageController struct {
	ctx         context.Context
	cancel      context.CancelFunc
	services    viewapi.Services
	visible     func() bool
	now         func() time.Time
	unsubChange func()

	// UI-thread-owned presentation state.
	active          bool
	demoInitialized bool
	demoOn          bool
	demoRev         uint64
	sources         []viewapi.UsageSource
	state           *usagePageState
	inventory       []usageInventorySource
	invRev          uint64
	resources       []scopedResourceKey
	choiceCursor    string
	heat            *heatmapCache
	activityWidth   float32
	heatRequest     uint64
	errorKey        string
	lastToastKey    string
	renderer        *usagePageRenderer

	// Watch and background read ownership.
	watch       viewapi.UsageWatch
	watchCtx    context.Context
	watchCancel context.CancelFunc
	heatCancel  context.CancelFunc

	// Cross-goroutine inbox guarded by mu; drained on UI thread in sync/render.
	mu             sync.Mutex
	closed         bool
	changeDirty    bool
	pendingView    *pendingOverviewUpdate
	pendingChoices []pendingChoiceUpdate
	pendingHeat    *pendingHeatUpdate
	wg             sync.WaitGroup
}

func newLiveUsageController(window shell.WindowContext, services viewapi.Services, now func() time.Time) *liveUsageController {
	if now == nil {
		now = time.Now
	}
	parent := window.Context
	if parent == nil {
		parent = context.Background()
	}
	ctx, cancel := context.WithCancel(parent)
	c := &liveUsageController{
		ctx:         ctx,
		cancel:      cancel,
		services:    services,
		visible:     window.Visible,
		now:         now,
		unsubChange: func() {},
		state:       newUsagePageState(services.Node, now),
		renderer:    newUsagePageRenderer(),
	}
	if services.OnChange != nil {
		c.unsubChange = services.OnChange(func() {
			c.mu.Lock()
			if c.closed {
				c.mu.Unlock()
				return
			}
			c.changeDirty = true
			c.mu.Unlock()
			c.wake()
		})
	}
	return c
}

func (c *liveUsageController) wake() {
	if c.services.Wake != nil {
		c.services.Wake()
	}
}

func (c *liveUsageController) isClosed() bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.closed
}

func (c *liveUsageController) isVisible() bool {
	if c.visible == nil {
		return true
	}
	return c.visible()
}

func (c *liveUsageController) sync() {
	if c.isClosed() {
		return
	}
	c.drainPending()

	visible := c.isVisible()
	usageSvc := c.services.Usage
	if usageSvc == nil {
		return
	}

	var (
		sample *viewapi.UsageSample
		isDemo bool
	)
	if usageSvc.Sample != nil {
		sample, isDemo = usageSvc.Sample()
	}
	var sampleRev uint64
	if sample != nil {
		sampleRev = sample.Revision
	}

	demoChanged := !c.demoInitialized || c.demoOn != isDemo || (isDemo && c.demoRev != sampleRev)
	if demoChanged {
		c.demoInitialized = true
		c.demoOn = isDemo
		c.demoRev = sampleRev
		c.stopWatchAndReads()
		c.resetStateForLedgerChange()
		c.refreshSourcesAndInventory()
		if visible {
			c.active = true
			c.observe()
		} else {
			c.active = false
		}
		return
	}

	if !visible {
		if c.active {
			// Page became hidden: unsubscribe WatchOverview and cancel in-flight reads immediately.
			c.active = false
			c.stopWatchAndReads()
		}
		return
	}

	if !c.active {
		c.active = true
		c.refreshSourcesAndInventory()
		c.observe()
		return
	}

	// While active, react to node/peer/inventory changes from OnChange.
	c.mu.Lock()
	dirty := c.changeDirty
	c.changeDirty = false
	c.mu.Unlock()
	if dirty {
		sourcesChanged := c.refreshSourcesAndInventory()
		if sourcesChanged {
			c.observe()
		}
	}
}

func (c *liveUsageController) resetStateForLedgerChange() {
	c.state = newUsagePageState(c.services.Node, c.now)
	c.inventory = nil
	c.invRev++
	c.resources = nil
	c.choiceCursor = ""
	c.heat = nil
	c.errorKey = ""
	c.lastToastKey = ""
	c.renderer = newUsagePageRenderer()
	c.mu.Lock()
	c.pendingView = nil
	c.pendingChoices = nil
	c.pendingHeat = nil
	c.changeDirty = false
	c.mu.Unlock()
}

func (c *liveUsageController) nodeLabel(id protocol.NodeID) string {
	if c.services.NodeLabel != nil {
		if label := c.services.NodeLabel(id); label != "" {
			return label
		}
	}
	return shortPrefix(nodeHex(id))
}

func (c *liveUsageController) refreshSourcesAndInventory() bool {
	usageSvc := c.services.Usage
	if usageSvc == nil {
		return false
	}
	var nextSources []viewapi.UsageSource
	if usageSvc.Sources != nil {
		nextSources = usageSvc.Sources()
	}
	if len(nextSources) == 0 && c.services.Node != (protocol.NodeID{}) {
		nextSources = []viewapi.UsageSource{{ID: c.services.Node, Label: c.nodeLabel(c.services.Node), Local: true}}
	}

	sourcesChanged := len(nextSources) != len(c.sources)
	if !sourcesChanged {
		for i := range nextSources {
			if nextSources[i].ID != c.sources[i].ID {
				sourcesChanged = true
				break
			}
		}
	}
	c.sources = nextSources

	if sourcesChanged {
		validNodes := make(map[string]bool, len(nextSources))
		for _, s := range nextSources {
			validNodes[nodeHex(s.ID)] = true
		}
		c.resources = slices.DeleteFunc(c.resources, func(rk scopedResourceKey) bool {
			return !validNodes[rk.node]
		})
	}

	if usageSvc.Inventory != nil {
		nextInv := make([]usageInventorySource, 0, len(nextSources))
		for _, src := range nextSources {
			if inv, ok := usageSvc.Inventory(src.ID); ok {
				label := inv.Label
				if label == "" {
					label = src.Label
				}
				if label == "" {
					label = c.nodeLabel(src.ID)
				}
				nextInv = append(nextInv, usageInventorySource{
					node:      nodeHex(src.ID),
					label:     label,
					projects:  inv.Projects,
					providers: inv.Providers,
				})
			}
		}
		c.inventory = nextInv
		c.invRev++
	}
	return sourcesChanged
}

func (c *liveUsageController) stopWatchAndReads() {
	watch := c.watch
	c.watch = viewapi.UsageWatch{}
	c.watchCtx = nil
	watchCancel := c.watchCancel
	c.watchCancel = nil
	heatCancel := c.heatCancel
	c.heatCancel = nil

	if watchCancel != nil {
		watchCancel()
	}
	if heatCancel != nil {
		heatCancel()
	}
	if watch.Close != nil {
		watch.Close()
	}
}

func (c *liveUsageController) observe() {
	c.stopWatchAndReads()
	if c.isClosed() || !c.active || c.services.Usage == nil || c.services.Usage.WatchOverview == nil {
		return
	}

	gen := c.state.nextGeneration()
	c.choiceCursor = ""
	c.resources = nil
	c.heat = nil
	c.errorKey = ""
	c.lastToastKey = ""

	c.mu.Lock()
	c.pendingView = nil
	c.pendingChoices = nil
	c.pendingHeat = nil
	c.mu.Unlock()

	readCtx, readCancel := context.WithCancel(c.ctx)
	c.watchCtx = readCtx
	c.watchCancel = readCancel

	c.loadChoices(readCtx, gen, "")

	var target protocol.NodeID
	if !c.state.allNodes() {
		if id, ok := parseNodeHex(c.state.node); ok {
			target = id
		}
	}
	query := cloneUsageQuery(c.state.query)
	watch, err := c.services.Usage.WatchOverview(readCtx, target, query, func(ov viewapi.UsageOverview, err error) {
		c.mu.Lock()
		if c.closed {
			c.mu.Unlock()
			return
		}
		c.pendingView = &pendingOverviewUpdate{
			generation: gen,
			overview:   ov,
			err:        err,
		}
		c.mu.Unlock()
		c.wake()
	})
	if err != nil {
		c.reportErrorKey(errorKeyForErr(err))
		return
	}
	c.watch = watch
}

func (c *liveUsageController) drainPending() {
	c.mu.Lock()
	pv := c.pendingView
	c.pendingView = nil
	pcs := c.pendingChoices
	c.pendingChoices = nil
	ph := c.pendingHeat
	c.pendingHeat = nil
	c.mu.Unlock()

	if pv != nil {
		c.applyOverviewUpdate(*pv)
	}
	for _, pc := range pcs {
		if pc.generation != c.state.generation || pc.cursor != c.choiceCursor {
			continue
		}
		c.resources = slices.DeleteFunc(c.resources, func(rk scopedResourceKey) bool {
			return rk.node == pc.node
		})
		for _, k := range pc.resources {
			c.resources = append(c.resources, scopedResourceKey{node: pc.node, key: k})
		}
	}
	if ph != nil {
		if ph.generation == c.state.generation && ph.request == c.heatRequest && ph.cursor == c.state.resourceCursor() {
			c.heat = ph.cache
		}
	}
}

func (c *liveUsageController) applyOverviewUpdate(u pendingOverviewUpdate) {
	if u.generation != c.state.generation {
		return
	}
	if u.err != nil {
		if c.ctx.Err() == nil {
			c.reportErrorKey(errorKeyForErr(u.err))
		}
		return
	}
	viewState := projectOverview(u.overview, c.state)
	if !c.state.accept(viewState, u.generation) {
		return
	}
	if c.state.advance() {
		c.state.resetPagination()
		c.state.view = nil
		c.observe()
		return
	}
	if viewState.err != nil {
		c.reportErrorKey(errorKeyForFault(viewState.err))
	} else {
		c.errorKey = ""
		c.lastToastKey = ""
	}
	cursor := c.state.resourceCursor()
	if viewState.report != nil && c.choiceCursor != cursor {
		c.choiceCursor = cursor
		if c.watchCtx != nil {
			c.loadChoices(c.watchCtx, u.generation, cursor)
		}
		c.loadHeatmap(u.generation)
	}
}

func (c *liveUsageController) reportErrorKey(key string) {
	if key == "" {
		return
	}
	if c.lastToastKey != key {
		c.errorKey = key
		c.lastToastKey = key
	}
}

func (c *liveUsageController) loadChoices(parent context.Context, gen uint64, cursor string) {
	usageSvc := c.services.Usage
	if usageSvc == nil || usageSvc.ReadUsage == nil || len(c.sources) == 0 {
		return
	}
	choiceQuery := cloneUsageQuery(c.state.query)
	choiceQuery.Providers = []protocol.ProviderID{}
	choiceQuery.Models = []string{}
	choiceQuery.Before = nil
	all := c.state.allNodes()
	selectedNode := c.state.node

	for _, src := range c.sources {
		nodeStr := nodeHex(src.ID)
		if !all && nodeStr != selectedNode {
			continue
		}
		target := src.ID
		c.wg.Add(1)
		go func() {
			defer c.wg.Done()
			report, err := usageSvc.ReadUsage(parent, target, choiceQuery)
			if err != nil || parent.Err() != nil {
				return
			}
			c.mu.Lock()
			if c.closed {
				c.mu.Unlock()
				return
			}
			c.pendingChoices = append(c.pendingChoices, pendingChoiceUpdate{
				generation: gen,
				cursor:     cursor,
				node:       nodeStr,
				resources:  report.Resources,
			})
			c.mu.Unlock()
			c.wake()
		}()
	}
}

func (c *liveUsageController) loadHeatmap(gen uint64) {
	usageSvc := c.services.Usage
	if usageSvc == nil || usageSvc.ReadUsage == nil || c.activityWidth <= 0 || c.state.view == nil || c.state.view.report == nil {
		return
	}
	span := heatmapDays(c.activityWidth)
	spec, _ := rangeByKey(c.state.rangeKey)
	if spec.days < 30 || span <= spec.days {
		return
	}
	if c.heatCancel != nil {
		c.heatCancel()
		c.heatCancel = nil
	}
	parent := c.watchCtx
	if parent == nil {
		parent = c.ctx
	}
	hCtx, hCancel := context.WithCancel(parent)
	c.heatCancel = hCancel

	c.heatRequest++
	reqID := c.heatRequest
	cursor := c.state.resourceCursor()
	endMs := c.state.query.EndMs
	heatQuery := cloneUsageQuery(c.state.query)
	heatQuery.StartMs = endMs - int64(span)*dayMs
	heatQuery.Dimension = protocol.UsageDimensionModel
	heatQuery.Before = nil

	var nodes []protocol.NodeID
	if c.state.allNodes() {
		for _, src := range c.sources {
			nodes = append(nodes, src.ID)
		}
	} else if id, ok := parseNodeHex(c.state.node); ok {
		nodes = []protocol.NodeID{id}
	}
	if len(nodes) == 0 {
		hCancel()
		c.heatCancel = nil
		return
	}

	c.wg.Add(1)
	go func() {
		defer c.wg.Done()
		var (
			wg    sync.WaitGroup
			mu    sync.Mutex
			lists [][]protocol.UsagePoint
		)
		for _, nodeID := range nodes {
			wg.Add(1)
			go func(target protocol.NodeID) {
				defer wg.Done()
				report, err := usageSvc.ReadUsage(hCtx, target, heatQuery)
				if err != nil || hCtx.Err() != nil {
					return
				}
				mu.Lock()
				lists = append(lists, report.Days)
				mu.Unlock()
			}(nodeID)
		}
		wg.Wait()
		if hCtx.Err() != nil {
			return
		}
		var cache *heatmapCache
		if len(lists) > 0 {
			cache = &heatmapCache{
				span:   span,
				cursor: cursor,
				start:  heatQuery.StartMs,
				end:    endMs,
				offset: heatQuery.UtcOffsetMinutes,
				days:   mergeDays(lists),
			}
		}
		c.mu.Lock()
		if c.closed {
			c.mu.Unlock()
			return
		}
		c.pendingHeat = &pendingHeatUpdate{
			generation: gen,
			request:    reqID,
			cursor:     cursor,
			cache:      cache,
		}
		c.mu.Unlock()
		c.wake()
	}()
}

func (c *liveUsageController) Close() {
	c.mu.Lock()
	if c.closed {
		c.mu.Unlock()
		return
	}
	c.closed = true
	c.mu.Unlock()

	c.active = false
	if c.unsubChange != nil {
		c.unsubChange()
		c.unsubChange = nil
	}
	c.stopWatchAndReads()
	c.cancel()
}

func (c *liveUsageController) wait() {
	c.wg.Wait()
}

// usagePageHost implementation.
func (c *liveUsageController) State() *usagePageState            { return c.state }
func (c *liveUsageController) Inventory() []usageInventorySource { return c.inventory }
func (c *liveUsageController) InventoryRevision() uint64         { return c.invRev }
func (c *liveUsageController) Resources() []scopedResourceKey    { return c.resources }
func (c *liveUsageController) HeatmapCache() *heatmapCache       { return c.heat }
func (c *liveUsageController) PendingErrorKey() string           { return c.errorKey }
func (c *liveUsageController) ClearErrorKey()                    { c.errorKey = "" }

func (c *liveUsageController) Refreshing() bool {
	return c.state.view != nil && c.state.view.refreshing
}

func (c *liveUsageController) SelectFilter(field, value string) {
	if c.state.selectFilter(field, value) {
		c.observe()
	}
}

func (c *liveUsageController) SelectBreakdown(mode string) {
	if c.state.selectBreakdown(mode) {
		c.observe()
	}
}

func (c *liveUsageController) TurnPage(page int) {
	if c.state.turnPage(page) {
		c.observe()
	}
}

func (c *liveUsageController) Refresh() {
	moved := c.state.advance()
	if moved || c.state.page != 1 || c.lastToastKey != "" || c.watch.Refresh == nil {
		c.state.resetPagination()
		c.state.view = nil
		c.observe()
		return
	}
	refresh := c.watch.Refresh
	watchCtx := c.watchCtx
	if watchCtx == nil {
		watchCtx = c.ctx
	}
	c.wg.Add(1)
	go func() {
		defer c.wg.Done()
		_ = refresh(watchCtx)
	}()
}

func (c *liveUsageController) OnActivityWidth(width float32) {
	c.activityWidth = width
	if c.heat == nil || heatmapDays(width) > c.heat.span {
		c.loadHeatmap(c.state.generation)
	}
}

func (c *liveUsageController) WriteClipboard(text string) bool {
	if c.services.WriteClipboard != nil {
		_ = c.services.WriteClipboard(text)
		return true
	}
	return false
}

func (c *liveUsageController) render(uiCtx *ui.Context) {
	c.drainPending()
	c.renderer.render(uiCtx, c)
}

func projectOverview(ov viewapi.UsageOverview, st *usagePageState) usageViewState {
	var (
		connected   bool
		refreshing  bool
		firstErr    *protocol.Fault
		cursorParts = make([]string, 0, len(ov.Sources))
	)
	for _, src := range ov.Sources {
		if src.View.Connected {
			connected = true
		}
		if src.View.Refreshing {
			refreshing = true
		}
		if firstErr == nil && src.View.Error != nil {
			firstErr = src.View.Error
		}
		cur := ""
		if src.View.Report != nil {
			cur = strconv.FormatUint(src.View.Report.Cursor, 10)
		}
		cursorParts = append(cursorParts, nodeHex(src.Node)+":"+cur)
	}

	failure := ov.Error
	if failure == nil && ov.Summary == nil && !refreshing {
		failure = firstErr
	}

	var report *usageData
	if ov.Summary != nil {
		groups := make([]scopedUsageGroup, len(ov.Summary.Groups))
		for i, g := range ov.Summary.Groups {
			groups[i] = scopedUsageGroup{Node: nodeHex(g.Node), Group: g.Group}
		}
		items := make([]protocol.UsageRequest, len(ov.Summary.Requests.Items))
		for i, item := range ov.Summary.Requests.Items {
			items[i] = item.Request
		}
		report = &usageData{
			node:   st.node,
			query:  cloneUsageQuery(st.query),
			totals: ov.Summary.Totals,
			days:   ov.Summary.Days,
			recent: ov.Summary.Recent,
			groups: groups,
			requests: protocol.UsagePage{
				Items:   items,
				HasMore: ov.Summary.Requests.HasMore,
			},
		}
	}

	return usageViewState{
		report:         report,
		connected:      connected,
		refreshing:     refreshing,
		partial:        ov.Summary != nil && !ov.Complete,
		err:            failure,
		resourceCursor: strings.Join(cursorParts, "|"),
	}
}

func cloneUsageQuery(q protocol.UsageQuery) protocol.UsageQuery {
	cp := q
	cp.Projects = slices.Clone(q.Projects)
	cp.Worktrees = slices.Clone(q.Worktrees)
	cp.Providers = slices.Clone(q.Providers)
	cp.Models = slices.Clone(q.Models)
	if q.Before != nil {
		b := *q.Before
		cp.Before = &b
	}
	return cp
}

func errorKeyForFault(f *protocol.Fault) string {
	if f == nil {
		return ""
	}
	if f.Code == protocol.ErrorCodeInvalidRequest {
		return "usage_query_failed"
	}
	return "usage_read_failed"
}

func errorKeyForErr(err error) string {
	if err == nil {
		return ""
	}
	if f, ok := err.(*protocol.Fault); ok {
		return errorKeyForFault(f)
	}
	return "usage_read_failed"
}
