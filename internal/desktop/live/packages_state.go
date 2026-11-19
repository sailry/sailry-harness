package live

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"slices"
	"sort"
	"strings"
	"sync/atomic"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/transfers"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type packageState struct {
	working            map[string]int // the packages worked on, by the view's ID
	infos              map[string]protocol.PluginInfo
	removed            map[string]uint64
	reads              map[string]bool
	failures           map[string]string
	updates            map[string]protocol.PluginUpdatesReport
	directories        map[string]*localPackage // the updates read from a local directory
	catalog            map[string]protocol.PluginCatalogEntry
	catalogSources     map[string]protocol.PluginCatalogSource
	catalogInfos       map[string]protocol.PluginInfo
	catalogWaiters     map[string][]func()
	skillCatalog       map[string]protocol.PluginSkillcatalogEntry
	updateChecks       map[string]string
	documents          map[string]string
	marketKey          string
	marketGeneration   uint64
	marketCancel       context.CancelFunc
	pluginMarketNext   uint32
	skillMarketPages   [len(skillMarketSources)]protocol.PluginSkillcatalogPage
	requests           map[string]protocol.Request
	pending            map[string]protocol.Request
	tasks              []protocol.InstallationTask
	connected          bool
	observing          bool
	installFailed      bool
	pythonObserving    bool
	pythonFailed       bool
	pythonReadFailed   bool
	preparations       []settings.PythonPreparation
	queue              transfers.Queue
	pythonRead         bool
	pythonReady        bool
	pythonDraft        settings.Stored[settings.Python]
	pythonVersion      string
	selectedNode       string
	profile            *theme.Profile
	market             *theme.Market
	themes             map[string]theme.Snapshot
	themeCatalog       theme.CatalogListing
	themeBusy          map[string]bool
	themeRead          bool
	themeGeneration    atomic.Uint64
	downloadCancel     context.CancelFunc
	downloadGeneration uint64
	terminals          map[string]*liveTerminal
	skillOperation     *skillSourceOperation
	configuration      uint64
	images             *marketImages
}

func newPackageState() *packageState {
	return &packageState{
		infos: map[string]protocol.PluginInfo{}, removed: map[string]uint64{}, reads: map[string]bool{}, failures: map[string]string{},
		updates: map[string]protocol.PluginUpdatesReport{}, directories: map[string]*localPackage{},
		catalog: map[string]protocol.PluginCatalogEntry{}, catalogSources: map[string]protocol.PluginCatalogSource{}, catalogInfos: map[string]protocol.PluginInfo{}, catalogWaiters: map[string][]func(){}, skillCatalog: map[string]protocol.PluginSkillcatalogEntry{}, updateChecks: map[string]string{},
		documents: map[string]string{}, requests: map[string]protocol.Request{}, pending: map[string]protocol.Request{},
		themes: map[string]theme.Snapshot{}, themeBusy: map[string]bool{}, images: newMarketImages(),
	}
}

