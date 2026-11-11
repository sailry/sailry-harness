package live

import (
	"errors"
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func packageCatalogSource(number int) protocol.PluginCatalogSource {
	if number == 1 {
		return protocol.PluginCatalogSourceThirdParty
	}
	return protocol.PluginCatalogSourceOfficial
}

func (r *Runtime) searchPackageMarket(kind, source int, query string) {
	if kind == 2 {
		state := r.localRuntime().packages
		if !state.themeRead {
			r.localRuntime().readThemeCatalog(false)
		}
		r.App.Settings.MarketLoad.Loading, r.App.Settings.MarketLoad.Loaded = state.themeBusy["catalog"], state.themeRead && !state.themeBusy["catalog"]
		return
	}
	owner := r.packageOwner()
	if owner == nil {
		return
	}
	state := owner.packages
	key := fmt.Sprintf("%d/%d/%s", kind, source, query)
	if key == state.marketKey {
		return
	}
	state.marketKey = key
	state.marketGeneration++
	generation := state.marketGeneration
	r.App.Settings.MarketLoad.Loading = true
	owner.async(func() {
		var entries []protocol.PluginCatalogEntry
		var skillEntries []protocol.PluginSkillcatalogEntry
		var err error
		unavailable := false
		if kind == 0 {
			for page := uint32(1); ; page++ {
				var out protocol.Output
				out, err = owner.controller.Do(owner.ctx, protocol.Command{Kind: "search_plugin_catalog", Data: protocol.CommandSearchPluginCatalogPayload{Source: packageCatalogSource(source), Query: query, Page: page}})
				if err != nil {
					break
				}
				value, valid := detail[protocol.PluginCatalogPage](out.Data)
				if !valid || out.Kind != "plugin_catalog" {
					err = errors.New("unexpected plugin catalog response")
					break
				}
				entries = append(entries, value.Entries...)
				unavailable = unavailable || value.Unavailable
				if value.Page >= value.Pages {
					break
				}
			}
		} else {
			for _, catalogSource := range []protocol.PluginSkillcatalogSource{protocol.PluginSkillcatalogSourceOfficial, protocol.PluginSkillcatalogSourceSkillsSh, protocol.PluginSkillcatalogSourceSkillhub} {
				page := uint32(1)
				for {
					var out protocol.Output
					out, err = owner.controller.Do(owner.ctx, protocol.Command{Kind: "search_skill_catalog", Data: protocol.CommandSearchSkillCatalogPayload{Source: catalogSource, Query: query, Page: page}})
					if err != nil {
						break
					}
					value, valid := detail[protocol.PluginSkillcatalogPage](out.Data)
					if !valid || out.Kind != "skill_catalog" {
						err = errors.New("unexpected skill catalog response")
						break
					}
					skillEntries = append(skillEntries, value.Entries...)
					unavailable = unavailable || value.Unavailable
					if value.Next == nil || *value.Next <= page {
						break
					}
					page = *value.Next
				}
				if err != nil {
					break
				}
			}
		}
		owner.enqueue(func(c *ui.Context) {
			if state.marketGeneration != generation {
				return
			}
			if owner.localRuntime().packageOwner() != owner {
				return
			}
			m := owner.App.Settings.Model
			m.MarketLoad.Loading, m.MarketLoad.Loaded = false, true
			if err != nil {
				owner.packageReport(c, err)
				return
			}
			if unavailable {
				kit.Toast(c, kit.Failed, i18n.T(c, "unavailable"))
			}
			if kind == 0 {
				m.MarketPlugins = nil
				for _, entry := range entries {
					if state.catalogSources[entry.ID] != packageCatalogSource(source) {
						delete(state.catalogInfos, entry.ID)
					}
					state.catalog[entry.ID] = entry
					state.catalogSources[entry.ID] = packageCatalogSource(source)
					row := catalogPackageView(entry, owner.packageLocale(), source)
					row.Host = owner.packageHostLabel()
					m.MarketPlugins = append(m.MarketPlugins, row)
				}
				owner.projectPackages()
			} else {
				m.MarketSkills = nil
				for _, entry := range skillEntries {
					state.skillCatalog[entry.ID] = entry
					row := viewmodel.Skill{ID: entry.ID, Name: entry.Name, Description: stringValue(entry.Description), Origin: entry.Origin, Content: state.documents[entry.ID]}
					if entry.DisplayName != nil {
						row.Name = *entry.DisplayName
					}
					if title := entry.DisplayLocales[i18n.Match(owner.packageLocale())]; title != "" {
						row.Name = title
					}
					if text := entry.DescriptionLocales[i18n.Match(owner.packageLocale())]; text != "" {
						row.Description = text
					}
					if entry.Package != nil {
						row.Package = *entry.Package
					}
					m.MarketSkills = append(m.MarketSkills, row)
				}
				owner.projectSkillMarket()
				owner.readMarketSkillUpdates()
			}
		})
	})
}

func catalogPackageView(entry protocol.PluginCatalogEntry, locale string, source int) viewmodel.Package {
	sourceKey := "market_details_source_official"
	if source == 1 {
		sourceKey = "market_details_source_community"
	}
	row := viewmodel.Package{ID: entry.ID, Name: entry.Name, Version: stringValue(entry.Version), Description: stringValue(entry.Description), Source: sourceKey, Official: source == 0}
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
	if entry.Compatibility.Kind == "requires_newer" {
		if requirement, ok := detail[protocol.PluginCompatibilityRequiresNewerPayload](entry.Compatibility.Data); ok {
			row.Requires = requirement.Required
		}
	}
	for _, permission := range entry.Permissions {
		row.Permissions = append(row.Permissions, packagePermissionLabels[permission])
	}
	for _, version := range entry.Versions {
		row.Versions = append(row.Versions, version.Version)
	}
	if entry.ReleaseNotes != nil {
		row.Notes = entry.ReleaseNotes.Text
		if text := entry.ReleaseNotes.Locales[i18n.Match(locale)]; text != "" {
			row.Notes = text
		}
	}
	return row
}

func (r *Runtime) loadPackageDetails(kind settings.DetailKind, id string, done func()) {
	switch kind {
	case settings.DetailTheme:
		done()
	case settings.DetailSkill:
		r.loadSkillDocument(id, done)
	default:
		name := id
		if info, known := r.packages.catalogInfos[id]; known {
			name = info.Summary.Name
		}
		if info, installed := r.packages.infos[name]; installed {
			r.readInstalledPackageDetails(info, done)
			return
		}
		entry, exists := r.packages.catalog[id]
		if !exists {
			done()
			return
		}
		r.readCatalogInformation(entry, done)
	}
}

func (r *Runtime) checkPackageUpdates(kind viewmodel.PackageKind) {
	var requests []protocol.PluginUpdatesRequest
	for name, info := range r.packages.infos {
		if packageView(info, r.packageLocale()).Kind == kind {
			requests = append(requests, protocol.PluginUpdatesRequest{Name: name, ExpectedRevision: info.Summary.Revision})
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
	r.async(func() {
		out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "check_plugin_updates", Data: protocol.CommandCheckPluginUpdatesPayload{Packages: requests, Force: true}})
		r.enqueue(func(c *ui.Context) {
			r.App.Settings.SetBusy(busy, false)
			batch, valid := detail[protocol.PluginUpdatesBatch](out.Data)
			if err == nil && (!valid || out.Kind != "plugin_updates") {
				err = errors.New("unexpected plugin updates response")
			}
			if err != nil {
				r.packageReport(c, err)
				return
			}
			found := 0
			for _, report := range batch.Reports {
				r.packages.updates[report.Current.Name] = report
				if report.Available != nil || report.Blocked != nil {
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
	if !exists || report.Available == nil && report.Blocked == nil {
		for catalogID, entry := range r.packages.catalog {
			current, installed := r.packages.infos[name]
			candidate := r.catalogEntryMetadata(entry)
			if installed && (catalogID == name || r.packages.catalogInfos[catalogID].Summary.Name == name) && plugins.IsNewer(current.Summary.Version, candidate.Version) && candidate.Compatibility.Kind == "compatible" {
				r.installCatalogEntry(entry, r.packages.catalogSources[catalogID], current.Summary.Revision, finish)
				return
			}
		}
		finish()
		return
	}
	if report.Available == nil || report.Blocked != nil {
		finish()
		return
	}
	if report.Available.Kind == "official" {
		if entry, valid := detail[protocol.PluginCatalogEntry](report.Available.Data); valid {
			r.installCatalogEntry(entry, protocol.PluginCatalogSourceOfficial, report.Current.Revision, finish)
		} else {
			finish()
		}
		return
	}
	candidate, valid := detail[protocol.PluginInfo](report.Available.Data)
	if !valid || candidate.Origin == nil {
		finish()
		return
	}
	var command protocol.Command
	switch candidate.Origin.Kind {
	case "bundled":
		command = protocol.Command{Kind: "install_bundled_plugin", Data: protocol.CommandInstallBundledPluginPayload{Name: name, ExpectedRevision: report.Current.Revision}}
	case "online":
		if origin, ok := detail[protocol.PluginOriginOnlinePayload](candidate.Origin.Data); ok {
			if candidate.Skill != nil {
				command = protocol.Command{Kind: "install_skill", Data: protocol.CommandInstallSkillPayload{Source: origin.Source, Path: origin.Path, Name: name, ExpectedRevision: report.Current.Revision}}
			} else {
				command = protocol.Command{Kind: "install_plugin_source", Data: protocol.CommandInstallPluginSourcePayload{Source: origin.Source, Path: origin.Path, Name: name, ExpectedRevision: report.Current.Revision}}
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
		}
	})
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
		if entry, known := r.packages.catalog[id]; known {
			r.installCatalogEntry(entry, r.packages.catalogSources[id], current.Summary.Revision, finish)
		} else {
			r.updatePackage(name, finish)
		}
		return
	}
	entry, exists := r.packages.catalog[id]
	if !exists {
		finish()
		return
	}
	r.installCatalogEntry(entry, packageCatalogSource(r.App.Settings.MarketSource), 0, finish)
}

// installCatalogEntry installs the catalog's entry; finish runs once when the
// installation ends or is not started.
func (r *Runtime) installCatalogEntry(entry protocol.PluginCatalogEntry, source protocol.PluginCatalogSource, revision uint64, finish func()) {
	entry = r.catalogEntryMetadata(entry)
	if entry.Compatibility.Kind == "requires_newer" {
		finish()
		return
	}
	if entry.Bundled && entry.Commit == nil {
		r.packageCommand(protocol.Command{Kind: "install_bundled_plugin", Data: protocol.CommandInstallBundledPluginPayload{Name: entry.ID, ExpectedRevision: revision}}, func(protocol.Output, bool) { finish() })
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
			if err == nil && (!valid || out.Kind != "plugin_source" || source == protocol.PluginCatalogSourceOfficial && selection.Info.Summary.Name != entry.ID || entry.Commit != nil && selection.Source.Commit != *entry.Commit) {
				err = errors.New("plugin source does not match the selected catalog entry")
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
			if current, installed := r.packages.infos[selection.Info.Summary.Name]; installed {
				if !plugins.IsNewer(current.Summary.Version, selection.Info.Summary.Version) || revision == 0 {
					finish()
					return
				}
			}
			r.packageCommand(protocol.Command{Kind: "install_plugin_source", Data: protocol.CommandInstallPluginSourcePayload{Source: selection.Source, Path: selection.Path, Name: selection.Info.Summary.Name, ExpectedRevision: revision}}, func(protocol.Output, bool) { finish() })
		})
	})
}
