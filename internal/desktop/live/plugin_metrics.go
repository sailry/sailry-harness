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
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// projectPluginMetrics makes the context bar's figures the packages' and
// native metric contributions': lead figures first (provider account status),
// then statistics figures grouped by contributor.
func (r *Runtime) projectPluginMetrics(comp *composer.State, registry *plugins.Registry) {
	r.projectComposerMetrics(comp, registry, shell.ComposerMetricScope{})
}

func (r *Runtime) projectComposerMetrics(comp *composer.State, registry *plugins.Registry, scope shell.ComposerMetricScope) {
	root := r.localRuntime()
	comp.Metrics = func(view *ui.Context, compact bool, open *bool) {
		var lead []conversation.Metric
		if !scope.Demo && root.App != nil {
			lead = append(lead, root.App.MetricLead(view, scope)...)
		}
		if registry != nil {
			for _, group := range r.pluginMetrics(view, registry, protocol.PluginUiSlotFooter) {
				lead = append(lead, group...)
			}
		}
		var groups [][]conversation.Metric
		if root.App != nil {
			groups = append(groups, root.App.MetricStatistics(view, scope)...)
		}
		if registry != nil {
			groups = append(groups, r.pluginMetrics(view, registry, protocol.PluginUiSlotStatistics)...)
		}
		conversation.UsageBar(view, lead, groups, compact, open)
	}
}

// pluginMetrics are a slot's published figures, one group a package, in the
// registry's order (the declared order, then the package's name). A footer
// figure shows only while its package's view runs and enables it. Controls
// replaced by a native module are omitted so legacy plugins never duplicate
// native metrics.
func (r *Runtime) pluginMetrics(view *ui.Context, registry *plugins.Registry, slot protocol.PluginUiSlot) [][]conversation.Metric {
	if registry == nil {
		return nil
	}
	var groups [][]conversation.Metric
	index := map[string]int{}
	for _, entry := range registry.Entries(slot) {
		if entry.Retry || slot == protocol.PluginUiSlotFooter && !entry.State.Enabled ||
			r.legacyPluginControlReplaced(entry.Key.Package, entry.Declaration) {
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
		// Namespace plugin metric IDs distinctly from native module IDs so a
		// third-party or fixture package sharing a package name and control ID
		// never collides with native metrics in conversation.UsageBar.
		ID:    "plugin:" + entry.Key.Package.Name + ":" + entry.Key.ID,
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