func (r *Runtime) initB8Packages() {
	r.packages = newPackageState()
	r.packages.profile = theme.NewProfile(filepath.Dir(r.Preferences.Path))
	r.packages.market = theme.NewMarket(r.packages.profile)
	unsubscribeThemes := r.packages.profile.OnChange(r.readInstalledThemes)
	context.AfterFunc(r.ctx, unsubscribeThemes)
	m := r.App.Settings.Model
	m.Indexes = []string{"pypi.org", "mirrors.aliyun.com", "mirrors.tencent.com", "pypi.tuna.tsinghua.edu.cn", "custom"}
	m.PackageActions = &settings.PackageActions{
		Image: r.marketImage,
		CheckUpdates: func(kind viewmodel.PackageKind) {
			if owner := r.packageOwner(); owner != nil {
				owner.checkPackageUpdates(kind)
			}
		},
		Update: func(ids []string) {
			if owner := r.packageOwner(); owner != nil {
				owner.updatePackages(ids)
			}
		},
		Toggle: func(id string, enabled bool) {
			if owner := r.packageOwner(); owner != nil {
				owner.togglePackage(id, enabled)
			}
		},
		Configure: func(id string) {
			if owner := r.packageOwner(); owner != nil {
				owner.configurePackage(id)
			}
		},
		Uninstall: func(id string, done func(bool)) {
			if owner := r.packageOwner(); owner != nil {
				owner.uninstallPackage(id, done)
			} else {
				done(false)
			}
		},
		Install: func(path, directory string, done func(bool)) {
			if owner := r.packageOwner(); owner != nil {
				owner.installLocalPlugin(path, directory, done)
			} else {
				done(false)
			}
		},
		Browse: func(name, initial string, valid func() bool, selected func(string)) {
			r.localRuntime().chooseFile(name, "plugins_package", initial, valid, selected)
		},
		Download: func(repository, revision, path string, done func(bool)) {
			if owner := r.packageOwner(); owner != nil {
				owner.installGitPlugin(repository, revision, path, done)
			} else {
				done(false)
			}
		},
		CancelDownload: func() {
			if owner := r.packageOwner(); owner != nil && owner.packages.downloadCancel != nil {
				owner.packages.downloadCancel()
			}
		},
		LoadDetails: func(kind settings.DetailKind, id string, done func(string)) {
			if owner := r.packageOwner(); owner != nil {
				owner.loadPackageDetails(kind, id, done)
			} else if kind == settings.DetailSkill {
				done("resource host is offline")
			} else {
				done("")
			}
		},
		RetryMarket: func() {
			root := r.localRuntime()
			m := root.App.Settings.Model
			if m.MarketKind == 2 {
				root.packages.themeRead = false
			}
			r.searchPackageMarket(m.MarketKind, marketSource(m), m.MarketQuery, true)
		},
		MoreMarket: func() {
			if owner := r.packageOwner(); owner != nil {
				owner.loadMoreMarket()
			}
		},
		MarketPlugin: func(id string) {
			if owner := r.packageOwner(); owner != nil {
				owner.installMarketPlugin(id)
			}
		},
		PluginVersion: func(id, version string) {
			if owner := r.packageOwner(); owner != nil {
				owner.installPackageVersion(id, version)
			}
		},
		PinPlugin: func(id string, pinned bool) {
			if owner := r.packageOwner(); owner != nil {
				owner.pinPackage(id, pinned)
			}
		},
		MarketSkill: func(id string) {
			if owner := r.packageOwner(); owner != nil {
				owner.installMarketSkill(id)
			}
		},
		Theme:       r.installMarketTheme,
		ChooseSkill: r.chooseSkillSource,
		InstallSkill: func(draft settings.SkillSourceDraft, done func(bool)) {
			if owner := r.packageOwner(); owner != nil {
				owner.installSkillSource(draft, done)
			} else {
				done(false)
			}
		},
		CancelSkill: func() {
			if owner := r.packageOwner(); owner != nil {
				owner.cancelSkillSource()
			}
		},
		ReimportSkill: func(id string) {
			if owner := r.packageOwner(); owner != nil {
				owner.reimportSkill(id)
			}
		},
		SavePython: func() {
			if owner := r.packageOwner(); owner != nil {
				owner.savePython()
			}
		},
		About: func() { r.App.Settings.Open(settings.About); r.App.Navigate(shell.SettingsPage) },
	}
	m.AppearanceActions = &settings.AppearanceActions{
		Installed:    func(id string) bool { _, exists := r.packages.themes[id]; return exists },
		CheckUpdates: func() { r.readThemeCatalog(true) }, Import: r.importTheme, Export: r.exportTheme, Uninstall: r.uninstallTheme,
	}
	r.App.SetInstallations(&r.packages.queue)
	r.App.Hooks.PackageDialogs = r.packageDialogs
	r.initLiveTerminals()
	r.readInstalledThemes()
	r.packagesFrame(nil)
}

// packageOwner is the explicitly selected Node for package management.
func (r *Runtime) packageOwner() *Runtime {
	owner := r.managedSettingsNode()
	if owner == nil {
		return nil
	}
	if owner.packages == nil {
		owner.packages = newPackageState()
	}
	return owner
}

// openPackageDetails opens a package on the Node that supplied the link.
func (r *Runtime) openPackageDetails(c *ui.Context, name string) {
	root := r.localRuntime()
	root.selectPackageSettingsNode(r)
	root.App.Settings.Open(settings.Plugins)
	root.App.Navigate(shell.SettingsPage)
	root.packagesFrame(c)
	root.App.Settings.ShowDetails(settings.DetailPlugin, name)
}

