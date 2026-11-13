package live

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginScopeKey struct {
	node         protocol.NodeID
	surface      string
	worktree     string
	session      string
	presentation string
}

type pluginMountKey struct {
	scope pluginScopeKey
	role  string
	name  string
}

type pluginViewFailure struct {
	reference protocol.PluginReference
	code      protocol.ErrorCode
	title     string
}

type pluginViewState struct {
	mounts       map[pluginMountKey]*pluginMountedView
	registries   map[pluginScopeKey]*pluginContributionOwner
	drafts       map[string]*viewmodel.Conversation
	navigation   map[string]pluginNavigationEntry
	settings     map[settings.Section]pluginSettingsEntry
	schemas      map[settings.Section]*pluginSchemaSettings
	failures     map[pluginMountKey]pluginViewFailure
	icons        map[string]*ui.SVG
	iconFailures map[string]bool
	generation   uint64
	closed       bool
	// locale is the window's resolved catalog, which package text follows.
	locale string
	// remounts are the bundles of the usage pages the Demo mode switch
	// disposed (demoChanged), which their next mounts take.
	remounts map[pluginMountKey]protocol.PluginDesktopBundle
}

type pluginMountedView struct {
	key            pluginMountKey
	owner          *Runtime
	info           protocol.PluginInfo
	scope          protocol.PluginContext
	locale         string
	generation     uint64
	ctx            context.Context
	cancel         context.CancelFunc
	registry       *desktopplugins.Registry
	hooks          desktopplugins.ContributionHooks
	host           *desktopplugins.Host
	runtime        *desktopplugins.Runtime
	mounted        *desktopplugins.Mounted
	components     []desktopplugins.NativeComponent
	headerFilters  map[string]*pluginHeaderFilterState
	headerOverflow bool
	headerBody     string
	// details is what the page's Workspace gave the right column in the
	// frame being drawn.
	details func(*ui.Context) error
	// detailsHeading is what those details gave the column's header.
	detailsHeading  func() func(*ui.Context)
	activityVisible func(bool)
	loading         bool
	failure         error
	failureTitle    string
	reported        bool
	seen            bool
	disconnected    bool
	// bundle is the package's immutable view as mounted, which a demo
	// conversation's mount of the package takes over (ensurePluginView).
	bundle *protocol.PluginDesktopBundle
}

func pluginScopeIdentity(node protocol.NodeID, scope protocol.PluginContext) pluginScopeKey {
	key := pluginScopeKey{node: node, surface: scope.Surface.Kind}
	if scope.Worktree != nil {
		key.worktree = scope.Worktree.String()
	}
	if scope.Session != nil {
		key.session = scope.Session.String()
	}
	return key
}

func (r *Runtime) initPluginViews() {
	if r.parent != nil || r.pluginViews != nil {
		return
	}
	r.pluginViews = &pluginViewState{
		mounts:       map[pluginMountKey]*pluginMountedView{},
		registries:   map[pluginScopeKey]*pluginContributionOwner{},
		drafts:       map[string]*viewmodel.Conversation{},
		navigation:   map[string]pluginNavigationEntry{},
		settings:     map[settings.Section]pluginSettingsEntry{},
		schemas:      map[settings.Section]*pluginSchemaSettings{},
		failures:     map[pluginMountKey]pluginViewFailure{},
		icons:        map[string]*ui.SVG{},
		iconFailures: map[string]bool{},
	}
	r.App.Hooks.SelectPlugin = r.selectPluginView
	r.App.Hooks.PluginPage = r.renderPluginPage
	r.App.Hooks.PluginHeader = r.renderPluginHeader
	r.App.Hooks.DemoChanged = r.demoChanged
}

func (r *Runtime) closePluginViews() {
	root := r.localRuntime()
	state := root.pluginViews
	if state == nil {
		return
	}
	if r.parent == nil {
		state.closed = true
	}
	for key, mounted := range state.mounts {
		if r.parent == nil || mounted.owner == r {
			root.dropPluginView(mounted, true)
			delete(state.mounts, key)
		}
	}
	for key, owner := range state.registries {
		if r.parent == nil || owner.owner == r {
			_ = owner.registry.Close()
			delete(state.registries, key)
		}
	}
	for section, schema := range state.schemas {
		if r.parent == nil || schema.owner == r {
			root.closePluginSchema(section, schema)
		}
	}
}

