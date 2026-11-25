// Package shell is the main window's frame: the header band, the feature
// rail, the sidebar column and the right panel beside the page, each column
// opening, closing and resizing with the reference's motion, and the window's
// shortcuts, search, activity centre and knowledge viewer.
package shell

import (
	"net/url"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/onboarding"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/transfers"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// Layout metrics.
const (
	HeaderHeight = 48
	RailWidth    = 56
	NavWidth     = 240
	NavMin       = 200
	NavMax       = 320
	MainMin      = 320 // the conversation's least width
	PanelWidth   = 560
	PanelMin     = 360
	// trafficLights is the room macOS's window controls need at (12, 16).
	trafficLights = 80
	// handle is the width of a column's resize handle, over its edge.
	handle = 6
)

// The header's and the rail's icon buttons are Kit's IconButton at one size
// each, 8 DIPs apart. Header contributions (Hooks.PluginHeader) use
// HeaderButton too.
const (
	HeaderButton = 28
	RailButton   = 36
	ButtonGap    = 8
)

// Motion is the columns' open and close: Kit's duration_normal along
// easing_move. MyGo makes it instant with Reduce Motion.
var Motion = ui.Animation{Duration: 180 * time.Millisecond, Ease: ui.CubicBezier(0.2, 0, 0, 1)}

// Page is the stable ID of a destination in the main column. Native pages
// can use their own namespaced IDs.
type Page string

const (
	ConversationPage  Page = "page:conversation"
	SettingsPage      Page = "page:settings"
	FilesPage         Page = "page:files"
	GitPage           Page = "page:git"
	ActivityPage      Page = "page:activity"
	MarketPage        Page = "page:settings_plugins"
	HostPage          Page = "page:host"
	NotificationsPage Page = "page:notifications"
	ProjectPage       Page = "page:project"
	PluginPage        Page = "page:plugin"
)

// App is the window's state.
type App struct {
	Data      *viewmodel.Data
	Preview   bool
	Hooks     Hooks
	noticeTab int
	// noticeDetail is the ID of the notice whose details are open: the
	// notices are projected anew when one is read, so it is not a pointer.
	noticeDetail string
	// frosted is whether this frame's theme has window art, which the
	// columns blur (kit.Pane).
	frosted bool
	// Theme is the selected theme package's ID; empty is the built-in one.
	Theme   string
	Mode    theme.Mode
	Effects bool // the theme's animated effects
	// Surfaces are the window materials' opacities (Appearance).
	Surfaces theme.Surfaces
	Page     Page
	// NavOpen and NavWidth are the side column's saved choice, shared by the
	// pages that have one (HasNavigation); a page without one hides the
	// column and leaves the choice as it is.
	NavOpen  bool
	NavWidth float32
	// OverviewHidden is the header button's saved choice. A narrow pane
	// uses a popover without changing the wide pane's choice.
	OverviewHidden bool
	// PluginNavigation is the resource navigation supplied by the mounted
	// package page. The Shell owns its position, width and fold toggle.
	PluginNavigation func(*ui.Context)
	// navigated is whether the last frame's page had a side column.
	navigated bool
	// PanelOpen is whether the user wants the right panel; a narrow window
	// hides it without changing this.
	PanelOpen bool
	// PluginDetails draws the details a package page's Workspace supplies
	// to the right column, nil while the page shown offers none. They show
	// until the toggle folds them; the conversation's PanelOpen is its own.
	PluginDetails func(*ui.Context)
	// PluginDetailsHeader draws what those details give the column's
	// header segment: their title, then their own actions.
	PluginDetailsHeader func(*ui.Context)
	PluginDetailsLabel  string
	PanelWidth          float32
	// resourceColumn is the last frame's full-page resource owning details.
	// Changing owners settles the destination; folding within it animates.
	resourceColumn Page
	resourceTarget ResourceTarget

	pluginWorkspace      string
	pluginWorkspaces     map[string]*PluginWorkspacePrefs
	pluginNavigationMin  float32
	pluginDetailsMin     float32
	pluginDetailsDefault float32
	nativeWorkspacePage  Page
	nativeWorkspace      WorkspaceSlots
	workspaceHeaderPane  ui.Handle
	// DiffSplit is the window's diff layout, side by side when set: the
	// button at the end of a diff's footer writes it (kit.DiffSplit), and
	// it is saved with the window's other choices (prefs.go).
	DiffSplit bool
	// ListShare is the share of the height a list takes above its document
	// where a conversation panel is too narrow for both side by side
	// (panel.BindListShare); 0 is the first share. The divider between them
	// writes it, and it is saved as DiffSplit is.
	ListShare float32
	// viewZoom is the zoom commands' own state (view_zoom.go); the
	// interface size itself is a setting.
	viewZoom viewZoom

	// OnMode, when set, gets the theme mode as it changes, so the window's
	// native material follows the app's appearance.
	OnMode  func(theme.Mode)
	applied theme.Mode

	OnFeatureMenuChanged func()

	Sidebar  *sidebar.State
	Settings *settings.State
	// ProviderUI is installed with the optional model channels page.
	ProviderUI ProviderUI
	Website    string
	Feedback   string
	Messages   *conversation.State
	// overviews are the session overview popovers by conversation, each
	// opened by its conversation's own button (conversationActions); band
	// is the header's main region as the activity strip fits into it
	// (header.go); keyed is a conversation key of this frame
	// (conversation.stop, composer.focus, composer.models) for its page.
	overviews map[string]*overviewPopover
	// mainLayoutWidth is the destination width of the conversation during
	// sidebar and detail-panel transitions; moving never rewraps its rows.
	mainLayoutWidth float32
	mainLayoutShift float32
	band            band
	keyed           string
	Panel           *panel.State
	// Projects keeps each project's Files and Git state, shared by their
	// pages and the panel's tabs.
	Projects *panel.Projects
	drafts   map[string]*composer.State
	// transfers and voice play the composers' file transfers and voice
	// input on the frame clock (preview).
	transfers preview.Transfers
	voice     preview.Voice
	// clock is the frame's time, which the sample actions start their
	// playing from: a key's handler runs between frames, where the context
	// has no time to give.
	clock time.Time

	nativePages          []NativePage
	nativePageSync       []func()
	nativeWindowBindings map[Page]WindowBinding
	linkHandlers         linkHandlers
	appActions           *appActionWindow
	workspaceOverlay     workspaceOverlay
	translations         *i18n.Catalog

	features       []string // saved rail order, including unavailable features
	pluginFeatures map[string]Feature
	legacyFeatures map[string]string // native feature ID to verified profile-local predecessor
	pluginPage     string
	unpinned       map[string]bool // features off the rail
	// previewRail is the preview's rail in the shared UI state file, which a
	// live window writes back as it found it (Prefs).
	previewRail struct{ features, unpinned []string }
	// previewOrder is the preview's session order in that file, kept likewise.
	previewOrder string
	more         bool // the rail's More popover
	search       search
	activity     bool // the activity centre's popover
	reading      *reading
	shown        string // the conversation selected when a project page opened
	before       Page   // the page Settings opened over, which its rail button goes back to
	overview     *projectOverview
	// The panes that take the focus: the main column, where the
	// conversation or a page shows, and the right panel (pane).
	mainPane, panelPane ui.Handle
	sent                []string // the native menu's commands (Do)
	modal               bool     // a modal dialog of Kit's showed in the last frame
	fileSaveEnabled     bool
	fileSaveMenu        func(bool)
	menuKeys            []menuKey // the native menu's items with a key (syncMenuKeys)
	// minimizeWindow minimizes the window the Window menu's Minimize was
	// chosen in (WindowMinimize).
	minimizeWindow func()
	// children are the subagent sessions opened in the panel, by ID.
	children map[string][]conversation.Turn

	// Host is the device whose monitor HostPage shows;
	// hosts are the host pages' data, made from the dataset as first
	// shown, and their UI state. Metrics is whether the host page's right
	// column shows the metrics summary (layout.panel_open of Page::Host):
	// closed until the user opens it, as the reference's, since the page
	// itself is the monitor.
	Host    string
	hosts   map[string]*host.Host
	hostUI  map[string]*host.State
	samples map[string]*sample
	Metrics bool

	// installs is the installation queue the rail's button shows;
	// download is the Save as dialog's transfer, played from downloaded
	// on the frame clock (preview).
	installs     *transfers.Queue
	installsOpen bool
	download     transfers.Download
	downloaded   time.Time

	// Welcome is the welcome guide, opened at the first launch
	// (SetPrefs) and from the Help menu in the preview.
	Welcome onboarding.Guide
	// selected is the selected text dialog (system shortcuts).
	selected selection
	// closing is the close and quit prompt (closing.go).
	closing *closing
	// panes is the split workspace (panes.go).
	panes workspace
	// panels are the conversations' right panels (panels.go).
	panels conversationPanels
	// demo is the demo conversations while they show (demo.go).
	demo *demoMode
}

// New makes a standalone sample window with the supplied workspace pages.
func New(d *preview.Data, pages ...NativePage) *App {
	a := newPreview(d, Assembly{WorkspacePages: pages})
	a.Panel = panel.NewSample(d.Workspace)
	a.Panel.ShortcutBinding = a.panelShortcutBinding
	a.Sidebar.Sample.NewTerminal = a.sampleTerminalAction(a.Panel)
	return a
}

// NewWithAssembly makes a preview window with the supplied page contributions.
func NewWithAssembly(d *preview.Data, assembly Assembly) *App {
	a := newPreview(d, assembly)
	a.installPanelResources(assembly)
	a.Sidebar.Sample.NewTerminal = a.sampleTerminalAction(a.Panel)
	return a
}

func newPreview(d *preview.Data, assembly Assembly) *App {
	a := newApp(d, preview.Big, settings.New())
	a.Preview = true
	a.installPages(assembly)
	a.Sidebar.Preview = true
	a.Sidebar.Sample.OpenHost = a.OpenHost
	// Sample connection rows and monitors refer to the same sample devices.
	devices := a.Settings.Devices
	a.Settings.Devices = nil
	index := 0
	for _, device := range devices {
		if !device.Execution {
			a.Settings.Devices = append(a.Settings.Devices, device)
			continue
		}
		if index >= len(d.Hosts) {
			continue
		}
		h := d.Hosts[index]
		device.ID, device.Local = h.ID, h.Local
		a.Settings.Devices = append(a.Settings.Devices, device)
		index++
	}
	a.installs = preview.Installations()
	a.Messages.OpenLink = a.sampleLink
	return a
}

// NewLive creates an unseeded live window with the supplied workspace pages.
func NewLive(d *viewmodel.Data, pages ...NativePage) *App {
	return NewLiveWithAssembly(d, Assembly{WorkspacePages: pages})
}

// NewLiveWithAssembly creates an unseeded live window with the supplied contributions.
func NewLiveWithAssembly(d *viewmodel.Data, assembly Assembly) *App {
	a := newApp(d, "", settings.NewLive())
	a.installPanelResources(assembly)
	a.installPages(assembly)
	a.Panel.Live = true
	if len(d.Hosts) > 0 {
		a.Host = d.Hosts[0].ID
	}
	return a
}

func newApp(d *viewmodel.Data, selected string, st *settings.State) *App {
	a := &App{
		Data: d, Page: ConversationPage, before: ConversationPage,
		NavOpen: true, NavWidth: NavWidth, PanelWidth: PanelWidth, Effects: true, Surfaces: theme.DefaultSurfaces,
		Sidebar: sidebar.New(d, selected), Settings: st, Messages: conversation.New(),
		Panel: panel.New(d.Workspace), Projects: panel.NewProjects(d.Workspace),
		drafts: map[string]*composer.State{}, unpinned: map[string]bool{}, Host: "local",
		Welcome: onboarding.Guide{Seen: true},
	}
	st.Keys.SetPanelShortcuts(a.panelShortcuts)
	a.Panel.ShortcutBinding = a.panelShortcutBinding
	st.ConnectionActions = &settings.ConnectionActions{Inspect: func(device viewmodel.Device) { a.OpenHost(device.ID) }}
	// The sidebar's sort menu and the settings' session order are one choice.
	a.Sidebar.Sort = &st.Model.SessionOrder
	return a
}

// Composer returns the composer of a conversation, which keeps its draft.
func (a *App) Composer(id string) *composer.State {
	if s, ok := a.drafts[id]; ok {
		return s
	}
	model := ""
	if len(a.Data.Models) > 0 {
		model = a.Data.Models[0].ID
	}
	s := composer.New(model)
	for _, host := range a.Data.Hosts {
		if host.Local {
			s.Host = host.ID
			break
		}
	}
	if cv := a.Data.Find(id); cv != nil && cv.Node != "" {
		s.Host = cv.Node
	}
	if pr := a.Data.ProjectOf(id); pr != nil {
		s.Host, s.Project, s.Worktree = pr.Host, pr.ID, pr.Branch
		if cv := a.Data.Find(id); cv != nil && cv.Worktree != "" {
			s.Worktree = cv.Worktree
		}
	}
	a.drafts[id] = s
	return s
}

// View builds the window.
func (a *App) View(c *ui.Context) {
	i18n.Bind(c, a.translations)
	if !a.hasPage(a.Page) {
		a.Navigate(ConversationPage)
	}
	defer a.updateFileSaveMenu(c)
	kit.BindDiffSplit(c, &a.DiffSplit)
	panel.BindListShare(c, &a.ListShare)
	kit.SetPreferences(c, kit.Preferences{ToastSeconds: uint64(settings.ToastSeconds[min(max(a.Settings.Toast, 0), len(settings.ToastSeconds)-1)]), MessageTextSize: uint8(a.Settings.TextSize)})
	// The panel is the selected conversation's before anything opens a tab
	// in it (panels.go).
	a.clock = c.Now()
	previousMessages := a.Messages
	a.syncDemo(c)
	if previousMessages != a.Messages {
		previousMessages.FinishFrame()
	}
	a.MountPanel()
	if a.Hooks.BeforeFrame != nil {
		a.underneath(func() { a.Hooks.BeforeFrame(c) })
	}
	if a.Hooks.AfterFrame != nil {
		defer a.underneath(a.Hooks.AfterFrame)
	}
	defer a.Messages.FinishFrame()
	if a.Mode != a.applied && a.OnMode != nil {
		a.OnMode(a.Mode)
	}
	a.applied = a.Mode
	// The system's appearance, before the app's mode replaces it.
	system := c.Theme().Dark
	theme.Apply(c, a.Theme, a.Mode)
	kit.SetEffects(a.Effects)
	if a.panels.focusMain {
		a.panels.focusMain = false
		if c.FocusWithin(a.panelPane) {
			a.mainPane.Focus(c)
		}
	}
	// The live workspace was resolved by BeforeFrame for the mounted session.
	// A selection changed by a shortcut takes that binding on the next frame.
	boundConversation := a.panels.mounted
	a.shortcuts(c)
	a.applyViewZoom(c)
	a.MountPanel()
	a.followSelection()
	a.followPanes()
	a.SyncNativePages()
	a.syncNativeWorkspace()
	a.handOverFocus(c)
	a.panes.enlargedFrame = a.panes.enlarged
	if target := a.panes.enlargedFocus; target != (panes.Target{}) {
		a.panes.enlargedFocus = panes.Target{}
		if a.Page == ConversationPage && a.panes.Active == target {
			a.Composer(target.ID).Focus()
		}
	}
	if a.Page == ConversationPage && (a.sampled() || a.panels.mounted == boundConversation) {
		a.syncPanel()
	}
	a.sweepOverviews()
	if a.paged() != "" {
		a.SetPluginWorkspace(a.feature(), 0, 260, 320)
	}
	p := theme.Of(c)
	// The title bar and the rail are one outer band in title_bar's color.
	band, fill := p.TitleBar, p.Background
	if c.Vibrancy() && theme.IsDark(c, a.Mode) == system {
		// The window material shows through: the outer band at 45% of the title
		// bar, the panel at 83.5% of the background. The material takes the
		// system's appearance, which MyGo cannot override, so only while the app
		// shows that appearance too, followed or forced: a forced mode of the
		// other appearance paints opaque, so the band never shows a material of
		// the other appearance.
		band, fill = p.TitleBar.Alpha(a.Surfaces.Sidebar), p.Background.Alpha(a.Surfaces.Main)
	}
	c.Root().Background(band)
	// The window's art and effects lie under the band and the panel, which
	// turns translucent over them as over the window material.
	window := kit.Backdrop(c, "background.window")
	if window != nil {
		fill = p.Background.Alpha(a.Surfaces.Main)
	}
	a.frosted = window != nil
	root := kit.Draws(ui.Column(c.Key("frame")).Fill(), window)
	f := a.frame(c, root)
	a.mainLayoutWidth = f.mainWidth
	a.mainLayoutShift = f.mainShift
	root.Children(func() {
		a.header(c, f)
		ui.Row(c).Grow(1).MinHeight(0).AlignItems(ui.Stretch).Children(func() {
			a.rail(c, f.rail)
			// The panel's outline: a 1 DIP top and left edge, rounded at the top left.
			body := ui.Row(c.Key("panel")).Grow(1).MinWidth(0).AlignItems(ui.Stretch).BorderWidth(1, 0, 0, 1).BorderColor(p.SidebarBorder).
				Radius(p.RadiusLg, 0, 0, 0).Clip()
			if page := a.nativePage(a.Page); page != nil && page.Resource != "" && page.BindKeys != nil {
				if pr := a.project(); pr != nil {
					page.BindKeys(c, pr, body)
				}
			}
			body.Children(func() {
				a.nav(c, f, fill)
				main := kit.Draws(kit.Pane(c, ui.Column(c.Key("main")).Grow(1).MinWidth(0), fill, a.frosted, a.Surfaces.Blur*theme.PaneBlurMax), kit.Backdrop(c, "background.content"))
				// On the conversation standing alone ⌘W closes its tab, when
				// it has one; its pane keys work with or without.
				a.pane(c, main, &a.mainPane, a.closable() || a.tabClosable(), func() {
					if !a.closePageTab(c) && !a.ClosePage() {
						a.closeShownTab(c)
					}
				})
				if a.standsAlone() && a.Data.Find(a.panes.Active.ID) != nil {
					a.paneKeys(c, main, a.panes.Active)
				}
				a.mainColumn(c, main, f)
				a.details(c, f, fill)
				a.enlargedPane(c)
			})
		})
	})
	a.overlays(c)
	a.renderWorkspaceOverlay(c)
	if a.Hooks.PluginOverlays != nil {
		a.Hooks.PluginOverlays(c)
	}
	// Last, over every dialog: a toast shown while one is open stays reachable.
	kit.ToastHost(c)
}

// frame is where the columns are in this frame: how open each is, from 0
// to 1 along Motion, and their widths.
//
// The widths are in the content's DIPs. Column preferences and fit limits
// use window DIPs; zoom converts them after allowing the conversation
// sidebar some extra room.
type frame struct {
	nav, details         float32 // openness
	rail                 float32 // the rail's width
	navWidth, panelW     float32 // their full widths
	mainWidth, mainShift float32 // settled content width and its visual offset
	showsPanel, fitted   bool
	// zoom is the content's zoom and window the window's width in its own
	// DIPs, which the resize handles' limits are in.
	zoom, window float32
}

// HasNavigation reports whether the page shown has a side column beside the
// rail, as the reference's feature_navigation: the sessions on the
// conversation and on the host and project pages opened from it, the
// sections on Settings. Activity, Files, Git, Market, the notification
// center have none. A package page can supply its own resource navigation.
func (a *App) HasNavigation() bool {
	switch a.Page {
	case ConversationPage, HostPage, ProjectPage, SettingsPage:
		return true
	}
	return a.workspaceSlots().Navigation != nil
}

// NavShown reports whether the side column shows: the page has one and the
// user keeps it open.
func (a *App) NavShown() bool { return a.NavOpen && a.HasNavigation() }

// toggleNav opens or closes the side column (⌘B, the header button, the
// menu). A page without one keeps the saved choice, as the reference's
// toggle_sidebar returns when no navigation is available.
func (a *App) toggleNav() {
	if a.HasNavigation() {
		a.NavOpen = !a.NavOpen
	}
}

// frame animates the columns' openness on root, which stays from frame to
// frame.
func (a *App) frame(c *ui.Context, root ui.Element) frame {
	z := c.Zoom()
	w, _ := c.Size()
	w *= z
	f := frame{navWidth: a.clampNavigation(a.NavWidth), zoom: z, window: w}
	navTarget := float32(0)
	if a.NavShown() {
		navTarget = 1
	}
	if navigated := a.HasNavigation(); navigated && a.navigated {
		f.nav = root.AnimateTo("nav", navTarget, Motion)
	} else {
		// The column belongs to the page: going to or from a page without
		// one shows the destination settled, with no motion.
		f.nav = root.Jump("nav", navTarget)
		a.navigated = navigated
	}
	// Whether the panel fits is judged where the sidebar will end, so the
	// sidebar's motion does not toggle it.
	nav := f.navWidth * navTarget
	f.panelW = ClampPanel(a.PanelWidth, w, nav)
	if a.workspaceDetails() {
		f.panelW = a.clampPluginDetails(a.pluginWorkspacePrefs().Width, w, nav)
	}
	f.fitted = PanelFits(w, nav, f.panelW)
	f.showsPanel = (a.Page == ConversationPage && a.PanelOpen || a.Page == HostPage && a.Metrics || a.extensionDetails() ||
		a.workspaceDetails() && !a.pluginWorkspacePrefs().Folded) && f.fitted
	target := float32(0)
	if f.showsPanel {
		target = 1
	}
	rail := float32(RailWidth)
	if z >= 1.5 {
		// At 150% the 54-DIP buttons otherwise nearly fill the 56-DIP rail.
		rail += min(16, max(w-rail-z-nav-f.panelW*target-MainMin, 0))
	}
	// The same conversation sidebar shows beside the host and project pages.
	// At larger sizes it gets up to 56 extra window DIPs, but never at the
	// expense of a fitted details column or the main column's minimum space.
	switch a.Page {
	case ConversationPage, HostPage, ProjectPage:
		space := max(w-rail-z-f.navWidth-f.panelW*target-MainMin, 0)
		f.navWidth += min(112*(z-1), space)
		nav = f.navWidth * navTarget
	}
	var resource Page
	if a.resourceDetails() {
		resource = a.Page
	}
	if resource != a.resourceColumn {
		// A resource's list belongs to its page, including the first frame
		// its project becomes available. It inherits no other page's motion.
		f.details = root.Jump("details", target)
		a.resourceColumn = resource
	} else {
		f.details = root.AnimateTo("details", target, Motion)
	}
	// The panel's 1 DIP outline is the content's, z of the window's.
	f.mainWidth = max(w-rail-z-nav-f.panelW*target, 0)
	actual := max(w-rail-z-f.navWidth*f.nav-f.panelW*f.details, 0)
	f.mainShift = (actual - f.mainWidth) / 2
	f.rail, f.navWidth, f.panelW, f.mainWidth, f.mainShift = rail/z, f.navWidth/z, f.panelW/z, f.mainWidth/z, f.mainShift/z
	return f
}

// nav is the sidebar's column: its full width clipped to how open it is, so
// its content slides rather than reflows, and the resize handle on its edge
// while it is fully open.
func (a *App) nav(c *ui.Context, f frame, panel ui.Color) {
	if f.nav <= 0.001 {
		return
	}
	a.Sidebar.SelectedHost = ""
	if a.Page == HostPage {
		a.Sidebar.SelectedHost = a.Host
	}
	a.Sidebar.Preview = a.Preview || a.Demo()
	p := theme.Of(c)
	ui.Row(c.Key("nav")).Width(f.navWidth * f.nav).Shrink(0).ClipX().AlignItems(ui.Stretch).Children(func() {
		col := kit.Pane(c, ui.Column(c.Key("content")).Width(f.navWidth).Shrink(0), p.Sidebar.Alpha(float32(panel.A)/255), a.frosted, a.Surfaces.Blur*theme.PaneBlurMax).
			BorderWidth(0, 1, 0, 0).BorderColor(p.SidebarBorder)
		kit.Draws(col, kit.Backdrop(c, "background.sidebar")).Children(func() {
			if a.Page == SettingsPage {
				settings.Nav(c, a.Settings)
			} else if navigation := a.workspaceSlots().Navigation; navigation != nil {
				if a.Page == PluginPage {
					navigation(c)
				} else {
					workspaceSlot(c, navigation)
				}
			} else {
				a.Sidebar.Terminal = a.ActiveTerminal()
				sidebar.View(c, a.Sidebar)
			}
		})
	})
	if f.nav >= 0.999 {
		resize(c, "nav-handle", i18n.T(c, "sidebar_toggle"), 1, &a.NavWidth, NavWidth, a.clampNavigation, f.zoom)
	}
}

// details is the right panel's column, anchored at the window's right
// edge as it slides open and closed.
func (a *App) details(c *ui.Context, f frame, fill ui.Color) {
	if f.details <= 0.001 {
		return
	}
	p := theme.Of(c)
	if f.details >= 0.999 {
		// The fit helpers already subtract the base rail; count only its zoom allowance here.
		nav := (f.navWidth*f.nav+f.rail)*f.zoom - RailWidth
		width, reset := &a.PanelWidth, float32(PanelWidth)
		clamp := func(v float32) float32 {
			return ClampPanel(v, f.window, nav)
		}
		if a.workspaceDetails() {
			width, reset = &a.pluginWorkspacePrefs().Width, a.pluginDetailsDefault
			clamp = func(v float32) float32 { return a.clampPluginDetails(v, f.window, nav) }
		}
		resize(c, "panel-handle", i18n.T(c, "details"), -1, width, reset, clamp, f.zoom)
	}
	ui.Row(c.Key("details")).Width(f.panelW * f.details).Shrink(0).ClipX().Justify(ui.End).AlignItems(ui.Stretch).Children(func() {
		col := kit.Pane(c, ui.Column(c.Key("content")).Width(f.panelW).Shrink(0), fill, a.frosted, a.Surfaces.Blur*theme.PaneBlurMax).BorderWidth(0, 0, 0, 1).BorderColor(p.Border)
		a.pane(c, col, &a.panelPane, true, func() {
			if a.extensionDetails() {
				a.Settings.CloseDetails()
				return
			}
			if a.paged() != "" || a.workspaceSlots().Details != nil {
				a.pluginWorkspacePrefs().Folded = true
				a.mainPane.Focus(c)
				return
			}
			if a.Page == HostPage {
				a.Metrics = false
				a.mainPane.Focus(c)
				return
			}
			a.closeTab(c)
		})
		col.Children(func() {
			if a.extensionDetails() {
				settings.DetailsPanel(c, a.Settings)
				return
			}
			if draw := a.workspaceSlots().Details; draw != nil {
				if a.Page == PluginPage {
					draw(c)
				} else {
					workspaceSlot(c, draw)
				}
				return
			}
			if page := a.nativePage(a.Page); page != nil && page.Resource != "" && page.RenderDetails != nil {
				if pr := a.project(); pr != nil {
					page.RenderDetails(c, a, pr)
				}
				return
			}
			if a.Page == HostPage {
				h, st := a.hostPage()
				a.sample(c, h)
				host.MetricsPanel(c, st, h)
				return
			}
			if a.Page == ConversationPage && a.PanelOpen {
				panel.View(c, a.Panel, a.panels.project, kit.KeyText)
			}
		})
	})
}

// syncPanel gives the panel what the page shows beside it, before its tabs
// in the header and its content in the column are built.
func (a *App) syncPanel() {
	a.Panel.Paged = a.paged()
	if a.ActiveTerminal() == "" {
		a.Panel.HasProject = a.Data.ProjectOf(a.Sidebar.Selected) != nil
		a.Panel.Cwd = ""
		selected := a.SelectedConversation()
		if pr := a.Data.ProjectHolding(selected); pr != nil {
			a.Panel.Cwd = pr.Path
		}
		if cv := selected; cv != nil && cv.WorktreePath != "" {
			a.Panel.Cwd = cv.WorktreePath
		}
	} else if a.sampled() {
		// A live terminal pane's project and folder come from its Node with
		// its workspace; a sample terminal names its own project.
		pr := a.panelProject()
		a.Panel.HasProject, a.Panel.Cwd = pr != nil, ""
		if pr != nil {
			a.Panel.Cwd = pr.Path
		}
	}
	if a.Page == ConversationPage {
		a.panels.project = a.project()
		a.Panel.BindProject(a.panels.project)
	}
	a.Panel.Focus = func(c *ui.Context) {
		if a.PanelOpen {
			a.panelPane.Focus(c)
		} else {
			a.mainPane.Focus(c)
		}
	}
}

// resize is a column's drag handle: a 6 DIP strip over the edge with the
// resize cursor; dragging moves the edge by dir (1 for an edge on the
// right of the column, -1 on its left), and a double click puts the
// column back at reset. Custom: ui.Split keeps both panes 40 DIPs wide and
// cannot close a pane to nothing, which the columns' motion needs. width,
// reset and clamp are in the window's DIPs, zoom of the content's the
// drag moves by.
func resize(c *ui.Context, key, label string, dir float32, width *float32, reset float32, clamp func(float32) float32, zoom float32) {
	p := theme.Of(c)
	h := ui.Box(c.Key(key)).Width(handle).Margin(0, -handle/2).ZIndex(1).Cursor(ui.CursorResizeEW).Label(label).Shrink(0)
	if h.Hovered() || h.Pressed() {
		// A 2 DIP line in the middle of the wider hit area.
		h.Justify(ui.Center).Children(func() { ui.Box(c).Width(2).FillHeight().Background(p.Ring.Alpha(0.5)) })
	}
	if dx, _, ok := h.Dragged(); ok {
		*width = clamp(clamp(*width) + dir*dx*zoom)
	}
	if h.DoubleClicked() {
		*width = clamp(reset)
	}
}

// togglePanel handles the header button. Closing a conversation's panel here
// removes its tabs, unlike the keyboard layout toggle.
func (a *App) togglePanel() {
	if a.togglePageDetails() || a.Page != ConversationPage || a.panelOwner() != a.panels.mounted {
		return
	}
	if a.PanelOpen {
		a.Panel.Clear()
		a.foldConversationPanel()
	} else {
		a.PanelOpen = true
	}
}

// togglePageDetails handles pages with their own right column, not the
// conversation's panel.
func (a *App) togglePageDetails() bool {
	switch {
	case a.extensionDetails():
		// The details show for a selected item: folding them deselects it.
		a.Settings.CloseDetails()
	case a.workspaceDetails():
		state := a.pluginWorkspacePrefs()
		state.Folded = !state.Folded
	case a.Page == HostPage:
		a.Metrics = !a.Metrics
	default:
		return false
	}
	return true
}

// openPanel shows a resource in the right panel.
func (a *App) openPanel(k panel.Kind) {
	a.syncPanel()
	if !a.Panel.Offers(k) {
		// A shortcut opens only what the launcher lists.
		return
	}
	a.RevealConversationPanel()
	a.Panel.Open(k)
}

// pane makes e a pane the focus goes into as it is clicked, as the
// reference's track_focus on a mouse down, and has it take ⌘W
// (CloseFocused) while the focus is inside it and it can close: the
// innermost element taking a key gets it (Element.Shortcut), so the panel
// and a page close only themselves, and nothing outside them sees the key.
func (a *App) pane(c *ui.Context, e ui.Element, ref *ui.Handle, closes bool, close func()) {
	e.Focusable().FocusRing(false).Bind(ref)
	if closes && e.Shortcut(CloseFocused.Mods, CloseFocused.Key) && !c.ShortcutRepeated() {
		close()
	}
}

// closeTab is ⌘W in the right panel: its active tab, the focus staying in the
// panel for the next, or the panel itself with none left, the focus then
// going back to the conversation.
func (a *App) closeTab(c *ui.Context) {
	if a.Page != ConversationPage || !a.PanelOpen || a.panelOwner() != a.panels.mounted {
		return
	}
	if a.Panel.Active != (panel.Tab{}) && a.Panel.Close(a.Panel.Active) {
		a.panelPane.Focus(c)
		return
	}
	a.foldConversationPanel()
	a.mainPane.Focus(c)
}

// adjacent is the conversation after or before the selected one in the
// sidebar's order.
func adjacent(a *App, forward bool) string {
	return sidebar.Adjacent(a.Sidebar.Order(), a.Sidebar.Selected, forward)
}

// page is the main panel: the conversation, the settings page, or a
// feature's page.
func (a *App) page(c *ui.Context) {
	settings.SettleEdits(c, a.Settings.Model)
	if bound := a.activeWindowBinding(); bound.Render != nil {
		bound.Render(c)
		return
	}
	if page := a.nativePage(a.Page); page != nil {
		if page.Render != nil {
			page.Render(c, a)
			return
		}
		if page.BindWindow != nil {
			a.unavailable(c)
			return
		}
	}
	switch a.Page {
	case NotificationsPage:
		a.notificationCenter(c)
		return
	case SettingsPage:
		settings.Page(c, a.Settings, a.Data, settings.Look{Theme: &a.Theme, Mode: &a.Mode, Effects: &a.Effects, Surfaces: &a.Surfaces, Mark: &a.Messages.Mark})
		return
	case PluginPage:
		if a.Hooks.PluginPage != nil {
			a.Hooks.PluginPage(c, a.pluginPage)
		} else if f, ok := a.lookupFeature(a.pluginPage); ok {
			kit.Result(c, kit.ResultProps{Icon: f.Icon, Title: i18n.T(c, f.Label), SubTitle: i18n.T(c, "unavailable")}).Grow(1).Justify(ui.Center)
		}
		return
	case HostPage:
		a.hostView(c)
		return
	case ProjectPage:
		a.projectView(c)
		return
	}
	if a.split(c) {
		return
	}
	if cv := a.SelectedConversation(); cv != nil {
		if t := session(cv.ID); a.panes.enlargedFrame == t {
			a.enlargedSlot(c, t)
			return
		}
		a.conversation(c, cv, a.mainPane)
	}
}

// conversation is a session's transcript and composer, alone in the main
// column or in a pane of a split, focus being where it takes its keys.
// Each session keeps its own list, composer and search: the transcript's
// state is kept by session, and a session shows in one pane at most.
func (a *App) conversation(c *ui.Context, cv *preview.Conversation, focus ui.Handle) {
	// Only the explicit preview advances simulated replies.
	if a.sampled() {
		preview.Advance(cv, c.Now())
	}
	if a.sampled() && cv.Playing() {
		c.After(preview.Tick)
	}
	if a.Demo() {
		a.demoControls(cv)
	}
	comp := a.Composer(cv.ID)
	comp.Running = cv.Running()
	comp.Searching = a.Messages.SearchOpenIn(cv.ID)
	// A sample conversation's figures are its own usage. A live session's
	// are its packages' (the statistics and footer slots), which the
	// controller's hook builds: both would show each figure twice. The
	// demo's are the statistics package's too where a controller mounts it
	// (Hooks.DemoConversation), read from the conversation's own usage.
	comp.Metrics = nil
	if a.sampled() {
		comp.Metrics = func(c *ui.Context, compact bool, open *bool) {
			conversation.UsageBar(c, nil, [][]conversation.Metric{conversation.Metrics(c, cv.Session.Usage)}, compact, open)
		}
	}
	// A sample conversation's context meter reads its own usage; the
	// controller's hook gives a live session's (and the demo's) theirs.
	comp.Indicator = nil
	if a.sampled() {
		stats := conversation.UsageContext(cv.Session.Usage, comp.Chosen(a.Data.Models).Name)
		comp.Indicator = func(c *ui.Context) { conversation.ContextMeter(c, &comp.Meter, stats) }
	}
	var act ConversationActions = &actions{a: a, c: c, cv: cv, comp: comp}
	if a.sampled() {
		comp.SetAttachmentCallbacks(&composer.AttachmentCallbacks{Pick: func() { a.sampleAttach(c, cv, comp, act) }})
	}
	if a.sampled() && a.transfers.Advance(comp.Attachments, c.Now()) {
		c.After(preview.Tick)
	}
	if a.sampled() {
		if changing, done := a.voice.Advance(&comp.Dictation, c.Now()); changing {
			c.After(preview.Tick)
		} else if done {
			// The demo hears what its catalog says; the preview says that no
			// microphone opened.
			if text := preview.DemoCatalog(a.demoLocale())["dictation.text"]; a.Demo() && text != "" {
				if comp.Draft != "" && !strings.HasSuffix(comp.Draft, " ") && !strings.HasSuffix(comp.Draft, "\n") {
					comp.Draft += " "
				}
				comp.Draft += text
			} else {
				kit.PreviewNotice(c)
			}
		}
	}
	ctx := &composer.Context{Models: a.Data.Models, Readonly: true}
	if a.sampled() {
		ctx = a.sampleComposer(cv, comp)
		if a.Demo() && a.Hooks.DemoConversation != nil {
			a.Hooks.DemoConversation(c, cv, comp)
		}
	} else if a.Hooks.Conversation != nil {
		ctx, act = a.Hooks.Conversation(c, cv, comp)
	}
	if a.keyed != "" && cv.ID == a.Sidebar.Selected {
		switch a.keyed {
		case "conversation.stop":
			if comp.Stops() {
				act.Stop()
			}
		case "composer.focus":
			comp.Focus()
		case "composer.models":
			comp.OpenModels(ctx)
		}
		a.keyed = ""
	}
	// Escape in the conversation after the picker and the search bar: the
	// queue's editor, then Stop.
	if comp.Escapes() && c.FocusWithin(focus) && c.Shortcut(0, ui.KeyEscape) {
		comp.Escape(act)
	}
	if cv.ID == a.Sidebar.Selected {
		if !a.sampled() && a.Hooks.RenderChild != nil {
			a.Panel.Child = a.Hooks.RenderChild
		} else {
			a.Panel.Child = func(c *ui.Context, id string) {
				conversation.View(c, a.Messages, id, a.children[id], nil, act)
			}
		}
	}
	if len(cv.Turns) == 0 && !cv.Session.Loading && cv.Session.Connection == conversation.Connected {
		// A new session welcomes with the theme's hero and the mode
		// cards, whose prompts go to the composer as drafts.
		conversation.Welcome(c, a.Messages, func() {
			composer.View(c, comp, ctx, act)
		}, func(prompt string) { comp.Draft = prompt })
		return
	}
	// The composer floats over the transcript's end, which fills the pane.
	// The turns and composer stay centered in the full pane. The activity
	// card fits in its right margin, or uses the header popover when narrow.
	entry := a.overviewEntry(cv.ID)
	entry.act = act
	overview := a.conversationOverview(c, cv, act)
	row := ui.Row(c.Key("activity-row")).FillWidth().Grow(1).MinHeight(0).AlignItems(ui.Stretch)
	bounds := row.Bounds()
	entry.state.Docked = a.overviewDocked(overview)
	docked := entry.state.Docked && !a.OverviewHidden
	row.Children(func() {
		ui.Column(c.Key("transcript-column")).Grow(1).MinWidth(0).AlignItems(ui.Stretch).Children(func() {
			conversation.Docked(c, a.Messages, cv.ID, cv.Turns, &cv.Session, act, func() {
				composer.View(c, comp, ctx, act)
			})
			conversation.Navigation(c, a.Messages)
		})
		if docked {
			ui.Column(c.Key("overview-column")).Absolute().Top(overviewCardInset).Right(overviewCardInset+8).
				Translate(a.mainLayoutShift, 0).
				Children(func() {
					composer.OverviewCard(c, &entry.state, overview, min(bounds.H-2*overviewCardInset, overviewCardMax))
				})
		}
	})
	if !docked {
		composer.OverviewClosed(&entry.state, overview)
	}
}

// actions carries out the page's actions on the preview data. Preview
// data calls no model or tool: decisions and answers change only the
// preview's rows, and say so in a toast.
type actions struct {
	a    *App
	c    *ui.Context
	cv   *preview.Conversation
	comp *composer.State
}

// Send plays the preview's reply; attachments and references go along
// in name only.
func (x *actions) Send(m composer.Message) {
	if x.blocked() {
		return
	}
	text := m.Text
	if text == "" && len(m.Attachments) > 0 {
		text = i18n.T(x.c, "chat_attachment_count", "count", len(m.Attachments))
	}
	preview.Send(x.cv, text, x.a.clock)
	x.a.Messages.List(x.cv.ID).ScrollToEnd()
}

// Guide joins the message to the running reply, pending in the queue.
func (x *actions) Guide(m composer.Message) {
	if x.blocked() {
		return
	}
	preview.Guide(x.cv, m, x.a.clock)
	x.a.Messages.List(x.cv.ID).ScrollToEnd()
}

func (x *actions) Queue(m composer.Message) {
	if x.blocked() {
		return
	}
	preview.Queue(x.cv, m, x.a.clock)
	x.a.Messages.List(x.cv.ID).ScrollToEnd()
}

func (x *actions) SendQueued(id string) {
	if x.blocked() {
		return
	}
	preview.SendQueued(x.cv, id, x.a.clock)
	x.a.Messages.List(x.cv.ID).ScrollToEnd()
}

func (x *actions) WithdrawGuidance(id string) {
	if x.blocked() {
		return
	}
	preview.WithdrawGuidance(x.cv, id)
}
func (x *actions) SaveQueued(id, text string) {
	if x.blocked() {
		return
	}
	preview.SaveQueued(x.cv, id, text)
}
func (x *actions) RemoveQueued(id string) {
	if x.blocked() {
		return
	}
	preview.RemoveQueued(x.cv, id)
}
func (x *actions) MoveQueued(id, before string) {
	if x.blocked() {
		return
	}
	preview.MoveQueued(x.cv, id, before)
}

// Upload plays a transfer on the clock; the file is not read, which the
// toast says.
func (x *actions) Upload(a *composer.Attachment) {
	if x.blocked() {
		return
	}
	x.notice()
	x.a.transfers.Start(a, x.a.clock)
	x.c.After(preview.Tick)
}

func (x *actions) Discard(a *composer.Attachment) {
	if x.blocked() {
		return
	}
	x.a.transfers.Drop(a)
}

// Dictate plays voice input on the clock; no microphone opens.
func (x *actions) Dictate(d *composer.Dictation, cmd composer.DictationCommand) {
	if x.blocked() {
		return
	}
	x.a.voice.Command(d, cmd, x.a.clock)
	x.c.After(preview.Tick)
}

// Compact, a new worktree, a new project and the teams' settings would
// ask a model or write to disk: the preview says so. The demo does each in
// memory: a compaction turn that runs and completes, a worktree of the
// conversation's project chosen for it, the project editor and the teams'
// settings.
func (x *actions) Compact() {
	if !x.a.Demo() {
		x.notice()
		return
	}
	preview.Compact(x.cv, x.a.clock)
	x.a.Messages.List(x.cv.ID).ScrollToEnd()
}

func (x *actions) CreateWorktree() {
	pr := x.a.Data.ProjectOf(x.cv.ID)
	if !x.a.Demo() || pr == nil {
		x.notice()
		return
	}
	tree := x.a.Sidebar.AddWorktree(pr)
	x.comp.Worktree, x.cv.Worktree, x.cv.WorktreePath = tree.Branch, tree.Branch, tree.Path
}

func (x *actions) AddProject() {
	if !x.a.Demo() {
		x.notice()
		return
	}
	x.a.Sidebar.OpenEditor(nil)
}

func (x *actions) CanManageTeams() bool {
	return x.a.Settings.HasSection(settings.Teams)
}

func (x *actions) ManageTeams() {
	if !x.CanManageTeams() {
		return
	}
	if !x.a.Demo() {
		x.notice()
		return
	}
	x.a.Settings.Open(settings.Teams)
	x.a.Navigate(SettingsPage)
}

func (x *actions) Stop() {
	if x.blocked() {
		return
	}
	preview.Stop(x.cv)
}

// The activity strip: a file of the Git badge opens its diff; a plugin's
// action, stopping a command and opening a port would reach the host, so
// the preview says so.
func (x *actions) OpenChange(path string) {
	x.a.openPanel(panel.Changes)
	x.a.Panel.Show(panel.Tab{Kind: panel.Changes, ID: path})
}

// Badge in the demo is a button of the goal's panel, which pauses, resumes
// or ends the goal in memory.
func (x *actions) Badge(id, button string) {
	if !x.a.Demo() || !preview.GoalAction(x.cv, id, button) {
		x.notice()
	}
}

// StopCommand in the demo ends the background command: its row leaves.
func (x *actions) StopCommand(id string) {
	if !x.a.Demo() {
		x.notice()
		return
	}
	x.cv.Commands = slices.DeleteFunc(slices.Clone(x.cv.Commands), func(k composer.Command) bool { return k.ID == id })
}

// OpenService in the demo shows the service in the panel's browser.
func (x *actions) OpenService(int) {
	if !x.a.Demo() {
		x.notice()
		return
	}
	x.a.openPanel(panel.Browser)
}

func (x *actions) Decide(id string, d composer.Decision, reason string) {
	if x.blocked() {
		return
	}
	preview.Decide(x.cv, id, d)
	if !x.a.Demo() {
		kit.Message(x.c, i18n.T(x.c, "approval_preview_result"))
	}
}

func (x *actions) Answer(id string, ans composer.Answer) {
	if x.blocked() {
		return
	}
	var answers []string
	if q := x.cv.Waiting.Question; q != nil {
		for _, i := range ans.Choices {
			answers = append(answers, q.Options[i])
		}
	}
	if ans.Text != "" {
		answers = append(answers, ans.Text)
	}
	preview.Resolve(x.cv, id, conversation.QuestionAccepted, answers)
	x.notice()
}

func (x *actions) CancelQuestion(id string) {
	if x.blocked() {
		return
	}
	preview.Resolve(x.cv, id, conversation.QuestionCancelled, nil)
}

func (x *actions) DeclineQuestion(id string) {
	if x.blocked() {
		return
	}
	preview.Resolve(x.cv, id, conversation.QuestionDeclined, nil)
}

// OpenChild shows a subagent's session in a tab of the right panel: the
// preview's made-up run for it.
func (x *actions) OpenChild(id string) {
	if x.blocked() {
		return
	}
	title := ""
	for _, t := range x.cv.Turns {
		for _, b := range t.Reply.Blocks {
			for _, k := range b.Children {
				if k.ID == id {
					title = k.Title
				}
			}
		}
	}
	if x.a.children == nil {
		x.a.children = map[string][]conversation.Turn{}
	}
	if _, ok := x.a.children[id]; !ok {
		x.a.children[id] = preview.ChildTurns(id, title)
	}
	x.a.RevealConversationPanel()
	x.a.Panel.ShowChild(id, title)
}

// The transcript's actions that would run a model, change the session,
// write a file or leave the window only say so in the preview.
func (x *actions) notice() {
	if x.a.sampled() {
		kit.PreviewNotice(x.c)
		return
	}
	kit.Notify(x.c, "", i18n.T(x.c, "unavailable"))
}

func (x *actions) blocked() bool {
	if !x.a.sampled() {
		x.notice()
		return true
	}
	return false
}

// Retry in the demo runs the turn again: its reply plays from the start.
func (x *actions) Retry(turn string) {
	if !x.a.Demo() {
		x.notice()
		return
	}
	preview.Retry(x.cv, turn, x.a.clock)
	x.a.Messages.List(x.cv.ID).ScrollToEnd()
}

func (x *actions) ResumeLogin(string) { x.notice() }

// Fork in the demo copies the conversation up to the turn, in memory, and
// shows the copy; Worktree puts the copy in a new worktree of its project.
func (x *actions) Fork(turn string) { x.fork(turn, false) }

func (x *actions) Worktree(turn string) { x.fork(turn, true) }

func (x *actions) fork(turn string, worktree bool) {
	if !x.a.Demo() || worktree && x.a.Data.ProjectOf(x.cv.ID) == nil {
		x.notice()
		return
	}
	x.a.Sidebar.Fork(x.cv, turn, worktree)
	x.a.Page = ConversationPage
}

// Rewind in the demo drops the turns after the turn, which the transcript
// confirmed first.
func (x *actions) Rewind(turn string) {
	if !x.a.Demo() {
		x.notice()
		return
	}
	preview.Rewind(x.cv, turn)
}

// Export writes a file: the demo's transcript has no export menu.
func (x *actions) Export(string, conversation.ExportFormat) { x.notice() }

// Read speaks through the system's voice: the preview says so, and the
// demo, which reaches no voice, only shows the button reading.
func (x *actions) Read(string, string) { x.notice() }

// demoControls disables what a demo's turn cannot do: a worktree for a chat
// without a project.
func (a *App) demoControls(cv *preview.Conversation) {
	if a.Data.ProjectOf(cv.ID) != nil {
		return
	}
	for i := range cv.Turns {
		cv.Turns[i].Controls.NoWorktree = true
	}
}

// Edit opens the message's editor in its bubble; the preview's Save only says
// it changes nothing, the demo's replaces the message in memory, and Cancel
// restores the bubble.
func (x *actions) Edit(turn string) {
	for i := range x.cv.Turns {
		t := &x.cv.Turns[i]
		t.Editing = nil
		if t.ID == turn {
			t.Editing = &conversation.Editing{Text: t.User}
		}
	}
}

// SaveEdit in the demo replaces the message in memory and sends it again:
// the turns after it leave and its reply plays.
func (x *actions) SaveEdit(turn, text string) {
	if !x.a.Demo() {
		x.notice()
		return
	}
	preview.Resend(x.cv, turn, text, x.a.clock)
	x.a.Messages.List(x.cv.ID).ScrollToEnd()
}

func (x *actions) CancelEdit(turn string) {
	for i := range x.cv.Turns {
		if x.cv.Turns[i].ID == turn {
			x.cv.Turns[i].Editing = nil
		}
	}
}

// Start in the demo opens a new chat in memory with the quote as its draft.
func (x *actions) Start(text string) {
	if !x.a.Demo() {
		x.notice()
		return
	}
	x.a.Sidebar.NewChat()
	x.a.Page = ConversationPage
	x.a.Composer(x.a.Sidebar.Selected).Draft = text
}

// Undo in the demo asks over the History tab, as the application does, and
// then marks the card's file, or with "" all of them, undone in memory.
func (x *actions) Undo(turn, path string) {
	if !x.a.Demo() {
		x.notice()
		return
	}
	x.a.MountPanel()
	x.a.syncPanel()
	pr := x.a.project()
	if pr == nil {
		preview.Undo(x.cv, turn, path)
		return
	}
	target := path
	if target == "" {
		target = x.cv.Title
	}
	x.a.RevealConversationPanel()
	x.a.Panel.Sync(pr)
	x.a.Panel.Show(panel.Tab{Kind: panel.History})
	pr.Confirm("turn_changes_undo_all", target, func(done func(bool)) {
		preview.Undo(x.cv, turn, path)
		done(true)
	}, "workspace_restore_confirm")
}

// Review in the demo opens the file's diff, or the changes, in the panel.
func (x *actions) Review(_, path string) {
	if !x.a.Demo() {
		x.notice()
		return
	}
	x.a.openPanel(panel.Changes)
	if path != "" {
		x.a.Panel.Show(panel.Tab{Kind: panel.Changes, ID: path})
	}
}

// ImageAction is the image viewer's Copy, Save or Open, which would write
// the clipboard, a file or leave the window: the preview only says so.
func (x *actions) ImageAction(kit.ImageAction, conversation.Image) { x.notice() }

// Quote adds the quoted selection to the end of the draft.
func (x *actions) Quote(text string) {
	comp := x.a.Composer(x.cv.ID)
	if comp.Draft != "" && !strings.HasSuffix(comp.Draft, "\n\n") {
		comp.Draft += "\n\n"
	}
	comp.Draft += text
}

// LoadOlder reveals the next page of the preview's older turns.
func (x *actions) LoadOlder() {
	if x.blocked() {
		return
	}
	preview.LoadOlder(x.cv)
}

// FileAction opens a file in the system's app; saving and asking for
// edits only say so in the preview.
func (x *actions) FileAction(f conversation.File, a conversation.FileAction) {
	switch a {
	case conversation.OpenExternal:
		// The demo's files are on no disk: the click with the platform
		// modifier opens the tab too.
		if x.a.Demo() {
			x.OpenFile(f)
			return
		}
		x.openSystem(f)
	case conversation.Download:
		if !x.a.Demo() {
			x.a.startDownload(f.Name())
		}
	case conversation.RequestEdit:
		// The demo asks for the edit as the application does: the file and
		// the prompt go to the draft.
		if !x.a.Demo() {
			x.notice()
			return
		}
		x.comp.InsertFiles([]string{f.Path}, i18n.T(x.c, "artifact_edit_prompt"))
	default:
		x.notice()
	}
}

// openSystem hands a file to the system's default app through MyGo's
// OpenURL, the one place files leave the window. A file that is not
// there, as the preview's are not, toasts the failure.
func (x *actions) openSystem(f conversation.File) {
	path, err := filepath.Abs(f.Path)
	if err == nil {
		_, err = os.Stat(path)
	}
	if err != nil {
		kit.Toast(x.c, kit.Failed, i18n.T(x.c, "files_open_failed"))
		return
	}
	x.c.OpenURL((&url.URL{Scheme: "file", Path: path}).String())
}

// OpenFile shows a file's preview as a tab of the right panel; a file
// without a native preview opens in the system's app (WebView later). The
// demo's, which is on no disk, opens a tab that names it.
func (x *actions) OpenFile(f conversation.File) {
	if f.Kind == conversation.OtherFile && !x.a.Demo() {
		x.openSystem(f)
		return
	}
	x.a.RevealConversationPanel()
	x.a.Panel.ShowArtifact(f)
}