func (r *Runtime) packagesFrame(c *ui.Context) {
	root := r.localRuntime()
	owner := r.packageOwner()
	m := root.App.Settings
	node := ""
	if owner != nil {
		node = owner.controller.Target().String()
	}
	changed := root.packages.selectedNode != node
	if changed {
		if previousNode := root.packages.selectedNode; previousNode != "" {
			if previous := root.runtimeFor(previousNode); previous != nil && previous.packages != nil {
				previous.cancelMarketSearch()
				if m.PythonStatus == "" {
					previous.packages.pythonDraft = m.Python
				}
			}
		}
		root.packages.selectedNode = node
		root.packages.configuration++
		m.MarketPlugins, m.MarketSkills = nil, nil
		m.MarketPaging, m.MarketPagingFailed = false, false
		m.CloseDetails()
		m.CloseManagedPackageDialogs()
	}
	root.projectPythonSettings(owner, changed)
	if owner == nil {
		m.Packages, m.SkillList, m.MarketPlugins, m.MarketSkills = nil, nil, nil, nil
		if m.MarketKind != 2 {
			m.MarketLoad.Loading, m.MarketLoad.Loaded = false, true
			m.MarketMore, m.MarketFailed = false, false
		}
	} else {
		if changed {
			owner.packages.marketKey = ""
		}
		owner.projectPackages()
		if !owner.packages.observing {
			owner.observeInstallations()
		}
	}
	if root.App.Page == shell.MarketPage {
		root.searchPackageMarket(m.MarketKind, marketSource(m.Model), m.MarketQuery, false)
	}
	r.projectInstallations()
	r.projectMarketThemes()
}

func (r *Runtime) projectPythonSettings(owner *Runtime, changed bool) {
	m := r.App.Settings
	status := owner.machineSettingsStatus()
	if status == "" {
		state := owner.packages
		if !state.pythonObserving {
			owner.observePython()
		}
		if !state.pythonRead {
			owner.readPython()
		}
		switch {
		case state.pythonReadFailed:
			status = "unavailable"
		case !state.pythonReady:
			status = "plugins_loading"
		default:
			if changed || m.PythonStatus != "" {
				m.Python = state.pythonDraft
			}
			m.PythonVersion, m.PythonPreparing = state.pythonVersion, state.preparations
		}
	}
	if status != "" {
		m.Python, m.PythonVersion, m.PythonPreparing = settings.Stored[settings.Python]{}, "", nil
	}
	m.PythonStatus = status
}

func packageReference(summary protocol.PluginSummary) protocol.PluginReference {
	return protocol.PluginReference{Name: summary.Name, Digest: summary.Digest, SettingsRevision: summary.SettingsRevision}
}

func packageVersion(summary protocol.PluginSummary) string {
	return fmt.Sprintf("%d/%s/%d", summary.Revision, summary.Digest, summary.SettingsRevision)
}

// samePackageVersion compares what packageVersion formats, for the per-frame checks.
func samePackageVersion(first, second protocol.PluginSummary) bool {
	return first.Revision == second.Revision && first.Digest == second.Digest && first.SettingsRevision == second.SettingsRevision
}

func (r *Runtime) projectPackages() {
	state := r.packages
	if r.localRuntime().packageOwner() != r {
		return
	}
	var packages []viewmodel.Package
	var skills []viewmodel.Skill
	for _, summary := range r.snapshot().Plugins {
		if removed, exists := state.removed[summary.Name]; exists {
			if summary.Revision <= removed {
				continue
			}
			delete(state.removed, summary.Name)
		}
		info, exists := state.infos[summary.Name]
		if exists && (info.Summary.Revision > summary.Revision || info.Summary.Revision == summary.Revision && info.Summary.SettingsRevision > summary.SettingsRevision) {
			summary = info.Summary
		}
		if !exists || !samePackageVersion(info.Summary, summary) {
			if key := packageVersion(summary); !state.reads[summary.Name] && state.failures[summary.Name] != key {
				r.readPackage(summary.Name, key, nil)
			}
			info = protocol.PluginInfo{Summary: summary}
		}
		if r.parent != nil && desktopPlugin(info) {
			continue
		}
		view := packageView(info, r.packageLocale())
		view.Host = r.packageHostLabel()
		for id, entry := range state.catalog {
			if id == summary.Name || state.catalogInfos[id].Summary.Name == summary.Name {
				applyPackageCatalog(&view, entry, r.packageLocale())
			}
		}
		if report, ok := state.updates[summary.Name]; ok {
			applyPackageUpdate(&view, info, report, r.packageLocale())
		}
		packages = append(packages, view)
		for _, skill := range info.Skills {
			id := summary.Name + ":" + skill.Name
			row := viewmodel.Skill{ID: id, Name: localizedSkill(skill, r.packageLocale()), Description: skill.Description, Enabled: summary.Enabled, Content: state.documents[id]}
			if info.Skill == nil {
				row.Package = summary.Name
			} else {
				row.Origin, row.Update = packageOrigin(info), settings.Update(&view)
			}
			skills = append(skills, row)
		}
	}
	sort.Slice(packages, func(first, second int) bool { return packages[first].Name < packages[second].Name })
	sort.Slice(skills, func(first, second int) bool { return skills[first].Name < skills[second].Name })
	r.App.Settings.Packages, r.App.Settings.SkillList = packages, skills
	var catalogEntries []protocol.PluginCatalogEntry
	for _, row := range r.App.Settings.MarketPlugins {
		if entry, known := state.catalog[row.ID]; known {
			catalogEntries = append(catalogEntries, entry)
		}
	}
	r.readInstalledCatalogCandidates(catalogEntries)
	r.projectPluginMarket()
	r.projectSkillMarket()
	if r.App.Page == shell.MarketPage {
		if r.App.Settings.MarketKind == 1 {
			r.readMarketSkillUpdates()
		} else if r.App.Settings.MarketKind == 0 {
			r.readMarketPluginUpdates()
		}
	}
}

