package remindersdesktop

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewapi"
)

// PageID is the stable navigation ID for the native Reminders page.
const PageID shell.Page = "page:reminders"

// Register installs the native Reminders page and its embedded locales.
func Register(assembly *shell.Assembly) {
	assembly.WorkspacePages = append(assembly.WorkspacePages, Page())
	assembly.Locales = append(assembly.Locales, Locales)
}

// Page constructs the shell.NativePage definition for Reminders.
func Page() shell.NativePage {
	return shell.NativePage{
		Page: PageID,
		Feature: shell.Feature{
			ID:            string(PageID),
			Icon:          icons.BellNav,
			Label:         "reminders_page_title",
			DefaultPinned: true,
			LegacyPackage: "reminders",
		},
		ReplacesLegacyViews: true,
		BindWindow: func(window shell.WindowContext) shell.WindowBinding {
			services, ok := viewapi.WindowServices(window.Context)
			if !ok {
				return shell.WindowBinding{}
			}
			ctrl := newController(window.Context, services, window.Visible)
			return shell.WindowBinding{
				Render: ctrl.render,
				Slots:  ctrl.slots,
				Sync:   ctrl.sync,
				Close:  ctrl.close,
			}
		},
		Render: func(c *ui.Context, _ *shell.App) {
			kit.Result(c, kit.ResultProps{
				Icon:     icons.BellNav,
				Title:    i18n.T(c, "reminders_page_title"),
				SubTitle: i18n.T(c, "reminders_description"),
			}).Grow(1).Justify(ui.Center)
		},
	}
}
