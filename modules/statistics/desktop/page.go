package statisticsdesktop

import (
	"fmt"
	"math"
	"slices"
	"strconv"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	usagePageContentMaxWidth float32 = 1040
	usageNarrowBreakpoint    float32 = 1000
	usageTableRowHeight      float32 = 36
	usageBreakdownEmptyH     float32 = 160
	usageRequestsEmptyH      float32 = 192
	usageMaxVisiblePages             = 7
)

type usageTableState struct {
	list      ui.ListState
	selected  int
	selection ui.Selection[int]
	revision  string
	rowIDs    []string
}

func (s *usageTableState) syncRevision(revision string, rowIDs []string) {
	if s.revision == revision && slices.Equal(s.rowIDs, rowIDs) {
		return
	}
	retained := map[string]struct{}{}
	for idx := range s.selection.All() {
		if idx >= 0 && idx < len(s.rowIDs) {
			retained[s.rowIDs[idx]] = struct{}{}
		}
	}
	s.selected = -1
	s.selection.Clear()
	for idx, id := range rowIDs {
		if _, ok := retained[id]; ok {
			s.selection.Add(idx)
			if s.selected < 0 {
				s.selected = idx
			}
		}
	}
	s.list = ui.ListState{Columns: s.list.Columns}
	s.revision = revision
	s.rowIDs = slices.Clone(rowIDs)
}

// usagePageHost abstracts live vs Demo data backing for the Usage workspace UI.
type usagePageHost interface {
	State() *usagePageState
	Inventory() []usageInventorySource
	InventoryRevision() uint64
	Resources() []scopedResourceKey
	HeatmapCache() *heatmapCache
	Refreshing() bool
	PendingErrorKey() string
	ClearErrorKey()
	SelectFilter(field, value string)
	SelectBreakdown(mode string)
	TurnPage(page int)
	Refresh()
	OnActivityWidth(width float32)
	WriteClipboard(text string) bool
}

type cachedSummary struct {
	data  *usageData
	value overviewSummary
}

type cachedTrend struct {
	data   *usageData
	unit   string
	offset int32
	locale string
	value  trendData
}

type cachedHeatmap struct {
	data   *usageData
	heat   *heatmapCache
	width  float32
	locale string
	valid  bool
	value  heatmapGrid
}

type cachedBreakdown struct {
	data   *usageData
	mode   string
	invRev uint64
	locale string
	rows   []breakdownItem
	notes  bool
}

type cachedRequests struct {
	data   *usageData
	locale string
	day    int
	year   int
	rows   []requestRowView
	rowIDs []string
}

type usagePageRenderer struct {
	scroll        ui.ScrollState
	breakdownTbl  usageTableState
	requestsTbl   usageTableState
	activityWidth float32
	contentWidth  float32
	summaryCache  cachedSummary
	trendCache    cachedTrend
	heatCache     cachedHeatmap
	breakCache    cachedBreakdown
	reqCache      cachedRequests
}

func newUsagePageRenderer() *usagePageRenderer {
	return &usagePageRenderer{
		breakdownTbl: usageTableState{selected: -1},
		requestsTbl:  usageTableState{selected: -1},
	}
}

func usageWorkspaceSlots(refreshing func() bool, onRefresh func()) shell.WorkspaceSlots {
	return shell.WorkspaceSlots{
		Title: func(c *ui.Context) {
			ui.Text(c, i18n.T(c, "usage")).FontSize(14).SingleLine().Shrink(1).TextColor(theme.Of(c).Foreground)
		},
		Header: func(c *ui.Context) {
			busy := refreshing != nil && refreshing()
			b := kit.IconButton(c.Key("usage-refresh"), icons.RotateCw, i18n.T(c, "usage_refresh"), shell.HeaderButton, false).Disabled(busy)
			if busy {
				b.Opacity(0.5)
			}
			b.OnClick(func() {
				if onRefresh != nil {
					onRefresh()
				}
			})
		},
	}
}

func (r *usagePageRenderer) render(c *ui.Context, host usagePageHost) {
	t := func(key string, args ...any) string { return i18n.T(c, key, args...) }
	if errKey := host.PendingErrorKey(); errKey != "" {
		kit.Toast(c, kit.Failed, t(errKey))
		host.ClearErrorKey()
	}

	st := host.State()
	data := st.data()

	ui.Scroll(c.Key("usage-scroll")).Fill().TrackScroll(&r.scroll).Children(func() {
		body := ui.Column(c.Key("usage-content")).FillWidth().MaxWidth(usagePageContentMaxWidth).Margin(0, ui.Auto).Padding(24).Gap(16)
		if w := body.LayoutBounds().W; w > 48 {
			r.contentWidth = w - 48
		}
		body.Children(func() {
			r.renderFilters(c, host, t)
			if data != nil {
				r.renderCard(c, "usage-summary", func() {
					r.renderSummary(c, data, t)
				})
				r.renderCard(c, "usage-trend-card", func() {
					r.renderTrend(c, st, data, t)
				})
				if rangeHasCalendar(st.rangeKey) {
					r.renderCard(c, "usage-activity-card", func() {
						r.renderActivity(c, host, st, data, t)
					})
				}
				r.renderCard(c, "usage-breakdown-card", func() {
					r.renderBreakdown(c, host, st, data, t)
				})
				r.renderCard(c, "usage-requests-card", func() {
					r.renderRequests(c, host, st, data, t)
				})
			} else if host.Refreshing() {
				r.renderCard(c, "usage-loading", func() {
					ui.Row(c).FillWidth().Height(160).Center().Children(func() {
						kit.Loader(c, 24)
					})
				})
			}
		})
	})
}