func (r *Runtime) pluginViewsFrame(view *ui.Context) {
	if r.parent != nil {
		return
	}
	r.initPluginViews()
	state := r.pluginViews
	if state == nil || state.closed {
		return
	}
	state.locale = i18n.Match(view.Locale())
	for _, owner := range state.registries {
		owner.seen = false
		if !rootPluginRegistryCurrent(r, owner) {
			r.closePluginRegistry(owner)
			continue
		}
		r.syncPluginRegistry(view, owner)
	}
	r.projectMenuRegistries()
	r.projectPluginViews(view)
	for section, schema := range state.schemas {
		// Re-entering a failed settings page is an explicit read attempt,
		// including after the user dismissed its recovery toast.
		if !rootPluginSchemaCurrent(r, section, schema) || schema.failure != nil &&
			(r.App.Page != shell.SettingsPage || r.App.Settings.Section != section) {
			r.closePluginSchema(section, schema)
		}
	}
	for key, mounted := range state.mounts {
		mounted.seen = false
		if !mounted.owner.controller.Connected() {
			r.disconnectPluginView(mounted)
		} else if mounted.disconnected {
			r.dropPluginView(mounted, false)
			delete(state.mounts, key)
			continue
		}
		if !r.pluginViewCurrent(mounted) {
			r.dropPluginView(mounted, mounted.owner.controller.Connected())
			delete(state.mounts, key)
			continue
		}
		if mounted.mounted != nil {
			if mounted.host != nil && !mounted.disconnected {
				native := mounted.host.Native()
				if native.Frame != nil {
					native.Frame(view)
				}
			}
			if err := mounted.mounted.Pump(); err != nil {
				r.failPluginView(view, mounted, err, "plugin_callback_failed")
			}
		}
		if mounted.failure != nil && !mounted.reported {
			r.failPluginView(view, mounted, mounted.failure, mounted.failureTitle)
		}
	}
}

func (r *Runtime) pluginViewsAfterFrame() {
	if r.parent != nil || r.pluginViews == nil {
		return
	}
	for _, mounted := range r.pluginViews.mounts {
		visible := mounted.seen
		if mounted.key.role == "contribution" {
			if owner := r.pluginViews.registries[mounted.key.scope]; owner != nil {
				visible = owner.seen
			}
		}
		if mounted.host != nil {
			mounted.host.SetVisible(visible)
		}
		if mounted.activityVisible != nil {
			mounted.activityVisible(visible && mounted.key.role == "navigation")
		}
	}
}

func pluginViewScopeExists(owner *Runtime, scope protocol.PluginContext) bool {
	if scope.Session != nil {
		session, exists := owner.session(scope.Session.String())
		if !exists || scope.Worktree != nil && session.Worktree != *scope.Worktree {
			return false
		}
	}
	if scope.Worktree != nil {
		for _, worktree := range owner.snapshot().Worktrees {
			if worktree.ID == *scope.Worktree {
				return true
			}
		}
		return false
	}
	return true
}

func (r *Runtime) pluginViewCurrent(mounted *pluginMountedView) bool {
	root := r.localRuntime()
	if root.pluginViews == nil || root.pluginViews.closed || root.pluginViews.mounts[mounted.key] != mounted ||
		mounted.ctx.Err() != nil && mounted.failure == nil && !mounted.disconnected || mounted.locale != root.packageLocale() ||
		!mounted.owner.controller.Connected() && !mounted.disconnected || !pluginViewScopeExists(mounted.owner, mounted.scope) {
		return false
	}
	if mounted.disconnected {
		return true
	}
	if mounted.key.role == "navigation" && !root.pluginNavigationRetained(mounted) {
		return false
	}
	if mounted.key.role == "settings" && !root.pluginSettingsRetained(mounted) {
		return false
	}
	if mounted.key.role == "contribution" {
		owner := root.pluginViews.registries[mounted.key.scope]
		if owner == nil || !rootPluginRegistryCurrent(root, owner) {
			return false
		}
	}
	snapshot := mounted.owner.snapshot()
	if mounted.host != nil {
		mounted.host.ObserveSettings(snapshot.Plugins, true)
	}
	for _, summary := range snapshot.Plugins {
		if summary.Name != mounted.scope.Package.Name {
			continue
		}
		if summary.Compatibility.Kind != "compatible" || !summary.Enabled && mounted.scope.Surface.Kind != "settings" {
			return false
		}
		if packageReference(summary) != mounted.scope.Package {
			return mounted.host != nil && summary.Digest == mounted.scope.Package.Digest && mounted.host.RetainSettings()
		}
		return true
	}
	return false
}

