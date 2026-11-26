package shell

import (
	"context"
	"fmt"
	"io/fs"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Assembly selects the optional pages, services, and translations for each window.
type Assembly struct {
	WorkspacePages            []NativePage
	SettingsPages             []settings.NativePage
	ExecutionSettings         []settings.ExecutionSettingFactory
	Permissions               []PermissionContribution
	ProjectWorkflows          []ProjectWorkflow
	PluginControlReplacements []PluginControlReplacement
	MetricContributions       []MetricContribution
	ProviderUI                func(*settings.State) ProviderUI
	Locales                   []fs.FS
	panelResources            []panelRegistration
	linkHandlers              linkHandlers
	appActions                appActionRegistry
}

// ComposerMetricScope describes the active conversation and selected model
// channel when rendering composer context-bar metrics.
type ComposerMetricScope struct {
	Node        string
	Provider    string
	ProviderRev uint64
	Model       string
	Auth        protocol.Authentication
	Credential  *protocol.CredentialRef
	CredRev     uint64
	Connected   bool
	Demo        bool
	HasMessages bool
	Statistics  *protocol.ConversationStatistics
}

// MetricBinding belongs to one metric contributor in one window. Lead and
// Statistics produce composer context-bar metrics; Provider supplies Settings
// channel usage. Sync runs before layout; Close releases owned resources when
// the window closes.
type MetricBinding struct {
	Lead       func(*ui.Context, ComposerMetricScope) []conversation.Metric
	Statistics func(*ui.Context, ComposerMetricScope) []conversation.Metric
	Provider   func(*ui.Context, string, string) *settings.ChannelUsage
	Sync       func()
	Close      func()
}

// MetricContribution registers a window-scoped metric contributor in Assembly.
type MetricContribution struct {
	Bind func(WindowContext) MetricBinding
}

// PluginControlReplacement identifies an old official control provided by a
// native module in this assembly. It never reserves an ID in another package.
type PluginControlReplacement struct {
	Package string
	Slot    protocol.PluginUiSlot
	ID      string
}

// Module registers the contributions it supplies to a desktop assembly.
type Module func(*Assembly)

func Assemble(modules ...Module) Assembly {
	var assembly Assembly
	for _, register := range modules {
		register(&assembly)
	}
	return assembly
}

// NativePage contributes a window-local rail destination and its content.
type NativePage struct {
	Page    Page
	Feature Feature
	Render  func(*ui.Context, *App)
	Usage   protocol.FeatureUse
	// ReplacesLegacyViews also replaces the official LegacyPackage's non-navigation
	// workspace views and contributions. Package settings and history remain available.
	ReplacesLegacyViews bool
	// LegacyWorkspace is the replaced package's layout ID for a project resource page.
	LegacyWorkspace string
	// Bind returns a window-local frame callback. visible reports whether this
	// route is active in the live window, including between frames.
	Bind func(app *App, visible func() bool) func()
	// BindWindow supplies the live window's instance of this page. Render and
	// Workspace are the fallbacks when there is no live binding.
	BindWindow func(WindowContext) WindowBinding
	// SupportsDemo reports whether this page's WindowBinding handles Demo mode
	// using sample data without contacting a Node.
	SupportsDemo bool
	// Workspace supplies the slots around Render before layout, for pages
	// without a project Resource.
	Workspace func(*App) WorkspaceSlots
	// Resource shares a workspace panel tab's project and the shell's layout.
	Resource panel.Kind
	// CheckpointHistory requests older checkpoints for the shown workspace.
	CheckpointHistory bool
	BindWorkspace     panel.GitWorkspaceMount
	// RenderDetails and BindKeys are optional resource callbacks. The shell
	// calls them with the current workspace project when it is available.
	RenderDetails func(*ui.Context, *App, *panel.Project)
	BindKeys      func(*ui.Context, *panel.Project, ui.Element)
	// ExtensionDetails shares Settings' extension details in the right column.
	ExtensionDetails bool
}

// WindowContext is passed only to pages registered in this live window.
type WindowContext struct {
	Context context.Context
	Visible func() bool
}

// WindowBinding belongs to one registered page in one window. Sync runs
// before layout; Close releases whatever the page still owns at window close.
type WindowBinding struct {
	Render func(*ui.Context)
	Slots  func() WorkspaceSlots
	Sync   func()
	Close  func()
}

// WorkspaceSlots are a native page's current frame: the Shell places the
// navigation, details and header slots and owns their sizing and folding.
type WorkspaceSlots struct {
	PreferenceKey       string
	MinNavigationWidth  float32
	MinDetailsWidth     float32
	DefaultDetailsWidth float32
	DetailsLabel        string // i18n key or a localized label
	Navigation          func(*ui.Context)
	Details             func(*ui.Context)
	DetailsHeader       func(*ui.Context)
	Title               func(*ui.Context)
	Header              func(*ui.Context)
	// Close handles the scoped Close Tab action, including when it is a no-op.
	Close func(*ui.Context)
}

func (a *App) nativeWorkspacePageDefinition() *NativePage {
	page := a.nativePage(a.Page)
	if page == nil || page.Resource != "" {
		return nil
	}
	if page.Workspace == nil && a.activeWindowBinding().Slots == nil {
		return nil
	}
	return page
}

func (a *App) syncNativeWorkspace() {
	page := a.nativeWorkspacePageDefinition()
	if page == nil {
		a.nativeWorkspacePage, a.nativeWorkspace = "", WorkspaceSlots{}
		return
	}
	a.nativeWorkspacePage = page.Page
	var slots WorkspaceSlots
	if bound := a.activeWindowBinding(); bound.Slots != nil {
		slots = bound.Slots()
	} else {
		slots = page.Workspace(a)
	}
	a.nativeWorkspace = slots
	key := slots.PreferenceKey
	if key == "" {
		key = page.Feature.ID
	}
	width := slots.DefaultDetailsWidth
	if width == 0 {
		width = 400
	}
	a.SetPluginWorkspace(key, slots.MinNavigationWidth, slots.MinDetailsWidth, width)
}

func (a *App) installPages(assembly Assembly) {
	a.installAppActions(assembly)
	a.installLinkHandlers(assembly)
	a.metricContributions = slices.Clone(assembly.MetricContributions)
	a.translations = i18n.New(assembly.Locales...)
	a.Settings.SetNativePages(assembly.SettingsPages)
	a.Settings.ExecutionSettings = settings.BindExecutionSettings(assembly.ExecutionSettings, settings.ExecutionSettingHost{Preview: a.Preview})
	if assembly.ProviderUI != nil && a.Settings.HasSection(settings.Providers) {
		a.ProviderUI = assembly.ProviderUI(a.Settings)
	}
	a.nativePages = []NativePage{{Page: ConversationPage, Feature: Feature{
		ID: string(ConversationPage), Icon: icons.Chat, Label: "shell_sessions", Fixed: true, DefaultPinned: true,
	}}}
	if a.Preview {
		activity := Feature{ID: string(ActivityPage), Icon: icons.Grid, Label: "activity", DefaultPinned: true}
		a.nativePages = append(a.nativePages, NativePage{Page: ActivityPage, Feature: activity, Render: func(c *ui.Context, _ *App) {
			kit.Result(c, kit.ResultProps{Icon: activity.Icon, Title: i18n.T(c, activity.Label), SubTitle: i18n.T(c, "unavailable")}).Grow(1).Justify(ui.Center)
		}})
	}
	for _, p := range assembly.WorkspacePages {
		if p.Page == "" || p.Feature.ID == "" || p.Render == nil && p.BindWindow == nil {
			panic(fmt.Sprintf("shell: native page %q requires a page ID, rail ID, and renderer", p.Page))
		}
		if a.hasPage(p.Page) {
			panic(fmt.Sprintf("shell: duplicate native page ID %q", p.Page))
		}
		if a.pageForFeature(p.Feature.ID) != nil {
			panic(fmt.Sprintf("shell: duplicate native page rail ID %q", p.Feature.ID))
		}
		a.nativePages = append(a.nativePages, p)
		if !a.Preview && p.Bind != nil {
			if sync := p.Bind(a, func() bool { return a.Page == p.Page && !a.DemoOn() }); sync != nil {
				a.nativePageSync = append(a.nativePageSync, sync)
			}
		}
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

func (a *App) ReplacesLegacyViews(packageName string) bool {
	for _, page := range a.nativePages {
		if packageName != "" && page.Feature.LegacyPackage == packageName {
			return page.ReplacesLegacyViews
		}
	}
	return false
}

func (a *App) activeWindowBinding() WindowBinding {
	page := a.nativePage(a.Page)
	if a.DemoOn() && (page == nil || !page.SupportsDemo) {
		return WindowBinding{}
	}
	return a.nativeWindowBindings[a.Page]
}

// BindWindow supplies the live context after the window's services are ready.
// Fixture windows never acquire live resources.
func (a *App) BindWindow(ctx context.Context) {
	if a.Preview || a.windowBound {
		return
	}
	a.windowBound = true
	for _, page := range a.nativePages {
		if page.BindWindow == nil {
			continue
		}
		if a.nativeWindowBindings == nil {
			a.nativeWindowBindings = make(map[Page]WindowBinding)
		}
		supportsDemo := page.SupportsDemo
		targetPage := page.Page
		a.nativeWindowBindings[targetPage] = page.BindWindow(WindowContext{
			Context: ctx,
			Visible: func() bool {
				return ctx.Err() == nil && a.windowBound && a.Page == targetPage && (supportsDemo || !a.DemoOn())
			},
		})
	}
	for _, contribution := range a.metricContributions {
		if contribution.Bind == nil {
			continue
		}
		a.metricBindings = append(a.metricBindings, contribution.Bind(WindowContext{
			Context: ctx,
			Visible: func() bool {
				return ctx.Err() == nil && a.windowBound && !a.DemoOn()
			},
		}))
	}
}

// CloseWindowBindings releases the per-window callbacks registered by BindWindow.
func (a *App) CloseWindowBindings() {
	a.windowBound = false
	a.closeAppActions()
	a.closeWorkspaceOverlay()
	bindings := a.nativeWindowBindings
	a.nativeWindowBindings = nil
	metrics := a.metricBindings
	a.metricBindings = nil
	a.linkHandlers = nil
	a.nativeWorkspacePage, a.nativeWorkspace = "", WorkspaceSlots{}
	for _, page := range a.nativePages {
		if close := bindings[page.Page].Close; close != nil {
			close()
		}
	}
	for _, binding := range metrics {
		if binding.Close != nil {
			binding.Close()
		}
	}
}

// SyncNativePages delivers shared live state to the installed page controllers
// and metric contributors.
func (a *App) SyncNativePages() {
	for _, sync := range a.nativePageSync {
		sync()
	}
	if len(a.nativeWindowBindings) != 0 {
		for _, page := range a.nativePages {
			if sync := a.nativeWindowBindings[page.Page].Sync; sync != nil {
				sync()
			}
		}
	}
	for _, binding := range a.metricBindings {
		if binding.Sync != nil {
			binding.Sync()
		}
	}
}

// MetricLead returns the context-bar lead metrics contributed by bound native
// modules for the active composer scope.
func (a *App) MetricLead(c *ui.Context, scope ComposerMetricScope) []conversation.Metric {
	var metrics []conversation.Metric
	for _, binding := range a.metricBindings {
		if binding.Lead != nil {
			metrics = append(metrics, binding.Lead(c, scope)...)
		}
	}
	return metrics
}

// MetricStatistics returns the context-bar statistics metric groups contributed
// by bound native modules for the active composer scope, one group per contributor.
func (a *App) MetricStatistics(c *ui.Context, scope ComposerMetricScope) [][]conversation.Metric {
	var groups [][]conversation.Metric
	for _, binding := range a.metricBindings {
		if binding.Statistics == nil {
			continue
		}
		if group := binding.Statistics(c, scope); len(group) > 0 {
			groups = append(groups, group)
		}
	}
	return groups
}

// ProviderUsage returns the first native channel usage summary contributed for
// the given Node and provider ID, or nil when none applies.
func (a *App) ProviderUsage(c *ui.Context, node, channel string) *settings.ChannelUsage {
	for _, binding := range a.metricBindings {
		if binding.Provider == nil {
			continue
		}
		if usage := binding.Provider(c, node, channel); usage != nil {
			return usage
		}
	}
	return nil
}

// NativePageUsage returns the counter declared by the current native page.
func (a *App) NativePageUsage() protocol.FeatureUse {
	if page := a.nativePage(a.Page); page != nil {
		return page.Usage
	}
	return ""
}

// ResourceKind identifies the workspace view used by the current page.
func (a *App) ResourceKind() panel.Kind { return a.paged() }

// ResourceCheckpointHistory reports whether this view needs checkpoint history.
func (a *App) ResourceCheckpointHistory() bool {
	page := a.nativePage(a.Page)
	return page != nil && page.Resource != "" && page.CheckpointHistory
}

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