func (r *usagePageRenderer) renderCard(c *ui.Context, key string, body func()) {
	kit.Card(c.Key(key), func() {
		ui.Column(c).FillWidth().MinWidth(0).Padding(16, 0).Gap(12).Children(body)
	})
}

func (r *usagePageRenderer) renderFilters(c *ui.Context, host usagePageHost, t translateFunc) {
	st := host.State()
	opts := buildFilterChoices(st, host.Inventory(), host.Resources(), t)

	rangeIdx := 0
	rangeLabels := make([]string, len(usageRanges))
	for i, spec := range usageRanges {
		rangeLabels[i] = t("usage_range_" + spec.key)
		if spec.key == st.rangeKey {
			rangeIdx = i
		}
	}

	toSelectOptions := func(items []filterChoice) []kit.SelectOption {
		out := make([]kit.SelectOption, len(items))
		for i, item := range items {
			out[i] = kit.SelectOption{Value: item.id, Label: item.label}
		}
		return out
	}

	ui.Row(c.Key("usage-filters")).FillWidth().Wrap().Justify(ui.SpaceBetween).AlignItems(ui.Center).Gap(12).Children(func() {
		if kit.Segmented(c, "usage-filter-range", &rangeIdx, rangeLabels, kit.SegmentedProps{}).Label(t("usage_range")).Changed() {
			if rangeIdx >= 0 && rangeIdx < len(usageRanges) {
				host.SelectFilter("range", usageRanges[rangeIdx].key)
			}
		}
		ui.Row(c.Key("usage-filter-selects")).Wrap().Gap(8).MinWidth(0).Children(func() {
			projVal := st.selectedValue("project")
			if el, changed := kit.SelectField(c.Key("project"), "usage-filter-project", &projVal, kit.SelectProps{
				Label:   t("usage_all_projects"),
				Options: toSelectOptions(opts.projects),
			}); el.Width(176).MinWidth(0) != (ui.Element{}) && changed {
				host.SelectFilter("project", projVal)
			}

			provVal := st.selectedValue("provider")
			if el, changed := kit.SelectField(c.Key("provider"), "usage-filter-provider", &provVal, kit.SelectProps{
				Label:   t("usage_all_providers"),
				Options: toSelectOptions(opts.providers),
			}); el.Width(176).MinWidth(0) != (ui.Element{}) && changed {
				host.SelectFilter("provider", provVal)
			}

			modelVal := st.selectedValue("model")
			if el, changed := kit.SelectField(c.Key("model"), "usage-filter-model", &modelVal, kit.SelectProps{
				Label:   t("usage_all_models"),
				Options: toSelectOptions(opts.models),
			}); el.Width(176).MinWidth(0) != (ui.Element{}) && changed {
				host.SelectFilter("model", modelVal)
			}
		})
	})
}

func tokenLabelText(t translateFunc, value string, ok bool, partial bool) string {
	if !ok {
		return t("composer_metric_unknown")
	}
	if partial {
		return t("usage_token_subtotal", "value", value)
	}
	return value
}

func (r *usagePageRenderer) summaryFor(data *usageData) overviewSummary {
	if r.summaryCache.data == data {
		return r.summaryCache.value
	}
	v := computeOverview(data.totals)
	r.summaryCache = cachedSummary{data: data, value: v}
	return v
}

func (r *usagePageRenderer) trendFor(data *usageData, unit, locale string, t translateFunc) trendData {
	offset := data.query.UtcOffsetMinutes
	if r.trendCache.data == data && r.trendCache.unit == unit && r.trendCache.offset == offset && r.trendCache.locale == locale {
		return r.trendCache.value
	}
	v := buildTrend(data.days, data.recent, unit, offset, t)
	r.trendCache = cachedTrend{data: data, unit: unit, offset: offset, locale: locale, value: v}
	return v
}

func (r *usagePageRenderer) heatmapFor(data *usageData, heat *heatmapCache, width float32, locale string, t translateFunc) heatmapGrid {
	if r.heatCache.valid && r.heatCache.data == data && r.heatCache.heat == heat && r.heatCache.width == width && r.heatCache.locale == locale {
		return r.heatCache.value
	}
	points := data.days
	startMs := data.query.StartMs
	endMs := data.query.EndMs
	offset := data.query.UtcOffsetMinutes
	if heat != nil {
		points = heat.days
		startMs = heat.start
		endMs = heat.end
		offset = heat.offset
	}
	v := buildHeatmap(points, t, width, startMs, endMs, offset)
	r.heatCache = cachedHeatmap{data: data, heat: heat, width: width, locale: locale, valid: true, value: v}
	return v
}

func (r *usagePageRenderer) breakdownFor(data *usageData, mode string, inv []usageInventorySource, invRev uint64, locale string, t translateFunc) ([]breakdownItem, bool) {
	if r.breakCache.data == data && r.breakCache.mode == mode && r.breakCache.invRev == invRev && r.breakCache.locale == locale {
		return r.breakCache.rows, r.breakCache.notes
	}
	names := buildCatalogNames(inv, data.node)
	rows := buildBreakdown(data.groups, mode, names, t)
	notes := breakdownAnnotated(rows)
	r.breakCache = cachedBreakdown{data: data, mode: mode, invRev: invRev, locale: locale, rows: rows, notes: notes}
	return rows, notes
}