func (r *Runtime) ensurePluginView(view *ui.Context, owner *Runtime, info protocol.PluginInfo, scope protocol.PluginContext, role string, registry *desktopplugins.Registry) *pluginMountedView {
	root := r.localRuntime()
	root.initPluginViews()
	if root.pluginViews == nil || root.pluginViews.closed || owner == nil || !pluginViewScopeExists(owner, scope) {
		return nil
	}
	scope.Package = packageReference(info.Summary)
	key := pluginMountKey{scope: pluginScopeIdentity(owner.controller.Target(), scope), role: role, name: scope.Package.Name}
	if registry != nil {
		for identity, contributions := range root.pluginViews.registries {
			if contributions.registry == registry {
				key.scope = identity
				break
			}
		}
	}
	if mounted := root.pluginViews.mounts[key]; mounted != nil {
		if mounted.owner == owner && root.pluginViewCurrent(mounted) {
			mounted.seen = true
			return mounted
		}
		root.dropPluginView(mounted, true)
		delete(root.pluginViews.mounts, key)
	}
	if !owner.controller.Connected() {
		return nil
	}
	ctx, cancel := context.WithCancel(owner.ctx)
	root.pluginViews.generation++
	mounted := &pluginMountedView{
		key: key, owner: owner, info: info, scope: scope, locale: root.packageLocale(),
		generation: root.pluginViews.generation, ctx: ctx, cancel: cancel, registry: registry,
		loading: true, seen: true,
	}
	root.pluginViews.mounts[key] = mounted
	if registry != nil && info.Extension != nil {
		declarations := info.Extension.Ui
		if scope.Surface.Kind == "provider" || scope.Surface.Kind == "project" {
			declarations = pluginViewDeclarations(info.Summary.Name, declarations, scope.Surface.Kind)
		}
		hooks, err := registry.Hooks(scope.Package, declarations)
		if err != nil {
			root.failPluginView(view, mounted, err, "plugin_initialization_failed")
			return mounted
		}
		mounted.hooks = hooks
		_ = registry.SetActivity(scope.Package, false, false)
	}
	// The package is immutable, so a bundle read once is mounted again
	// without asking the Node.
	remount := func(bundle protocol.PluginDesktopBundle) *pluginMountedView {
		root.enqueue(func(current *ui.Context) {
			if mounted.ctx.Err() != nil || !root.pluginViewCurrent(mounted) {
				return
			}
			mounted.loading = false
			root.mountPluginView(current, mounted, bundle)
		})
		return mounted
	}
	// A usage page the Demo mode switch disposed takes the bundle it had.
	if bundle, kept := root.pluginViews.remounts[key]; kept {
		delete(root.pluginViews.remounts, key)
		if bundle.Package == scope.Package {
			return remount(bundle)
		}
	}
	// A demo conversation's mount takes the bundle another mount of the
	// package read: the demo asks the Node nothing it has answered.
	if strings.HasPrefix(key.scope.presentation, pluginDemoPresentation) {
		for _, other := range root.pluginViews.mounts {
			if other == mounted || other.bundle == nil || other.owner != owner || other.scope.Package != scope.Package ||
				other.scope.Surface.Kind != scope.Surface.Kind {
				continue
			}
			return remount(*other.bundle)
		}
	}
	if !owner.async(func() {
		output, err := owner.controller.Do(ctx, protocol.Command{Kind: "read_plugin_view", Data: protocol.CommandReadPluginViewPayload{
			Package: scope.Package, Surface: scope.Surface,
		}})
		bundle, valid := detail[protocol.PluginDesktopBundle](output.Data)
		if err == nil && (output.Kind != "plugin_view" || !valid || bundle.Package != scope.Package) {
			err = errors.New("unexpected immutable plugin view response")
		}
		root.enqueue(func(current *ui.Context) {
			if mounted.disconnected || mounted.ctx.Err() != nil || !root.pluginViewCurrent(mounted) {
				return
			}
			mounted.loading = false
			if err != nil {
				root.failPluginView(current, mounted, err, "plugin_initialization_failed")
				return
			}
			root.mountPluginView(current, mounted, bundle)
		})
	}) {
		root.dropPluginView(mounted, true)
		delete(root.pluginViews.mounts, key)
		return nil
	}
	return mounted
}