func (r *Runtime) packageHostLabel() string {
	root := r.localRuntime()
	if root == r {
		return "composer_host_local"
	}
	if host := root.hosts[r.controller.Target().String()]; host != nil && host.view.Name != "" {
		return host.view.Name
	}
	return r.controller.Target().String()
}

// packageLocale is the locale package text is read in: the window's resolved
// catalog, as the reference's rust_i18n::locale(), once a frame has reported
// it.
func (r *Runtime) packageLocale() string {
	if root := r.localRuntime(); root.pluginViews != nil && root.pluginViews.locale != "" {
		return root.pluginViews.locale
	}
	if r.Preferences.Data.Language != nil && *r.Preferences.Data.Language == "chinese" {
		return "zh-CN"
	}
	return "en"
}

func stringValue(value *string) string {
	if value != nil {
		return *value
	}
	return ""
}

func localizedSkill(skill protocol.PluginSkill, locale string) string {
	if text := skill.DisplayLocales[i18n.Match(locale)]; text != "" {
		return text
	}
	if text := stringValue(skill.DisplayName); text != "" {
		return text
	}
	return skill.Name
}

func packageOrigin(info protocol.PluginInfo) string {
	if info.UpdateSource != nil && info.UpdateSource.Kind == "official" {
		return "market_details_source_official"
	}
	if info.Origin != nil {
		switch info.Origin.Kind {
		case "bundled":
			return "market_details_source_official"
		case "online":
			return "market_details_source_community"
		}
	}
	return "market_details_source_local"
}

// addedPermissions are the labels of the actions an update asks that the
// installed package does not have.
func addedPermissions(installed *protocol.PluginExtension, candidate []protocol.PluginAction) []string {
	var added []string
	for _, action := range candidate {
		if label, known := packagePermissionLabels[action]; known && (installed == nil || !slices.Contains(installed.Actions, action)) {
			added = append(added, label)
		}
	}
	return added
}

func packageView(info protocol.PluginInfo, locale string) viewmodel.Package {
	summary := info.Summary
	view := viewmodel.Package{ID: summary.Name, Name: summary.Name, Description: stringValue(summary.Description), Version: stringValue(summary.Version), Enabled: summary.Enabled,
		Installed: summary.Revision != 0, Installable: summary.Compatibility.Kind == "compatible", Pinned: summary.Pinned, Source: packageOrigin(info)}
	view.Official = view.Source == "market_details_source_official"
	view.Requires, view.Unsupported = packageCompatibility(summary.Compatibility)
	if info.Skill != nil {
		view.Kind = viewmodel.KindSkill
	} else if info.McpSource != nil && info.Extension == nil {
		view.Kind = viewmodel.KindMCP
	}
	if extension := info.Extension; extension != nil {
		if extension.Display != nil {
			view.Name = pluginNavigation(*extension.Display, locale)
		}
		if extension.Description != nil {
			view.Description = pluginNavigation(*extension.Description, locale)
		}
		if extension.Category != nil {
			view.Category = "market_category_" + string(*extension.Category)
		}
		if extension.Display != nil {
			view.Icon = pluginIconSource(extension.Display.Icon)
		}
		if extension.Tint != nil {
			view.Tint = string(*extension.Tint)
		}
		for _, tool := range extension.Tools {
			view.Tools = append(view.Tools, tool.Name)
		}
		for _, action := range extension.Actions {
			view.Permissions = append(view.Permissions, packagePermissionLabels[action])
		}
	}
	for _, skill := range info.Skills {
		view.Skills = append(view.Skills, localizedSkill(skill, locale))
	}
	for _, mcp := range info.Mcp {
		view.MCP = append(view.MCP, mcp.Name)
	}
	view.Settings = packageSettingsFields(info.Settings, protocol.PluginSettingsState{}, locale)
	view.Versions = packageVersions(info.Versions, locale)
	view.Notes = packageReleaseNotes(info.ReleaseNotes, locale)
	includeInstalledPackageVersion(&view)
	return view
}