func (r *usagePageRenderer) requestsFor(data *usageData, now time.Time, locale string, t translateFunc) ([]requestRowView, []string) {
	localNow := now.In(time.Local)
	if r.reqCache.data == data && r.reqCache.locale == locale && r.reqCache.year == localNow.Year() && r.reqCache.day == localNow.YearDay() {
		return r.reqCache.rows, r.reqCache.rowIDs
	}
	rows := formatRequestRows(data.requests.Items, t, now, time.Local)
	rowIDs := make([]string, len(rows))
	for i, rv := range rows {
		rowIDs[i] = rv.id
	}
	r.reqCache = cachedRequests{data: data, locale: locale, year: localNow.Year(), day: localNow.YearDay(), rows: rows, rowIDs: rowIDs}
	return rows, rowIDs
}

func (r *usagePageRenderer) renderSummary(c *ui.Context, data *usageData, t translateFunc) {
	p := theme.Of(c)
	unknown := t("composer_metric_unknown")
	ov := r.summaryFor(data)
	partialTokens := ov.reported < ov.responses
	coverageText := ""
	if partialTokens {
		coverageText = t("usage_token_coverage", "reported", formatGrouped(ov.reported), "responses", formatGrouped(ov.responses))
	}
	viewportW, _ := c.Size()
	narrow := viewportW > 0 && viewportW < usageNarrowBreakpoint

	tokensSection := func() {
		ui.Column(c.Key("tokens")).Grow(1).Shrink(1).MinWidth(0).Children(func() {
			ui.Text(c, t("composer_tokens")).FontSize(12).TextColor(p.MutedForeground)
			compactStr := ""
			groupedStr := ""
			if ov.hasTotal {
				compactStr = formatCompact(ov.total)
				groupedStr = formatGrouped(ov.total)
			}
			totalEl := ui.Text(c.Key("usage-total"), tokenLabelText(t, compactStr, ov.hasTotal, partialTokens)).
				FontSize(30).FontWeight(600).SingleLine().FontFeatures("tnum").Margin(4, 0, 0, 0)
			kit.Tooltip(c, totalEl, tokenLabelText(t, groupedStr, ov.hasTotal, partialTokens))

			if partialTokens {
				ui.Text(c.Key("usage-token-coverage"), coverageText).FontSize(12).TextColor(p.MutedForeground).Margin(2, 0, 0, 0)
			}

			// 3-part proportional composition bar (Cached chart_3, Uncached chart_2, Output chart_4).
			ui.Row(c.Key("usage-composition")).FillWidth().Height(8).Margin(12, 0, 0, 0).Gap(2).Radius(999).Clip().Background(p.Muted).Children(func() {
				if ov.hasTotal && ov.total > 0 {
					for i, spec := range tokenSeries {
						if ov.parts[i] == 0 {
							continue
						}
						seg := ui.Box(c.Key(spec.id)).FillHeight().Basis(0).Grow(float32(ov.parts[i]) / float32(ov.total)).Background(p.Chart[spec.colorIdx])
						if i == 2 {
							seg.MinWidth(3)
						}
					}
				}
			})

			// 2x2 legend: Reasoning is a subset of Output so it uses a neutral dot and no percentage.
			legendCell := func(key string, dotColor ui.Color, name string, val string, share string, showShare bool) {
				ui.Row(c.Key(key)).Grow(1).Shrink(1).MinWidth(0).Gap(6).AlignItems(ui.Center).Children(func() {
					ui.Box(c).Size(8, 8).Shrink(0).Radius(2).Background(dotColor)
					ui.Text(c, name).FontSize(12).SingleLine().TextColor(p.MutedForeground)
					ui.Text(c, val).FontSize(12).FontWeight(500).SingleLine().FontFeatures("tnum")
					if showShare && share != "" {
						ui.Text(c, share).FontSize(12).SingleLine().FontFeatures("tnum").TextColor(p.MutedForeground)
					}
				})
			}
			partText := func(idx int) (string, string) {
				if !ov.hasParts {
					return unknown, ""
				}
				share, _ := formatPercentOneDecimal(ov.shares[idx], ov.shareKnown[idx])
				return formatCompact(ov.parts[idx]), share
			}
			cVal, cShare := partText(0)
			uVal, uShare := partText(1)
			oVal, oShare := partText(2)
			rVal := unknown
			if ov.hasReasoning {
				rVal = formatCompact(ov.reasoning)
			}
			ui.Column(c.Key("usage-parts")).FillWidth().Gap(6).Margin(12, 0, 0, 0).Children(func() {
				ui.Row(c.Key("row-0")).FillWidth().Gap(14).Children(func() {
					legendCell("cache", p.Chart[tokenSeries[0].colorIdx], t(tokenSeries[0].labelKey), cVal, cShare, true)
					legendCell("input", p.Chart[tokenSeries[1].colorIdx], t(tokenSeries[1].labelKey), uVal, uShare, true)
				})
				ui.Row(c.Key("row-1")).FillWidth().Gap(14).Children(func() {
					legendCell("output", p.Chart[tokenSeries[2].colorIdx], t(tokenSeries[2].labelKey), oVal, oShare, true)
					legendCell("reasoning", p.MutedForeground, t("usage_column_reasoning"), rVal, "", false)
				})
			})
		})
	}

	statBlock := func(key, name, value string, extra func(), under func()) {
		ui.Column(c.Key("usage-stat-" + key)).Grow(1).Shrink(1).MinWidth(0).Gap(4).Children(func() {
			ui.Text(c, name).FontSize(12).SingleLine().TextColor(p.MutedForeground)
			ui.Row(c).AlignItems(ui.Center).Gap(6).MinWidth(0).Children(func() {
				ui.Text(c, value).FontSize(18).FontWeight(600).SingleLine().FontFeatures("tnum").Shrink(1).MinWidth(0)
				if extra != nil {
					extra()
				}
			})
			if under != nil {
				under()
			}
		})
	}

	hitVal := unknown
	if ov.hasHit {
		hitVal = fmt.Sprintf("%.0f%%", math.Round(ov.hit))
	}
	speedVal := unknown
	if ov.hasSpeed {
		speedVal = ov.speed
	}
	costVal := unknown
	if ov.hasCost {
		costVal = "$" + ov.cost
	}
	pricedTip := t("usage_cost_priced", "priced", formatGrouped(ov.priced), "responses", formatGrouped(ov.responses))

	renderHit := func() {
		var under func()
		if !partialTokens {
			hitPct := 0.0
			if ov.hasHit {
				hitPct = ov.hit
			}
			under = func() {
				ui.Progress(c.Key("usage-hit-bar"), min(max(hitPct, 0), 100)/100).
					Width(96).Height(4).TextColor(p.Chart[2]).Label(t("usage_hit_rate"))
			}
		}
		statBlock("hit", t("usage_hit_rate"), hitVal, nil, under)
	}
	renderResponses := func() {
		statBlock("responses", t("usage_responses"), formatGrouped(ov.responses), nil, nil)
	}
	renderSpeed := func() {
		var extra func()
		if ov.hasSpeed {
			extra = func() { ui.Text(c, t("usage_speed_unit")).FontSize(12).SingleLine().TextColor(p.MutedForeground) }
		}
		statBlock("speed", t("usage_speed"), speedVal, extra, nil)
	}
	renderCost := func() {
		var extra func()
		if ov.partialCost {
			extra = func() {
				badge := kit.Badge(c.Key("usage-cost-partial"), t("usage_cost_partial"), kit.Neutral)
				kit.Tooltip(c, badge, pricedTip)
			}
		}
		statBlock("cost", t("usage_cost"), costVal, extra, nil)
	}

	if narrow {
		ui.Column(c.Key("summary-narrow")).FillWidth().MinWidth(0).Children(func() {
			tokensSection()
			ui.Divider(c).Background(p.Border).Margin(16, 0, 16, 0)
			ui.Row(c.Key("stats-narrow")).FillWidth().Wrap().Gap(20).Children(func() {
				renderHit()
				renderResponses()
				renderSpeed()
				renderCost()
			})
		})
		return
	}

	ui.Row(c.Key("summary-wide")).FillWidth().MinWidth(0).AlignItems(ui.Stretch).Gap(24).Children(func() {
		tokensSection()
		ui.Box(c.Key("divider")).Width(1).FillHeight().Shrink(0).Background(p.Border)
		ui.Column(c.Key("stats-wide")).Width(356).Shrink(0).Justify(ui.Center).Gap(20).Children(func() {
			ui.Row(c.Key("stats-top")).FillWidth().Gap(20).Children(func() {
				renderHit()
				renderResponses()
			})
			ui.Row(c.Key("stats-bottom")).FillWidth().Gap(20).Children(func() {
				renderSpeed()
				renderCost()
			})
		})
	})
}