func (r *Runtime) mountPluginView(view *ui.Context, mounted *pluginMountedView, bundle protocol.PluginDesktopBundle) {
	root := r.localRuntime()
	mounted.bundle = &bundle
	native := mounted.owner.pluginNativeBinding(mounted.ctx, mounted.scope, mounted.key.scope.presentation)
	if native == nil {
		r.failPluginView(view, mounted, errors.New("plugin native services are unavailable"), "plugin_initialization_failed")
		return
	}
	bindPluginPanes(native)
	mounted.activityVisible = root.bindPluginActivity(mounted, native)
	mounted.activityVisible(mounted.key.role == "navigation" && mounted.seen)
	native.Invalidate = root.wake
	native.Header = func(content json.RawMessage) error {
		_, err := decodePluginHeader(content)
		if err == nil && mounted.headerBody != string(content) {
			mounted.headerBody = string(content)
			root.wake()
		}
		return err
	}
	if mounted.key.role == "navigation" {
		native.Details = func(render func(*ui.Context) error) { mounted.details = render }
		mounted.detailsHeading = native.DetailsHeading
	}
	post := native.Post
	native.Post = func(operation func(*ui.Context)) bool {
		if mounted.ctx.Err() != nil {
			return false
		}
		return post(func(current *ui.Context) {
			if mounted.ctx.Err() == nil && root.pluginViewCurrent(mounted) {
				operation(current)
			}
		})
	}
	var actions []protocol.PluginAction
	if mounted.info.Extension != nil {
		actions = mounted.info.Extension.Actions
	}
	host := desktopplugins.NewHost(desktopplugins.HostBinding{
		Context: mounted.ctx, Client: mounted.owner.controller, Scope: mounted.scope, Locale: mounted.locale,
		Actions: actions, Contributions: mounted.hooks, Native: native,
		ReadFiles:    slices.Contains(actions, protocol.PluginActionReadFiles),
		UsageSources: root.pluginUsageSources, UsageChanges: root.pluginUsageChanges(mounted.ctx),
		UsageSample: root.pluginUsageSample(),
	})
	mounted.host = host
	modules, err := host.Modules()
	if err != nil {
		r.failPluginView(view, mounted, err, "plugin_initialization_failed")
		return
	}
	components, err := desktopplugins.NativeComponents(host.Native())
	if err != nil {
		r.failPluginView(view, mounted, err, "plugin_initialization_failed")
		return
	}
	mounted.components = components
	if native.Frame != nil {
		native.Frame(view)
	}
	runtime, err := desktopplugins.NewRuntime(desktopplugins.RuntimeBindings{
		Modules: modules, Components: components, Contributions: mounted.hooks, Invalidate: root.wake,
		BindActions: host.Native().BindActions,
		Post: func(operation func()) {
			root.enqueue(func(*ui.Context) {
				if mounted.ctx.Err() != nil || !root.pluginViewCurrent(mounted) {
					return
				}
				operation()
			})
		},
	})
	if err != nil {
		r.failPluginView(view, mounted, err, "plugin_initialization_failed")
		return
	}
	mounted.runtime = runtime
	// The entry is noted before the view exists, as the reference does: the
	// view's first change already carries it. Noted after the mount it is a
	// second change, and the view loads everything twice.
	host.Enter()
	mounted.mounted, err = runtime.Mount(bundle, mounted.scope)
	if err != nil {
		r.failPluginView(view, mounted, err, "plugin_initialization_failed")
		return
	}
	if mounted.registry != nil {
		_ = mounted.registry.SetActivity(mounted.scope.Package, true, false)
	}
	delete(root.pluginViews.failures, mounted.key)
}

func (r *Runtime) dropPluginView(mounted *pluginMountedView, forget bool) {
	if mounted.activityVisible != nil {
		mounted.activityVisible(false)
	}
	mounted.cancel()
	if mounted.runtime != nil {
		_ = mounted.runtime.Close()
		mounted.runtime, mounted.mounted = nil, nil
	}
	if mounted.host != nil {
		_ = mounted.host.Close()
		mounted.host = nil
	}
	mounted.loading = false
	if mounted.registry != nil {
		if forget {
			_, _ = mounted.registry.Hooks(mounted.scope.Package, nil)
		} else {
			_ = mounted.registry.SetActivity(mounted.scope.Package, false, mounted.failure != nil)
		}
	}
}

func (r *Runtime) disconnectPluginView(mounted *pluginMountedView) {
	if mounted.disconnected || mounted.failure != nil {
		return
	}
	mounted.disconnected = true
	mounted.loading = false
	if mounted.activityVisible != nil {
		mounted.activityVisible(false)
	}
	if mounted.host != nil {
		_ = mounted.host.Disconnect()
	}
	mounted.cancel()
	if mounted.mounted != nil {
		mounted.mounted.Suspend()
	}
	if mounted.registry != nil {
		_ = mounted.registry.SetActivity(mounted.scope.Package, false, false)
	}
}

