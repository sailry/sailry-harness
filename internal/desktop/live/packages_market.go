package live

import (
	"errors"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func catalogPackageView(entry protocol.PluginCatalogEntry, locale string, source protocol.PluginCatalogSource) viewmodel.Package {
	official := source == protocol.PluginCatalogSourceOfficial
	sourceKey := "market_details_source_official"
	if !official {
		sourceKey = "market_details_source_community"
	}
	row := viewmodel.Package{ID: entry.ID, Name: entry.Name, Version: stringValue(entry.Version), MarketVersion: stringValue(entry.Version),
		Description: stringValue(entry.Description), Source: sourceKey, Official: official,
		Installable: !official && entry.Compatibility.Kind == "compatible" || official && entry.CompatibleVersion != nil}
	if entry.Display != nil {
		row.Name = pluginNavigation(*entry.Display, locale)
		row.Icon = pluginIconSource(entry.Display.Icon)
	}
	if entry.Tint != nil {
		row.Tint = string(*entry.Tint)
	}
	if text := entry.DescriptionLocales[i18n.Match(locale)]; text != "" {
		row.Description = text
	}
	if entry.Category != nil {
		row.Category = "market_category_" + string(*entry.Category)
	}
	compatibility := entry.Compatibility
	if official && entry.CompatibleVersion != nil {
		row.Version = entry.CompatibleVersion.Version
		compatibility = entry.CompatibleVersion.Compatibility
	}
	row.Requires, row.Unsupported = packageCompatibility(compatibility)
	for _, permission := range entry.Permissions {
		row.Permissions = append(row.Permissions, packagePermissionLabels[permission])
	}
	row.Versions = packageVersions(entry.Versions, locale)
	row.Notes = packageReleaseNotes(entry.ReleaseNotes, locale)
	if official && entry.CompatibleVersion != nil {
		row.Notes = packageReleaseNotes(entry.CompatibleVersion.ReleaseNotes, locale)
	}
	return row
}

func (r *Runtime) loadPackageDetails(kind settings.DetailKind, id string, done func(string, string)) {
	switch kind {
	case settings.DetailTheme:
		done("", "")
	case settings.DetailSkill:
		r.loadSkillDocument(id, done)
	default:
		finish := func() { done("", "") }
		name := id
		if info, known := r.packages.catalogInfos[id]; known {
			name = info.Summary.Name
		}
		if info, installed := r.packages.infos[name]; installed {
			r.readPackageUpdates([]string{name})
			r.readInstalledPackageDetails(info, finish)
			return
		}
		entry, exists := r.packages.catalog[id]
		if !exists {
			finish()
			return
		}
		r.readCatalogInformation(entry, finish)
	}
}

func (r *Runtime) checkPackageUpdates(kind viewmodel.PackageKind) {
	var requests []protocol.PluginUpdatesRequest
	directories := map[string]string{}
	for name, info := range r.packages.infos {
		if packageView(info, r.packageLocale()).Kind == kind {
			requests = append(requests, protocol.PluginUpdatesRequest{Name: name, ExpectedRevision: info.Summary.Revision})
			if r.Preferences == nil {
				continue
			}
			if directory, known := r.Preferences.Data.PluginDirectory(r.controller.Target(), name, info.Summary.Digest); known {
				directories[name] = directory
			}
		}
	}
	busy := settings.BusyPluginUpdates
	if kind == viewmodel.KindSkill {
		busy = settings.BusySkillUpdates
	}
	if len(requests) == 0 || r.App.Settings.Busy[busy] {
		return
	}
	r.App.Settings.SetBusy(busy, true)
	cache := r.packageUpdateCache()
	r.async(func() {
		out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "check_plugin_updates", Data: protocol.CommandCheckPluginUpdatesPayload{Packages: requests, Force: true}})
		batch, valid := detail[protocol.PluginUpdatesBatch](out.Data)
		if err == nil && (!valid || out.Kind != "plugin_updates") {
			err = errors.New("unexpected plugin updates response")
		}
		local := map[string]*localPackage{}
		var catalogs map[string]protocol.PluginCatalogEntry
		if err == nil {
			for index := range batch.Reports {
				report := &batch.Reports[index]
				name := report.Current.Name
				if directory, known := directories[name]; known && report.Manual {
					var failure error
					if local[name], failure = r.directoryUpdate(report, directory); failure != nil {
						batch.Failures = append(batch.Failures, protocol.PluginUpdatesFailure{Name: name, Error: protocol.Fault{Code: faultCode(failure), Message: failure.Error()}})
					}
				}
			}
			catalogs = r.resolvePackageUpdates(&batch, cache)
			for _, report := range batch.Reports {
				if upload := local[report.Current.Name]; upload != nil && !packageUpdateAvailable(report) {
					r.cancelAttachmentStream(upload.stream)
					delete(local, report.Current.Name)
				}
			}
		}
		r.enqueue(func(c *ui.Context) {
			r.App.Settings.SetBusy(busy, false)
			if err != nil {
				r.packageReport(c, err)
				return
			}
			found := 0
			for id, entry := range catalogs {
				r.packages.catalog[id], r.packages.catalogSources[id] = entry, protocol.PluginCatalogSourceOfficial
			}
			for _, report := range batch.Reports {
				if !r.recordPackageUpdate(report, batch.Failures) {
					continue
				}
				if delete(r.packages.directories, report.Current.Name); local[report.Current.Name] != nil {
					r.packages.directories[report.Current.Name] = local[report.Current.Name]
				}
				r.packages.updateChecks[report.Current.Name] = packageVersion(report.Current)
				if packageUpdateAvailable(report) && samePackageVersion(report.Current, r.packages.infos[report.Current.Name].Summary) {
					found++
				}
			}
			r.App.Settings.MarkUpdatesChecked()
			r.projectPackages()
			if len(batch.Failures) > 0 {
				kit.Toast(c, kit.Failed, i18n.T(c, "plugins_failed"))
			} else if found == 0 {
				kit.Toast(c, kit.Done, i18n.T(c, "plugins_up_to_date"))
			} else {
				kit.Toast(c, kit.Done, i18n.T(c, "plugins_updates_found", "count", found))
			}
		})
	})
}

