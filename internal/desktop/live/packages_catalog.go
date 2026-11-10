package live

import (
	"errors"
	"net/url"
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func packageReleaseNotes(notes *protocol.PluginCatalogReleaseNotes, locale string) string {
	if notes == nil {
		return ""
	}
	if translated := notes.Locales[i18n.Match(locale)]; translated != "" {
		return translated
	}
	return notes.Text
}

func applyPackageCatalog(row *viewmodel.Package, entry protocol.PluginCatalogEntry, locale string) {
	if plugins.IsNewer(&row.Version, entry.Version) {
		row.Latest = stringValue(entry.Version)
		if entry.Compatibility.Kind == "requires_newer" {
			if requirement, valid := detail[protocol.PluginCompatibilityRequiresNewerPayload](entry.Compatibility.Data); valid {
				row.Requires = requirement.Required
			}
		}
	}
	if len(entry.Versions) != 0 {
		row.Versions = nil
		for _, version := range entry.Versions {
			row.Versions = append(row.Versions, version.Version)
		}
	}
	if row.Installed && row.Version != "" && !slices.Contains(row.Versions, row.Version) {
		row.Versions = append(row.Versions, row.Version)
	}
	if entry.ReleaseNotes != nil {
		row.Notes = packageReleaseNotes(entry.ReleaseNotes, locale)
	}
}

func (r *Runtime) catalogEntryMetadata(entry protocol.PluginCatalogEntry) protocol.PluginCatalogEntry {
	if info, known := r.packages.catalogInfos[entry.ID]; known {
		entry.Version = info.Summary.Version
		entry.Compatibility = info.Summary.Compatibility
		if info.ReleaseNotes != nil {
			entry.ReleaseNotes = info.ReleaseNotes
		}
		if len(info.Versions) != 0 {
			entry.Versions = info.Versions
		}
	}
	return entry
}

func (r *Runtime) projectPluginMarket() {
	if r.localRuntime().packageOwner() != r {
		return
	}
	for index := range r.App.Settings.MarketPlugins {
		row := &r.App.Settings.MarketPlugins[index]
		entry, known := r.packages.catalog[row.ID]
		if !known {
			continue
		}
		id, name := row.ID, row.ID
		if info, known := r.packages.catalogInfos[id]; known {
			name = info.Summary.Name
			*row = packageView(info, r.packageLocale())
		}
		if current, installed := r.packages.infos[name]; installed {
			*row = packageView(current, r.packageLocale())
		} else {
			row.Installed = false
		}
		source := 0
		if r.packages.catalogSources[id] == protocol.PluginCatalogSourceThirdParty {
			source = 1
		}
		row.ID, row.InstallName, row.Source = id, name, catalogPackageView(entry, r.packageLocale(), source).Source
		row.Host = r.packageHostLabel()
		applyPackageCatalog(row, r.catalogEntryMetadata(entry), r.packageLocale())
	}
}

func (r *Runtime) readInstalledCatalogCandidates(entries []protocol.PluginCatalogEntry) {
	for _, entry := range entries {
		if entry.Repository == nil || r.packages.catalogSources[entry.ID] != protocol.PluginCatalogSourceThirdParty {
			continue
		}
		selected, err := plugins.ParseRepositorySource(protocol.PluginSkillsSource{Repository: *entry.Repository})
		if err != nil {
			continue
		}
		for _, info := range r.packages.infos {
			if info.Origin == nil || info.Origin.Kind != "online" {
				continue
			}
			origin, valid := detail[protocol.PluginOriginOnlinePayload](info.Origin.Data)
			if valid && origin.Source.Repository == selected.Repository {
				r.readCatalogInformation(entry, nil)
				break
			}
		}
	}
}

func (r *Runtime) readCatalogInformation(entry protocol.PluginCatalogEntry, done func()) {
	generation := r.packages.marketGeneration
	source := r.packages.catalogSources[entry.ID]
	key := "catalog/" + string(source) + "/" + entry.ID + "/" + strconv.FormatUint(generation, 10)
	if r.packages.reads[key] {
		if done != nil {
			r.packages.catalogWaiters[key] = append(r.packages.catalogWaiters[key], done)
		}
		return
	}
	if r.packages.failures[key] == strconv.FormatUint(generation, 10) {
		if done != nil {
			done()
		}
		return
	}
	if _, known := r.packages.catalogInfos[entry.ID]; known {
		if done != nil {
			done()
		}
		return
	}
	r.packages.reads[key] = true
	if done != nil {
		r.packages.catalogWaiters[key] = append(r.packages.catalogWaiters[key], done)
	}
	r.async(func() {
		out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_catalog_plugin_info", Data: protocol.CommandReadCatalogPluginInfoPayload{Source: source, ID: entry.ID, Bundled: entry.Bundled && entry.Commit == nil}})
		value, valid := detail[protocol.PluginCatalogDetails](out.Data)
		info, validInfo := detail[protocol.PluginInfo](value.Data)
		if err == nil && (!valid || out.Kind != "catalog_details" || value.Kind != "plugin" || !validInfo) {
			err = errors.New("unexpected catalog details response")
		}
		r.enqueue(func(c *ui.Context) {
			delete(r.packages.reads, key)
			waiters := r.packages.catalogWaiters[key]
			delete(r.packages.catalogWaiters, key)
			if r.packages.marketGeneration == generation {
				if err == nil {
					r.packages.catalogInfos[entry.ID] = info
					delete(r.packages.failures, key)
					r.projectPackages()
				} else {
					r.packages.failures[key] = strconv.FormatUint(generation, 10)
				}
				r.packageReport(c, err)
			}
			for _, waiter := range waiters {
				waiter()
			}
		})
	})
}

func (r *Runtime) readInstalledPackageDetails(info protocol.PluginInfo, done func()) {
	reference := packageReference(info.Summary)
	version := packageVersion(info.Summary)
	source, catalog := r.packages.catalogSources[info.Summary.Name]
	if info.Origin != nil && info.Origin.Kind == "bundled" {
		source, catalog = protocol.PluginCatalogSourceOfficial, true
	}
	if info.Origin != nil && info.Origin.Kind == "online" {
		if origin, valid := detail[protocol.PluginOriginOnlinePayload](info.Origin.Data); valid && strings.TrimSuffix(origin.Source.Repository, ".git") == plugins.OfficialRepository {
			source, catalog = protocol.PluginCatalogSourceOfficial, true
		}
	}
	r.async(func() {
		out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_plugin_version", Data: protocol.CommandReadPluginVersionPayload{Package: reference}})
		captured, valid := detail[protocol.PluginInfo](out.Data)
		if err == nil && (!valid || out.Kind != "plugin" || captured.Summary.Digest != reference.Digest) {
			err = errors.New("unexpected immutable plugin response")
		}
		var entry *protocol.PluginCatalogEntry
		var catalogErr error
		if err == nil && catalog {
			out, catalogErr = r.controller.Do(r.ctx, protocol.Command{Kind: "search_plugin_catalog", Data: protocol.CommandSearchPluginCatalogPayload{Source: source, Query: info.Summary.Name, Page: 1}})
			page, valid := detail[protocol.PluginCatalogPage](out.Data)
			if catalogErr == nil && (!valid || out.Kind != "plugin_catalog") {
				catalogErr = errors.New("unexpected plugin catalog response")
			}
			if catalogErr == nil {
				for _, candidate := range page.Entries {
					if candidate.ID == info.Summary.Name {
						entry = &candidate
						break
					}
				}
			}
		}
		r.enqueue(func(c *ui.Context) {
			if err == nil && packageVersion(r.packages.infos[info.Summary.Name].Summary) == version {
				captured.Summary, captured.Origin, captured.Skill = info.Summary, info.Origin, info.Skill
				r.packages.infos[info.Summary.Name] = captured
				if entry != nil {
					r.packages.catalog[entry.ID], r.packages.catalogSources[entry.ID] = *entry, source
				}
				r.projectPackages()
			}
			r.packageReport(c, errors.Join(err, catalogErr))
			done()
		})
	})
}

func (r *Runtime) installedCatalogSkill(entry protocol.PluginSkillcatalogEntry) (string, protocol.PluginInfo, bool) {
	for name, info := range r.packages.infos {
		if !slices.ContainsFunc(info.Skills, func(skill protocol.PluginSkill) bool { return skill.Name == entry.Name }) {
			continue
		}
		if entry.Package != nil && *entry.Package == name {
			return name, info, true
		}
		if info.Skill == nil || info.Origin == nil || entry.Source == nil {
			continue
		}
		if entry.Source.Kind == "registry" && info.Origin.Kind == "registry" {
			wanted, valid := detail[protocol.PluginSkillsRegistrySource](entry.Source.Data)
			origin, installed := detail[protocol.PluginOriginRegistryPayload](info.Origin.Data)
			if valid && installed && wanted.Namespace != nil && *wanted.Namespace == origin.Source.Namespace && wanted.Slug == origin.Source.Slug {
				return name, info, true
			}
		}
		if entry.Source.Kind == "git" && info.Origin.Kind == "online" {
			wanted, valid := detail[protocol.PluginSkillsSource](entry.Source.Data)
			origin, installed := detail[protocol.PluginOriginOnlinePayload](info.Origin.Data)
			if !valid || !installed {
				continue
			}
			selectedSkill := ""
			if address, err := url.Parse(wanted.Repository); err == nil && address.Hostname() == "skills.sh" {
				parts := strings.Split(strings.Trim(address.Path, "/"), "/")
				if len(parts) == 3 {
					wanted.Repository, selectedSkill = "https://github.com/"+parts[0]+"/"+parts[1], parts[2]
				}
			}
			selected, err := plugins.ParseRepositorySource(wanted)
			if err == nil && selected.Repository == origin.Source.Repository && (selected.Path == origin.Path || selectedSkill == entry.Name) {
				return name, info, true
			}
		}
	}
	return "", protocol.PluginInfo{}, false
}

func (r *Runtime) projectSkillMarket() {
	if r.localRuntime().packageOwner() != r {
		return
	}
	for index := range r.App.Settings.MarketSkills {
		row := &r.App.Settings.MarketSkills[index]
		entry, known := r.packages.skillCatalog[row.ID]
		name, _, installed := r.installedCatalogSkill(entry)
		row.Installed, row.Update = known && installed, false
		if report, exists := r.packages.updates[name]; row.Installed && exists {
			row.Update = report.Available != nil && report.Blocked == nil
		}
	}
}

func (r *Runtime) readMarketSkillUpdates() {
	var requests []protocol.PluginUpdatesRequest
	for _, entry := range r.packages.skillCatalog {
		name, info, installed := r.installedCatalogSkill(entry)
		if !installed || info.Skill == nil || r.packages.skillChecks[packageVersion(info.Summary)] {
			continue
		}
		r.packages.skillChecks[packageVersion(info.Summary)] = true
		requests = append(requests, protocol.PluginUpdatesRequest{Name: name, ExpectedRevision: info.Summary.Revision})
	}
	if len(requests) == 0 {
		return
	}
	r.async(func() {
		out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "check_plugin_updates", Data: protocol.CommandCheckPluginUpdatesPayload{Packages: requests}})
		batch, valid := detail[protocol.PluginUpdatesBatch](out.Data)
		if err == nil && (!valid || out.Kind != "plugin_updates") {
			err = errors.New("unexpected plugin updates response")
		}
		r.enqueue(func(c *ui.Context) {
			if err == nil {
				for _, report := range batch.Reports {
					r.packages.updates[report.Current.Name] = report
				}
				r.projectPackages()
			}
			r.packageReport(c, err)
		})
	})
}