func (r *usagePageRenderer) renderTrend(c *ui.Context, st *usagePageState, data *usageData, t translateFunc) {
	p := theme.Of(c)
	granularity := rangeUnit(st.rangeKey)
	trend := r.trendFor(data, granularity, i18n.Match(c.Locale()), t)

	ui.Row(c.Key("header")).FillWidth().Justify(ui.SpaceBetween).AlignItems(ui.Center).Gap(12).Children(func() {
		ui.Text(c, t("usage_trend")).FontSize(14).FontWeight(600)
		ui.Row(c.Key("swatches")).Wrap().Gap(12).Children(func() {
			for _, spec := range tokenSeries {
				ui.Row(c.Key(spec.id)).Gap(6).AlignItems(ui.Center).Children(func() {
					ui.Box(c).Size(8, 8).Shrink(0).Radius(2).Background(p.Chart[spec.colorIdx])
					ui.Text(c, t(spec.labelKey)).FontSize(12).SingleLine().TextColor(p.MutedForeground)
				})
			}
		})
	})
	renderStackedChart(c.Key("trend-"+granularity), trend)
	ui.Row(c.Key("caption")).FillWidth().Justify(ui.End).Children(func() {
		ui.Text(c, t("usage_local_time")).FontSize(12).TextColor(p.MutedForeground)
	})
}