func (r *Runtime) updatePackages(ids []string) {
	for _, id := range ids {
		if r.App.Settings.Busy[settings.BusyPackage+id] {
			continue
		}
		r.updatePackage(id, r.packageWork(id))
	}
}

// updatePackage installs the package's newer version; finish runs once when
// the update ends or when there is none to install.
func (r *Runtime) updatePackage(id string, finish func()) {
	name := r.packageID(id)
	report, exists := r.packages.updates[name]
	current, installed := r.packages.infos[name]
	if !exists || !installed || current.Summary.Pinned || !samePackageVersion(current.Summary, report.Current) || !packageUpdateAvailable(report) {
		finish()
		return
	}
	if local := r.packages.directories[name]; local != nil {
		r.installPackageUpload(*local, func(ok bool) {
			finish()
			if ok {
				delete(r.packages.updates, name)
				r.projectPackages()
			}
		})
		return
	}
	candidate, valid := detail[protocol.PluginInfo](report.Available.Data)
	if !valid || candidate.Origin == nil || r.ensureDesktopPackage(candidate) != nil {
		finish()
		return
	}
	if candidate.UpdateSource != nil && candidate.UpdateSource.Kind == "official" {
		r.updateOfficialPackage(report, candidate, finish)
		return
	}
	var command protocol.Command
	switch candidate.Origin.Kind {
	case "bundled":
		command = protocol.Command{Kind: "install_bundled_plugin", Data: protocol.CommandInstallBundledPluginPayload{Name: name, ExpectedRevision: report.Current.Revision}}
	case "online":
		if origin, ok := detail[protocol.PluginOriginOnlinePayload](candidate.Origin.Data); ok {
			source := packageRepositoryUpdateSource(candidate, origin)
			if candidate.Skill != nil {
				command = protocol.Command{Kind: "install_skill", Data: protocol.CommandInstallSkillPayload{Source: source, Path: origin.Path, Name: name, ExpectedRevision: report.Current.Revision}}
			} else {
				command = protocol.Command{Kind: "install_plugin_source", Data: protocol.CommandInstallPluginSourcePayload{Source: source, Path: origin.Path, Name: name, ExpectedRevision: report.Current.Revision}}
			}
		}
	case "registry":
		if origin, ok := detail[protocol.PluginOriginRegistryPayload](candidate.Origin.Data); ok {
			command = protocol.Command{Kind: "install_registry_skill", Data: protocol.CommandInstallRegistrySkillPayload{Source: origin.Source, Name: name, ExpectedRevision: report.Current.Revision}}
		}
	case "worktree":
		if origin, ok := detail[protocol.PluginOriginWorktreePayload](candidate.Origin.Data); ok {
			digest := candidate.Summary.Digest
			command = protocol.Command{Kind: "install_plugin", Data: protocol.CommandInstallPluginPayload{Worktree: origin.Worktree, Path: origin.Path, Name: name, ExpectedRevision: report.Current.Revision, ExpectedDigest: &digest}}
		}
	}
	if command.Kind == "" {
		finish()
		return
	}
	r.packageCommand(command, func(_ protocol.Output, ok bool) {
		finish()
		if ok {
			delete(r.packages.updates, name)
			r.projectPackages()
		}
	})
}

func packageRepositoryUpdateSource(info protocol.PluginInfo, origin protocol.PluginOriginOnlinePayload) protocol.PluginSkillsResolved {
	source := origin.Source
	if info.UpdateSource != nil && info.UpdateSource.Kind == "online" {
		if update, valid := detail[protocol.PluginOriginOnlinePayload](info.UpdateSource.Data); valid && update.Source.Repository == source.Repository && update.Source.Commit == source.Commit && update.Path == origin.Path {
			source.GitRef = update.Source.GitRef
		}
	}
	return source
}