func (r *Runtime) failPluginView(view *ui.Context, mounted *pluginMountedView, failure error, title string) {
	mounted.failure = failure
	mounted.failureTitle = title
	r.dropPluginView(mounted, false)
	if mounted.registry != nil {
		_ = mounted.registry.SetActivity(mounted.scope.Package, false, true)
	}
	if view == nil || mounted.reported {
		return
	}
	mounted.reported = true
	signature := pluginViewFailure{reference: mounted.scope.Package, code: faultCode(failure), title: title}
	root := r.localRuntime()
	if previous, exists := root.pluginViews.failures[mounted.key]; exists && previous == signature {
		return
	}
	root.pluginViews.failures[mounted.key] = signature
	// A view that could not start is the Node's notice too; a callback's
	// failure is reported by the Node that ran it.
	if title == "plugin_initialization_failed" && mounted.owner != nil {
		mounted.owner.reportDiagnostic(pluginDiagnostic(protocol.NotificationSystemSystemSourcePluginInitialization,
			signature.code, mounted.scope.Package.Name, mounted.info.Summary.Version, "view_initialize", mounted.scope))
	}
	view.AddToast(ui.Toast{
		Type: "error", Title: i18n.T(view, title), Description: failure.Error(), Timeout: kit.ToastDuration(view),
		Action: i18n.T(view, "chat_retry"), OnAction: func() {
			r.enqueue(func(current *ui.Context) {
				if root.pluginViews != nil {
					if active := root.pluginViews.mounts[mounted.key]; active != nil && active.scope.Package == mounted.scope.Package {
						root.retryPluginView(current, active)
					}
				}
			})
		},
	})
}

func (r *Runtime) retryPluginView(view *ui.Context, mounted *pluginMountedView) bool {
	root := r.localRuntime()
	if mounted == nil || mounted.failure == nil || !root.pluginViewCurrent(mounted) {
		return false
	}
	root.dropPluginView(mounted, true)
	delete(root.pluginViews.mounts, mounted.key)
	delete(root.pluginViews.failures, mounted.key)
	return root.ensurePluginView(view, mounted.owner, mounted.info, mounted.scope, mounted.key.role, mounted.registry) != nil
}

func (r *Runtime) renderMountedPlugin(view *ui.Context, mounted *pluginMountedView) bool {
	if mounted == nil || !r.pluginViewCurrent(mounted) {
		return false
	}
	mounted.seen = true
	if mounted.mounted == nil {
		// A mount takes a few frames; a spinner would only flash. A failed
		// view draws nothing either: its toast reports it and retries.
		return false
	}
	_, err := mounted.mounted.Render(view.Key(mounted.generation))
	if err != nil {
		r.failPluginView(view, mounted, err, "plugin_callback_failed")
		return false
	}
	return true
}

func (r *Runtime) pluginViewCatalog(owner *Runtime) []protocol.PluginInfo {
	if owner == nil {
		return nil
	}
	if owner.packages == nil {
		owner.packages = newPackageState()
	}
	var entries []protocol.PluginInfo
	for _, summary := range owner.snapshot().Plugins {
		if removed, exists := owner.packages.removed[summary.Name]; exists && summary.Revision <= removed {
			continue
		}
		info, exists := owner.packages.infos[summary.Name]
		if (!exists || !samePackageVersion(info.Summary, summary)) && owner.controller.Connected() && !owner.packages.reads[summary.Name] {
			if version := packageVersion(summary); owner.packages.failures[summary.Name] != version {
				owner.readPackage(summary.Name, version, nil)
			}
		}
		if !exists || info.Summary.Digest != summary.Digest {
			continue
		}
		info.Summary = summary
		entries = append(entries, info)
	}
	return entries
}

func (r *Runtime) pluginNavigationIcon(view *ui.Context, declaration *protocol.PluginDesktopIcon) *ui.SVG {
	if declaration == nil {
		return icons.Puzzle
	}
	root := r.localRuntime()
	body, err := json.Marshal(declaration)
	key := string(body)
	if icon := root.pluginViews.icons[key]; icon != nil {
		return icon
	}
	var icon *ui.SVG
	if err == nil {
		switch declaration.Kind {
		case "Name":
			var name string
			if json.Unmarshal(body, &name) == nil {
				icon, err = icons.Plugin(name)
			}
		case "Svg":
			var value protocol.PluginDesktopIconSvgPayload
			if err = json.Unmarshal(body, &value); err == nil {
				icon, err = ui.ParseSVG([]byte(value.Svg))
			}
		default:
			err = fmt.Errorf("unsupported plugin navigation icon")
		}
	}
	if icon == nil {
		if view != nil && !root.pluginViews.iconFailures[key] {
			root.pluginViews.iconFailures[key] = true
			if err == nil {
				err = errors.New("plugin navigation icon is unavailable")
			}
			root.packageReport(view, err)
		}
		return icons.Puzzle
	}
	root.pluginViews.icons[key] = icon
	return icon
}