func (r *usagePageRenderer) renderActivity(c *ui.Context, host usagePageHost, st *usagePageState, data *usageData, t translateFunc) {
	ui.Text(c, t("usage_heatmap")).FontSize(14).FontWeight(600)

	container := ui.Column(c.Key("usage-activity-width")).FillWidth().MinWidth(0)
	measured := container.LayoutBounds().W
	if measured <= 0 && r.contentWidth > 32 {
		measured = r.contentWidth - 32
	}
	if measured > 0 && math.Abs(float64(measured-r.activityWidth)) >= 1 {
		r.activityWidth = measured
		host.OnActivityWidth(measured)
	}

	var heat *heatmapCache
	if st.previous == nil {
		if hc := host.HeatmapCache(); hc != nil && hc.cursor == st.resourceCursor() {
			heat = hc
		}
	}

	locale := i18n.Match(c.Locale())
	container.Children(func() {
		if r.activityWidth > 0 {
			grid := r.heatmapFor(data, heat, r.activityWidth, locale, t)
			renderHeatmap(c.Key("usage-heatmap-plot"), grid)
		}
	})
}

func (r *usagePageRenderer) renderBreakdown(c *ui.Context, host usagePageHost, st *usagePageState, data *usageData, t translateFunc) {
	p := theme.Of(c)
	modeIdx := max(slices.Index(breakdownModes, st.breakdown), 0)
	modeLabels := []string{t("usage_column_model"), t("usage_column_provider"), t("usage_column_project")}

	ui.Row(c.Key("header")).FillWidth().Justify(ui.SpaceBetween).AlignItems(ui.Center).Gap(12).Children(func() {
		ui.Text(c, t("usage_breakdown")).FontSize(14).FontWeight(600)
		if kit.Segmented(c, "usage-breakdown-tabs", &modeIdx, modeLabels, kit.SegmentedProps{}).Label(t("usage_breakdown")).Changed() {
			if modeIdx >= 0 && modeIdx < len(breakdownModes) {
				host.SelectBreakdown(breakdownModes[modeIdx])
			}
		}
	})

	pending := data.query.Dimension != st.query.Dimension
	if pending {
		ui.Row(c.Key("pending")).FillWidth().Height(usageBreakdownEmptyH).Center().Children(func() {
			kit.Loader(c, 24)
		})
		return
	}

	rows, showNotes := r.breakdownFor(data, st.breakdown, host.Inventory(), host.InventoryRevision(), i18n.Match(c.Locale()), t)
	shown := rows
	if !st.expanded && len(shown) > breakdownVisible {
		shown = shown[:breakdownVisible]
	}
	hasSubtotal := slices.ContainsFunc(rows, func(item breakdownItem) bool {
		return item.reported < item.responses
	})

	shareColTitle := t("usage_column_share")
	tokensColW := float32(80)
	if hasSubtotal {
		shareColTitle = t("usage_column_subtotal_share")
		tokensColW = 200
	}
	cols := []ui.TableColumn{
		{ID: "0", Title: t("usage_column_name"), Width: 0, MinWidth: breakdownNameWidth, MaxWidth: 4096, Fixed: true, Align: ui.Start},
		{ID: "1", Title: shareColTitle, Width: 150, MinWidth: 16, MaxWidth: 4096, Fixed: true, Align: ui.End},
		{ID: "2", Title: t("usage_column_tokens"), Width: tokensColW, MinWidth: 16, MaxWidth: 4096, Fixed: true, Align: ui.End},
		{ID: "3", Title: t("usage_column_hit"), Width: 90, MinWidth: 16, MaxWidth: 4096, Fixed: true, Align: ui.End},
		{ID: "4", Title: t("usage_column_responses"), Width: 90, MinWidth: 16, MaxWidth: 4096, Fixed: true, Align: ui.End},
		{ID: "5", Title: t("usage_column_cost"), Width: 90, MinWidth: 16, MaxWidth: 4096, Fixed: true, Align: ui.End},
	}

	rowIDs := make([]string, len(shown))
	for i, item := range shown {
		rowIDs[i] = item.id
	}
	rev := fmt.Sprintf("%d:%s:%t", st.generation, st.breakdown, st.expanded)
	r.breakdownTbl.syncRevision(rev, rowIDs)
	r.breakdownTbl.list.Selected = &r.breakdownTbl.selected
	r.breakdownTbl.list.Selection = &r.breakdownTbl.selection

	unknown := t("composer_metric_unknown")
	headHeight := theme.Neutral(c).Space(8)
	frameH := headHeight + usageBreakdownEmptyH
	if len(shown) > 0 {
		frameH = kit.TableHeight(c, len(shown), usageTableRowHeight)
	}

	frame := kit.TableFrame(c, ui.Column(c.Key("usage-breakdown-table")).FillWidth().MinWidth(0).Height(frameH))
	frame.Children(func() {
		area := ui.Themed(c.Key("neutral"), theme.Neutral(c)).Background(ui.Transparent).FillWidth().MinHeight(0)
		if len(shown) > 0 {
			area.Grow(1)
		} else {
			area.Height(headHeight + 1).Shrink(0)
		}
		area.Children(func() {
			ui.Table(c, &r.breakdownTbl.list, cols, len(shown), func(row, col int) {
				item := shown[row]
				partial := item.reported < item.responses
				cell := kit.TableCell(c, usageTableRowHeight, row+1 == len(shown))
				if col > 0 {
					cell.AlignItems(ui.End)
				}
				switch col {
				case 0:
					tip := item.name
					if item.note != "" {
						tip = item.name + " · " + item.note
					}
					cell.Tooltip(tip).Children(func() {
						ui.Row(c).Gap(6).MinWidth(0).AlignItems(ui.Center).Children(func() {
							ui.Text(c, item.name).FontSize(14).SingleLine().TextColor(p.Foreground)
							if showNotes && item.note != "" {
								ui.Text(c, item.note).FontSize(14).SingleLine().TextColor(p.MutedForeground)
							}
						})
					})
				case 1:
					shareStr, hasShare := formatPercentOneDecimal(item.share, item.hasShare)
					tip := unknown
					if hasShare {
						tip = shareStr
					}
					cell.Tooltip(tip).Children(func() {
						if !hasShare {
							ui.Text(c, unknown).FontSize(14).SingleLine().TextColor(p.MutedForeground)
							return
						}
						ui.Row(c).Gap(6).AlignItems(ui.Center).Children(func() {
							ui.Text(c, shareStr).FontSize(14).SingleLine().FontFeatures("tnum").TextColor(p.Foreground)
							ui.Progress(c, min(max(item.share, 0), 100)/100).Width(72).Height(4).Label(shareStr)
						})
					})
				case 2:
					compactVal := ""
					exactVal := ""
					if item.hasTotal {
						compactVal = formatCompact(item.total)
						exactVal = strconv.FormatUint(item.total, 10)
					}
					display := tokenLabelText(t, compactVal, item.hasTotal, partial)
					tip := tokenLabelText(t, exactVal, item.hasTotal, partial)
					if partial {
						tip += "\n" + t("usage_token_coverage", "reported", formatGrouped(item.reported), "responses", formatGrouped(item.responses))
					}
					cell.Tooltip(tip).Children(func() {
						ui.Row(c).Gap(6).AlignItems(ui.Center).Children(func() {
							txt := ui.Text(c, display).FontSize(14).SingleLine().FontFeatures("tnum")
							if !item.hasTotal {
								txt.TextColor(p.MutedForeground)
							}
							if partial {
								ui.Text(c, formatGrouped(item.reported)+"/"+formatGrouped(item.responses)).FontSize(12).SingleLine().TextColor(p.MutedForeground)
							}
						})
					})
				case 3:
					if !item.hasHit {
						cell.Tooltip(unknown).Children(func() {
							ui.Text(c, unknown).FontSize(14).SingleLine().TextColor(p.MutedForeground)
						})
					} else {
						hitStr := fmt.Sprintf("%.0f%%", math.Round(item.hit))
						cell.Tooltip(hitStr).Children(func() {
							ui.Text(c, hitStr).FontSize(14).SingleLine().FontFeatures("tnum")
						})
					}
				case 4:
					respStr := formatGrouped(item.responses)
					cell.Tooltip(strconv.FormatUint(item.responses, 10)).Children(func() {
						ui.Text(c, respStr).FontSize(14).SingleLine().FontFeatures("tnum")
					})
				case 5:
					if !item.hasCost {
						cell.Tooltip(unknown).Children(func() {
							ui.Text(c, unknown).FontSize(14).SingleLine().TextColor(p.MutedForeground)
						})
					} else {
						costStr := "$" + item.cost
						cell.Tooltip(costStr).Children(func() {
							ui.Text(c, costStr).FontSize(14).SingleLine().FontFeatures("tnum")
						})
					}
				}
			}).Grow(1).FillWidth()
		})
		if len(shown) == 0 {
			ui.Column(c.Key("empty")).Grow(1).FillWidth().MinHeight(0).AlignItems(ui.Center).Justify(ui.Center).Children(func() {
				kit.Result(c, kit.ResultProps{Title: t("usage_empty"), Icon: icons.ChartPie})
			})
		}
	})

	if len(rows) > breakdownVisible {
		moreLabel := t("usage_show_all")
		if st.expanded {
			moreLabel = t("usage_collapse")
		}
		ui.Row(c.Key("more-row")).Children(func() {
			kit.TextAction(c.Key("usage-breakdown-more"), moreLabel, false).OnClick(func() {
				st.expanded = !st.expanded
			})
		})
	}
}

