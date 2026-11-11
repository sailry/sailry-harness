package live

import (
	"encoding/json"
	"math"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// projectPluginMetrics makes the context bar's figures the packages': the
// footer slot's first, a provider's account status, then the statistics
// slot's, one group a package (conversation/live/context.rs context_bar,
// conversation/live/contributions.rs registered_footer and
// registered_statistics). The core draws them and their hover cards from
// what each package published: its label, value and detail rows.
func (r *Runtime) projectPluginMetrics(comp *composer.State, registry *plugins.Registry) {
	comp.Metrics = func(view *ui.Context, compact bool, open *bool) {
		var lead []conversation.Metric
		for _, group := range r.pluginMetrics(view, registry, protocol.PluginUiSlotFooter) {
			lead = append(lead, group...)
		}
		conversation.UsageBar(view, lead, r.pluginMetrics(view, registry, protocol.PluginUiSlotStatistics), compact, open)
	}
}

// pluginMetrics are a slot's published figures, one group a package, in the
// registry's order (the declared order, then the package's name). A footer
// figure shows only while its package's view runs and enables it.
func (r *Runtime) pluginMetrics(view *ui.Context, registry *plugins.Registry, slot protocol.PluginUiSlot) [][]conversation.Metric {
	var groups [][]conversation.Metric
	index := map[string]int{}
	for _, entry := range registry.Entries(slot) {
		if entry.Retry || slot == protocol.PluginUiSlotFooter && !entry.State.Enabled {
			continue
		}
		at, grouped := index[entry.Key.Package.Name]
		if !grouped {
			at = len(groups)
			index[entry.Key.Package.Name] = at
			groups = append(groups, nil)
		}
		groups[at] = append(groups[at], r.pluginMetric(view, entry))
	}
	return groups
}

// pluginMetric is one published figure: the declared label and icon, the
// published value and its details, those with a finite progress as meters
// (conversation/usage/external.rs). A statistics figure with a handler shows
// its package's own view in the card.
func (r *Runtime) pluginMetric(view *ui.Context, entry plugins.ContributionEntry) conversation.Metric {
	locale := view.Locale()
	metric := conversation.Metric{
		ID:    entry.Key.Package.Name + ":" + entry.Key.ID,
		Order: int(entry.Declaration.Order),
		Label: pluginLabel(entry.Declaration.Label, locale),
		Value: i18n.T(view, "composer_metric_unknown"),
	}
	if entry.State.Label != nil {
		metric.Label = pluginLabel(*entry.State.Label, locale)
	}
	var value string
	if json.Unmarshal(entry.State.Value, &value) == nil && string(entry.State.Value) != "null" {
		metric.Value = value
	}
	name := entry.Declaration.Icon
	if entry.State.Icon != nil {
		name = entry.State.Icon
	}
	if name != nil {
		if icon, err := icons.Plugin(*name); err == nil {
			metric.Icon = icon
		}
	}
	if entry.Declaration.Width != nil {
		metric.Width = float32(*entry.Declaration.Width)
	}
	for _, detail := range entry.State.Details {
		row := conversation.Row{Label: pluginLabel(detail.Label, locale), Value: detail.Value}
		if detail.Progress != nil {
			if progress, err := detail.Progress.Float64(); err == nil && !math.IsNaN(progress) && !math.IsInf(progress, 0) {
				row.Meter, row.Progress = true, min(max(progress, 0), 100)
			}
		}
		metric.Rows = append(metric.Rows, row)
	}
	if entry.Declaration.Slot == protocol.PluginUiSlotStatistics && entry.Declaration.Kind == "metric" && entry.Declaration.Handler != nil {
		metric.Panel = func(view *ui.Context) { r.renderPluginContribution(view, entry.Key) }
	}
	return metric
}