func (r *Runtime) readPackage(name, version string, done func(bool)) {
	r.packages.reads[name] = true
	r.async(func() {
		out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: name}})
		r.enqueue(func(c *ui.Context) {
			delete(r.packages.reads, name)
			info, valid := detail[protocol.PluginInfo](out.Data)
			if err == nil && (!valid || out.Kind != "plugin" || info.Summary.Name != name) {
				err = fmt.Errorf("unexpected plugin response")
			}
			if err == nil {
				if removed, exists := r.packages.removed[name]; !exists || info.Summary.Revision > removed {
					r.packages.infos[name] = info
				}
				delete(r.packages.failures, name)
				r.projectPackages()
			} else {
				r.packages.failures[name] = version
				r.packageReport(c, err)
				var released *string
				for _, summary := range r.snapshot().Plugins {
					if summary.Name == name {
						released = summary.Version
					}
				}
				r.reportDiagnostic(pluginDiagnostic(protocol.NotificationSystemSystemSourcePluginMetadata, faultCode(err), name, released, "metadata_read", protocol.PluginContext{}))
			}
			if done != nil {
				done(err == nil)
			}
		})
	})
}

// pluginIconSource is a declared icon as a view model carries it: its name,
// or the markup of its own SVG.
func pluginIconSource(icon *protocol.PluginDesktopIcon) string {
	if icon == nil {
		return ""
	}
	if icon.Kind == "Svg" {
		value, _ := detail[protocol.PluginDesktopIconSvgPayload](icon.Data)
		return value.Svg
	}
	name, _ := detail[string](icon.Data)
	return name
}

func pluginNavigation(value protocol.PluginDesktopNavigation, locale string) string {
	if text := value.Locales[i18n.Match(locale)]; text != "" {
		return text
	}
	return value.Label
}

func (r *Runtime) packageID(id string) string {
	if _, exists := r.packages.infos[id]; exists {
		return id
	}
	if info, known := r.packages.catalogInfos[id]; known {
		return info.Summary.Name
	}
	for name, info := range r.packages.infos {
		if slices.ContainsFunc(info.Skills, func(skill protocol.PluginSkill) bool { return name+":"+skill.Name == id }) {
			return name
		}
	}
	return id
}

func (r *Runtime) packageReport(c *ui.Context, err error) {
	if errors.Is(err, errDesktopPackageTarget) {
		kit.Toast(c, kit.Failed, i18n.T(c, "plugins_desktop_only"))
		return
	}
	r.reportKeys(c, err, "plugins_failed", "plugins_settings_conflict", "plugins_unknown")
}

func packageCommandKey(command protocol.Command) string {
	encoded, _ := json.Marshal(command)
	return string(encoded)
}

func (r *Runtime) packageCommand(command protocol.Command, done func(protocol.Output, bool)) {
	r.packageCommandReport(command, "plugins_failed", done)
}