func (r *usagePageRenderer) renderRequests(c *ui.Context, host usagePageHost, st *usagePageState, data *usageData, t translateFunc) {
	p := theme.Of(c)
	ui.Text(c, t("usage_requests")).FontSize(14).FontWeight(600)

	rows, rowIDs := r.requestsFor(data, st.now(), i18n.Match(c.Locale()), t)
	rev := fmt.Sprintf("%d:%s:%d", st.generation, st.resourceCursor(), st.page)
	r.requestsTbl.syncRevision(rev, rowIDs)
	r.requestsTbl.list.Selected = &r.requestsTbl.selected
	r.requestsTbl.list.Selection = &r.requestsTbl.selection

	cols := []ui.TableColumn{
		{ID: "0", Title: t("usage_column_time"), Width: 116, MinWidth: 16, MaxWidth: 4096, Fixed: true, Align: ui.Start},
		{ID: "1", Title: t("usage_column_session"), Width: 0, MinWidth: 160, MaxWidth: 4096, Fixed: true, Align: ui.Start},
		{ID: "2", Title: t("usage_column_model"), Width: 204, MinWidth: 16, MaxWidth: 4096, Fixed: true, Align: ui.Start},
		{ID: "3", Title: t("usage_column_input"), Width: 72, MinWidth: 16, MaxWidth: 4096, Fixed: true, Align: ui.End},
		{ID: "4", Title: t("usage_column_cached"), Width: 64, MinWidth: 16, MaxWidth: 4096, Fixed: true, Align: ui.End},
		{ID: "5", Title: t("usage_column_output"), Width: 72, MinWidth: 16, MaxWidth: 4096, Fixed: true, Align: ui.End},
		{ID: "6", Title: t("usage_column_first_token"), Width: 88, MinWidth: 16, MaxWidth: 4096, Fixed: true, Align: ui.End},
		{ID: "7", Title: t("usage_column_duration"), Width: 80, MinWidth: 16, MaxWidth: 4096, Fixed: true, Align: ui.End},
		{ID: "8", Title: t("usage_column_cost"), Width: 84, MinWidth: 16, MaxWidth: 4096, Fixed: true, Align: ui.End},
	}

	headHeight := theme.Neutral(c).Space(8)
	frameH := headHeight + usageRequestsEmptyH
	if len(rows) > 0 {
		frameH = kit.TableHeight(c, len(rows), usageTableRowHeight)
	}

	toneColor := func(tone requestCellTone) ui.Color {
		switch tone {
		case toneMuted:
			return p.MutedForeground
		case toneWarning:
			return p.Warning
		default:
			return p.Foreground
		}
	}

	frame := kit.TableFrame(c, ui.Column(c.Key("usage-table")).FillWidth().MinWidth(0).Height(frameH))
	frame.Children(func() {
		area := ui.Themed(c.Key("neutral"), theme.Neutral(c)).Background(ui.Transparent).FillWidth().MinHeight(0)
		if len(rows) > 0 {
			area.Grow(1)
		} else {
			area.Height(headHeight + 1).Shrink(0)
		}
		var tblEl ui.Element
		area.Children(func() {
			tblEl = ui.Table(c, &r.requestsTbl.list, cols, len(rows), func(row, col int) {
				rc := rows[row].cells[col]
				cell := kit.TableCell(c, usageTableRowHeight, row+1 == len(rows))
				if col >= 3 {
					cell.AlignItems(ui.End)
				}
				cell.Tooltip(rc.tooltip).Children(func() {
					txt := ui.Text(c, rc.text).FontSize(14).SingleLine().TextColor(toneColor(rc.tone))
					if col == 0 || col >= 3 {
						txt.FontFeatures("tnum")
					}
				})
			}).Grow(1).FillWidth()
		})
		if len(rows) == 0 {
			ui.Column(c.Key("empty")).Grow(1).FillWidth().MinHeight(0).AlignItems(ui.Center).Justify(ui.Center).Children(func() {
				kit.Result(c, kit.ResultProps{Title: t("usage_empty"), Icon: icons.ChartPie})
			})
		} else if tblEl.Shortcut(ui.Cmd, ui.KeyC) {
			var selected []int
			for idx := range r.requestsTbl.selection.All() {
				selected = append(selected, idx)
			}
			slices.Sort(selected)
			if len(selected) > 0 {
				tsv := formatRequestsTSV(rows, selected)
				if !host.WriteClipboard(tsv) {
					c.Services().WriteClipboard(tsv)
				}
			}
		}
	})

	// Pagination footer
	ui.Row(c.Key("usage-pagination")).FillWidth().AlignItems(ui.Center).Justify(ui.SpaceBetween).Gap(12).Children(func() {
		ui.Text(c, t("usage_count", "count", formatGrouped(data.totals.Responses))).FontSize(12).TextColor(p.MutedForeground)
		if st.page > 1 || data.requests.HasMore {
			totalPages := len(st.cursors)
			nextPage := st.page
			if data.requests.HasMore {
				nextPage++
			}
			if nextPage > totalPages {
				totalPages = nextPage
			}
			disabled := !st.ready()
			ui.Row(c.Key("usage-pages")).Gap(4).AlignItems(ui.Center).Children(func() {
				kit.IconButton(c.Key("prev"), icons.ChevronLeft, t("usage_previous"), 28, false).
					Disabled(disabled || st.page <= 1).
					OnClick(func() { host.TurnPage(max(1, st.page-1)) })

				start := max(1, min(st.page-usageMaxVisiblePages/2, totalPages-usageMaxVisiblePages+1))
				for pg := start; pg <= totalPages && pg < start+usageMaxVisiblePages; pg++ {
					pageNum := pg
					kit.Ghost(c.Key(pageNum), strconv.Itoa(pageNum), pageNum == st.page).
						Size(28, 28).Disabled(disabled).
						Children(func() { ui.Text(c, strconv.Itoa(pageNum)).FontSize(13).SingleLine() }).
						OnClick(func() { host.TurnPage(pageNum) })
				}

				kit.IconButton(c.Key("next"), icons.ChevronRight, t("usage_next"), 28, false).
					Disabled(disabled || st.page >= totalPages).
					OnClick(func() { host.TurnPage(min(totalPages, st.page+1)) })
			})
		}
	})
}