func (r *Runtime) installMarketPlugin(id string) {
	if r.App.Settings.Busy[settings.BusyPackage+id] {
		return
	}
	finish := r.packageWork(id)
	name := id
	if info, known := r.packages.catalogInfos[id]; known {
		name = info.Summary.Name
	}
	if current, exists := r.packages.infos[name]; exists && current.Summary.Revision != 0 {
		r.updatePackage(name, finish)
		return
	}
	entry, exists := r.packages.catalog[id]
	if !exists {
		finish()
		return
	}
	r.installCatalogEntry(entry, r.packages.catalogSources[id], 0, protocol.PluginInstallModeInstall, nil, finish)
}

func (r *Runtime) installPackageVersion(id, version string) {
	if r.App.Settings.Busy[settings.BusyPackage+id] {
		return
	}
	name := r.packageID(id)
	mode, revision := protocol.PluginInstallModeInstall, uint64(0)
	if current, installed := r.packages.infos[name]; installed {
		if !packageView(current, r.packageLocale()).Official || stringValue(current.Summary.Version) == version {
			return
		}
		mode, revision = protocol.PluginInstallModeSwitch, current.Summary.Revision
	}
	entry, known := r.packages.catalog[id]
	if !known {
		entry = protocol.PluginCatalogEntry{ID: name}
	}
	if known && r.packages.catalogSources[id] != protocol.PluginCatalogSourceOfficial {
		return
	}
	r.installCatalogEntry(entry, protocol.PluginCatalogSourceOfficial, revision, mode, &version, r.packageWork(id))
}

func officialPackageSelection(info protocol.PluginInfo) (protocol.PluginCatalogSelection, error) {
	selection := protocol.PluginCatalogSelection{Info: info}
	if info.Origin == nil {
		return selection, errors.New("official plugin has no immutable source")
	}
	switch info.Origin.Kind {
	case "online":
		if origin, valid := detail[protocol.PluginOriginOnlinePayload](info.Origin.Data); valid {
			selection.Source, selection.Path = origin.Source, origin.Path
			return selection, nil
		}
	case "bundled":
		return selection, nil
	}
	return selection, errors.New("unexpected official plugin source")
}

func (r *Runtime) installOfficialSelection(selection protocol.PluginCatalogSelection, revision uint64, mode protocol.PluginInstallMode, finish func()) {
	r.packageCommand(protocol.Command{Kind: "install_catalog_plugin", Data: protocol.CommandInstallCatalogPluginPayload{Selection: selection, ExpectedRevision: revision, Mode: mode}}, func(_ protocol.Output, ok bool) {
		if ok {
			delete(r.packages.updates, selection.Info.Summary.Name)
			r.projectPackages()
		}
		finish()
	})
}

// installCatalogEntry installs the catalog's entry; finish runs once when the
// installation ends or is not started.
func (r *Runtime) installCatalogEntry(entry protocol.PluginCatalogEntry, source protocol.PluginCatalogSource, revision uint64, mode protocol.PluginInstallMode, version *string, finish func()) {
	if source == protocol.PluginCatalogSourceOfficial {
		r.async(func() {
			selection, err := r.inspectOfficialPackage(entry.ID, version)
			if err == nil {
				err = r.ensureDesktopPackage(selection.Info)
			}
			r.enqueue(func(c *ui.Context) {
				if err != nil {
					finish()
					r.packageReport(c, err)
					return
				}
				r.installOfficialSelection(selection, revision, mode, finish)
			})
		})
		return
	}
	if entry.Compatibility.Kind != "compatible" {
		finish()
		return
	}
	r.async(func() {
		out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_catalog_plugin", Data: protocol.CommandReadCatalogPluginPayload{Source: source, ID: entry.ID}})
		value, valid := detail[protocol.PluginSkillsSource](out.Data)
		if err == nil && (!valid || out.Kind != "plugin_repository") {
			err = errors.New("unexpected plugin repository response")
		}
		if entry.Commit != nil {
			value.GitRef = entry.Commit
		}
		var selection protocol.PluginCatalogSelection
		if err == nil {
			out, err = r.controller.Do(r.ctx, protocol.Command{Kind: "inspect_plugin_source", Data: protocol.CommandInspectPluginSourcePayload{Source: value}})
			selection, valid = detail[protocol.PluginCatalogSelection](out.Data)
			if err == nil && (!valid || out.Kind != "plugin_source" || entry.Commit != nil && selection.Source.Commit != *entry.Commit) {
				err = errors.New("plugin source does not match the selected catalog entry")
			}
			if err == nil {
				err = r.ensureDesktopPackage(selection.Info)
			}
		}
		r.enqueue(func(c *ui.Context) {
			if err != nil {
				finish()
				r.packageReport(c, err)
				return
			}
			r.packages.catalogInfos[entry.ID] = selection.Info
			r.projectPackages()
			if _, installed := r.packages.infos[selection.Info.Summary.Name]; installed && revision == 0 {
				finish()
				return
			}
			r.packageCommand(protocol.Command{Kind: "install_plugin_source", Data: protocol.CommandInstallPluginSourcePayload{Source: selection.Source, Path: selection.Path, Name: selection.Info.Summary.Name, ExpectedRevision: revision}}, func(protocol.Output, bool) { finish() })
		})
	})
}
