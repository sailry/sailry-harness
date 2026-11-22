package shell

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
)

// NativePage contributes a window-local rail destination and its content.
type NativePage struct {
	Page    Page
	Feature Feature
	Render  func(*ui.Context, *App)
	// Resource shares a workspace panel tab's project and the shell's layout.
	Resource panel.Kind
	// RenderDetails and BindKeys are optional resource callbacks. The shell
	// calls them with the current workspace project when it is available.
	RenderDetails func(*ui.Context, *App, *panel.Project)
	BindKeys      func(*ui.Context, *panel.Project, ui.Element)
	// ExtensionDetails shares Settings' extension details in the right column.
	ExtensionDetails bool
}

func (a *App) installPages(pages []NativePage) {
	a.nativePages = []NativePage{{Page: ConversationPage, Feature: Feature{
		ID: string(ConversationPage), Icon: icons.Chat, Label: "shell_sessions", Fixed: true, DefaultPinned: true,
	}}}
	if a.Preview {
		activity := Feature{ID: string(ActivityPage), Icon: icons.Grid, Label: "activity", DefaultPinned: true}
		a.nativePages = append(a.nativePages, NativePage{Page: ActivityPage, Feature: activity, Render: func(c *ui.Context, _ *App) {
			kit.Result(c, kit.ResultProps{Icon: activity.Icon, Title: i18n.T(c, activity.Label), SubTitle: i18n.T(c, "unavailable")}).Grow(1).Justify(ui.Center)
		}})
	}
	for _, p := range pages {
		if p.Page == "" || p.Feature.ID == "" || p.Render == nil {
			panic(fmt.Sprintf("shell: native page %q requires a page ID, rail ID, and renderer", p.Page))
		}
		if a.hasPage(p.Page) {
			panic(fmt.Sprintf("shell: duplicate native page ID %q", p.Page))
		}
		if a.pageForFeature(p.Feature.ID) != nil {
			panic(fmt.Sprintf("shell: duplicate native page rail ID %q", p.Feature.ID))
		}
		a.nativePages = append(a.nativePages, p)
	}
	a.features = a.mergeFeatureOrder(nil)
	for _, p := range a.nativePages {
		a.unpinned[p.Feature.ID] = !p.Feature.DefaultPinned && !p.Feature.Fixed
	}
}

func (a *App) nativePage(page Page) *NativePage {
	for i := range a.nativePages {
		if a.nativePages[i].Page == page {
			return &a.nativePages[i]
		}
	}
	return nil
}

func (a *App) pageForFeature(id string) *NativePage {
	for i := range a.nativePages {
		if a.nativePages[i].Feature.ID == id {
			return &a.nativePages[i]
		}
	}
	return nil
}

// NativePageEnabled reports whether the assembly contributed this page.
func (a *App) NativePageEnabled(p Page) bool { return a.nativePage(p) != nil }

// ResourceKind identifies the workspace view used by the current page.
func (a *App) ResourceKind() panel.Kind { return a.paged() }

func (a *App) hasPage(p Page) bool {
	switch p {
	case ConversationPage, SettingsPage, HostPage, NotificationsPage, ProjectPage, PluginPage:
		return true
	default:
		return a.nativePage(p) != nil
	}
}

// RevealResourceFile returns the navigation callback for a file already opened
// in the shared workspace. Without a Files resource, the action is unavailable.
func (a *App) RevealResourceFile() func(string) {
	for _, p := range a.nativePages {
		if p.Resource == panel.Files {
			return func(string) { a.Navigate(p.Page) }
		}
	}
	return nil
}