// demoUsageController serves DemoUsage in memory without contacting any Node,
// resetting cleanly whenever the Demo ledger or locale changes.
type demoUsageController struct {
	ledger        *preview.Usage
	locale        string
	node          protocol.NodeID
	state         *usagePageState
	inventory     []usageInventorySource
	resources     []scopedResourceKey
	heat          *heatmapCache
	activityWidth float32
	errorKey      string
	renderer      *usagePageRenderer
}

type demoControllerKey struct{}

func demoControllerFor(c *ui.Context, ledger *preview.Usage, now func() time.Time) *demoUsageController {
	ptr := ui.Local(c.Root(), demoControllerKey{}, func() *demoUsageController {
		return &demoUsageController{renderer: newUsagePageRenderer()}
	})
	ctrl := *ptr
	locale := i18n.Match(c.Locale())
	if ctrl.ledger != ledger || ctrl.locale != locale {
		ctrl.reset(ledger, locale, now)
	}
	return ctrl
}

func (d *demoUsageController) reset(ledger *preview.Usage, locale string, now func() time.Time) {
	d.ledger = ledger
	d.locale = locale
	d.heat = nil
	d.errorKey = ""
	d.renderer = newUsagePageRenderer()
	var node protocol.NodeID
	node[0] = 0xde
	d.node = node
	d.state = newUsagePageState(node, now)
	if ledger == nil {
		d.inventory = nil
		d.resources = nil
		return
	}
	projects, providers := ledger.Inventory()
	d.inventory = []usageInventorySource{{
		node:      nodeHex(node),
		label:     i18n.In(locale, "local_host_name"),
		projects:  projects,
		providers: providers,
	}}
	d.reload()
}