// packageCommandReport is packageCommand with the failure toast's key, for a
// command whose failure has its own words.
func (r *Runtime) packageCommandReport(command protocol.Command, failed string, done func(protocol.Output, bool)) {
	key := packageCommandKey(command)
	if r.busy["packages/"+key] {
		// The command in flight answers its own caller; this one was not sent.
		if done != nil {
			done(protocol.Output{}, false)
		}
		return
	}
	r.busy["packages/"+key] = true
	request, retained := r.packages.pending[key]
	if !retained {
		request = r.controller.Prepare(command)
	}
	if strings.HasPrefix(command.Kind, "install_") {
		r.packages.requests[request.ID.String()] = request
	}
	r.async(func() {
		// An unobserved outcome is resolved by its request, which is kept
		// only while the Node still runs it or cannot be asked.
		output, err := r.controller.Settle(r.ctx, request)
		r.enqueue(func(c *ui.Context) {
			delete(r.busy, "packages/"+key)
			if client.Retained(err) {
				r.packages.pending[key] = request
			} else {
				delete(r.packages.pending, key)
			}
			if err == nil {
				delete(r.packages.requests, request.ID.String())
				if info, ok := detail[protocol.PluginInfo](output.Data); ok && output.Kind == "plugin" {
					r.packages.infos[info.Summary.Name] = info
					if strings.HasPrefix(command.Kind, "install_") {
						// The directory of an earlier installation is not this one's.
						r.forgetPackageDirectory(info.Summary.Name, info.Summary.Revision)
					}
					if command.Kind == "set_plugin_pinned" {
						key := "plugins_following_versions"
						if info.Summary.Pinned {
							key = "plugins_version_pinned"
						}
						kit.Toast(c, kit.Done, i18n.T(c, key))
					}
				}
				if command.Kind == "remove_plugin" {
					kit.Toast(c, kit.Done, i18n.T(c, "plugins_uninstalled"))
				}
			}
			r.reportKeys(c, err, failed, "plugins_settings_conflict", "plugins_unknown")
			if done != nil {
				done(output, err == nil)
			}
		})
	})
}

// packageWork marks the package the view names id as worked on, so its
// buttons are busy, until the returned release runs; operations on one
// package may overlap.
func (r *Runtime) packageWork(id string) (release func()) {
	root := r.localRuntime()
	if root.packages.working == nil {
		root.packages.working = map[string]int{}
	}
	root.packages.working[id]++
	root.App.Settings.SetBusy(settings.BusyPackage+id, true)
	released := false
	return func() {
		if released {
			return
		}
		released = true
		if root.packages.working[id]--; root.packages.working[id] <= 0 {
			delete(root.packages.working, id)
			root.App.Settings.SetBusy(settings.BusyPackage+id, false)
		}
	}
}

func (r *Runtime) togglePackage(id string, enabled bool) {
	name := r.packageID(id)
	info, exists := r.packages.infos[name]
	if !exists {
		return
	}
	release := r.packageWork(id)
	r.packageCommand(protocol.Command{Kind: "set_plugin_enabled", Data: protocol.CommandSetPluginEnabledPayload{Name: name, ExpectedRevision: info.Summary.Revision, Enabled: enabled}}, func(protocol.Output, bool) { release() })
}

func (r *Runtime) pinPackage(id string, pinned bool) {
	if r.App.Settings.Busy[settings.BusyPackage+id] {
		return
	}
	name := r.packageID(id)
	info, exists := r.packages.infos[name]
	if !exists || info.Summary.Pinned == pinned || !packageView(info, r.packageLocale()).Official {
		return
	}
	release := r.packageWork(id)
	r.packageCommand(protocol.Command{Kind: "set_plugin_pinned", Data: protocol.CommandSetPluginPinnedPayload{Name: name, ExpectedRevision: info.Summary.Revision, Pinned: pinned}}, func(_ protocol.Output, ok bool) {
		release()
		if ok {
			delete(r.packages.updates, name)
			r.projectPackages()
			if !pinned {
				r.readPackageUpdates([]string{name})
			}
		}
	})
}

func (r *Runtime) uninstallPackage(id string, done func(bool)) {
	name := r.packageID(id)
	info, exists := r.packages.infos[name]
	if !exists {
		done(false)
		return
	}
	release := r.packageWork(id)
	r.packageCommand(protocol.Command{Kind: "remove_plugin", Data: protocol.CommandRemovePluginPayload{Name: name, ExpectedRevision: info.Summary.Revision}}, func(_ protocol.Output, ok bool) {
		release()
		if ok {
			r.packages.removed[name] = info.Summary.Revision
			if selected := r.App.Settings.Details; r.packageOwner() == r && selected != nil && r.packageID(selected.ID) == name {
				r.App.Settings.CloseDetails()
			}
			delete(r.packages.infos, name)
			delete(r.packages.updates, name)
			r.forgetPackageDirectory(name, info.Summary.Revision)
		}
		done(ok)
	})
}

// forgetPackageDirectory drops the local directory of a package removed or
// installed again at the revision, with the update read from it.
func (r *Runtime) forgetPackageDirectory(name string, revision uint64) {
	delete(r.packages.directories, name)
	if r.Preferences != nil {
		r.Preferences.Data.ForgetPluginDirectory(r.controller.Target(), name, revision)
	}
}

func optionalPackageText(value string) *string {
	if strings.TrimSpace(value) == "" {
		return nil
	}
	value = strings.TrimSpace(value)
	return &value
}
