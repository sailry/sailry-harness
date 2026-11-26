package statisticsdesktop

import (
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewapi"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// UsagePage is the native workspace route ID for the Usage page.
const UsagePage shell.Page = "page:usage"

// Register installs the native statistics desktop module (Usage workspace page
// and the five composer metrics) into assembly.
func Register(assembly *shell.Assembly) {
	assembly.Locales = append(assembly.Locales, Locales)
	assembly.WorkspacePages = append(assembly.WorkspacePages, Page())
	assembly.PluginControlReplacements = append(assembly.PluginControlReplacements,
		shell.PluginControlReplacement{Package: packageName, Slot: protocol.PluginUiSlotStatistics, ID: composerTokensID},
		shell.PluginControlReplacement{Package: packageName, Slot: protocol.PluginUiSlotStatistics, ID: composerSpeedID},
		shell.PluginControlReplacement{Package: packageName, Slot: protocol.PluginUiSlotStatistics, ID: composerCostID},
		shell.PluginControlReplacement{Package: packageName, Slot: protocol.PluginUiSlotStatistics, ID: composerCacheID},
		shell.PluginControlReplacement{Package: packageName, Slot: protocol.PluginUiSlotStatistics, ID: composerTurnsID},
	)
	assembly.MetricContributions = append(assembly.MetricContributions, shell.MetricContribution{
		Bind: bindComposerMetrics,
	})
}

// Page returns the native Usage workspace page declaration.
func Page() shell.NativePage {
	return shell.NativePage{
		Page: UsagePage,
		Feature: shell.Feature{
			ID:            string(UsagePage),
			Icon:          icons.ChartBarNav,
			Label:         "usage",
			DefaultPinned: true,
			LegacyPackage: packageName,
		},
		ReplacesLegacyViews: true,
		SupportsDemo:        true,
		BindWindow:          bindUsagePage,
		Render:              renderDemoUsagePage,
		Workspace: func(*shell.App) shell.WorkspaceSlots {
			return usageWorkspaceSlots(nil, nil)
		},
	}
}

func bindUsagePage(window shell.WindowContext) shell.WindowBinding {
	services, ok := viewapi.WindowServices(window.Context)
	if !ok {
		return shell.WindowBinding{}
	}
	ctrl := newLiveUsageController(window, services, time.Now)
	return shell.WindowBinding{
		Render: ctrl.render,
		Slots: func() shell.WorkspaceSlots {
			return usageWorkspaceSlots(ctrl.Refreshing, ctrl.Refresh)
		},
		Sync:  ctrl.sync,
		Close: ctrl.Close,
	}
}

func bindComposerMetrics(_ shell.WindowContext) shell.MetricBinding {
	return shell.MetricBinding{
		Statistics: func(c *ui.Context, scope shell.ComposerMetricScope) []conversation.Metric {
			return buildComposerMetrics(scope.HasMessages, scope.Statistics, func(key string, args ...any) string {
				return i18n.T(c, key, args...)
			})
		},
	}
}