func (d *demoUsageController) reload() {
	if d.ledger == nil {
		return
	}
	gen := d.state.nextGeneration()
	d.heat = nil
	report, err := d.ledger.Report(d.node, d.state.query)
	if err != nil {
		d.errorKey = "usage_read_failed"
		if f, ok := err.(*protocol.Fault); ok && f.Code == protocol.ErrorCodeInvalidRequest {
			d.errorKey = "usage_query_failed"
		}
		return
	}
	d.errorKey = ""
	d.state.accept(usageViewState{
		report:         reportToUsageData(report),
		connected:      true,
		resourceCursor: singleReportCursor(report),
	}, gen)

	// Load filter choice resources over the same window without provider/model filters.
	choiceQuery := d.state.query
	choiceQuery.Providers = []protocol.ProviderID{}
	choiceQuery.Models = []string{}
	choiceQuery.Before = nil
	if resReport, rerr := d.ledger.Report(d.node, choiceQuery); rerr == nil {
		d.resources = make([]scopedResourceKey, len(resReport.Resources))
		for i, k := range resReport.Resources {
			d.resources[i] = scopedResourceKey{node: nodeHex(d.node), key: k}
		}
	}
	d.loadHeatmap()
}

func (d *demoUsageController) loadHeatmap() {
	if d.ledger == nil || d.activityWidth <= 0 || d.state.view == nil || d.state.view.report == nil {
		return
	}
	span := heatmapDays(d.activityWidth)
	spec, _ := rangeByKey(d.state.rangeKey)
	if spec.days < 30 || span <= spec.days {
		return
	}
	q := d.state.query
	q.StartMs = q.EndMs - int64(span)*dayMs
	q.Dimension = protocol.UsageDimensionModel
	q.Before = nil
	report, err := d.ledger.Report(d.node, q)
	if err != nil {
		return
	}
	d.heat = &heatmapCache{
		span:   span,
		cursor: d.state.resourceCursor(),
		start:  q.StartMs,
		end:    q.EndMs,
		offset: q.UtcOffsetMinutes,
		days:   report.Days,
	}
}

func (d *demoUsageController) State() *usagePageState            { return d.state }
func (d *demoUsageController) Inventory() []usageInventorySource { return d.inventory }
func (d *demoUsageController) InventoryRevision() uint64         { return 1 }
func (d *demoUsageController) Resources() []scopedResourceKey    { return d.resources }
func (d *demoUsageController) HeatmapCache() *heatmapCache       { return d.heat }
func (d *demoUsageController) Refreshing() bool                  { return false }
func (d *demoUsageController) PendingErrorKey() string           { return d.errorKey }
func (d *demoUsageController) ClearErrorKey()                    { d.errorKey = "" }
func (d *demoUsageController) WriteClipboard(string) bool        { return false }

func (d *demoUsageController) SelectFilter(field, value string) {
	if d.state.selectFilter(field, value) {
		d.reload()
	}
}

func (d *demoUsageController) SelectBreakdown(mode string) {
	if d.state.selectBreakdown(mode) {
		d.reload()
	}
}

func (d *demoUsageController) TurnPage(page int) {
	if d.state.turnPage(page) {
		d.reload()
	}
}

func (d *demoUsageController) Refresh() {
	d.state.advance()
	d.state.resetPagination()
	d.reload()
}

func (d *demoUsageController) OnActivityWidth(width float32) {
	d.activityWidth = width
	if d.heat == nil || heatmapDays(width) > d.heat.span {
		d.loadHeatmap()
	}
}

func renderDemoUsagePage(c *ui.Context, app *shell.App) {
	ledger := app.DemoUsage()
	if ledger == nil {
		kit.Result(c, kit.ResultProps{
			Icon:  icons.ChartPie,
			Title: i18n.T(c, "usage_empty"),
		}).Grow(1).Justify(ui.Center)
		return
	}
	ctrl := demoControllerFor(c, ledger, time.Now)
	ctrl.renderer.render(c, ctrl)
}
