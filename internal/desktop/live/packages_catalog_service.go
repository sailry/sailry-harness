package live

import (
	"context"
	"errors"

	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) packageCatalogService() *settings.PackageCatalogService {
	return &settings.PackageCatalogService{
		Node: r.selectedPackageCatalog, ThemeCatalog: r.themeCatalogView, InstallTheme: r.installMarketTheme,
		Context: r.ctx, Run: r.async, Enqueue: r.enqueue,
	}
}

func (r *Runtime) selectedPackageCatalog() *settings.PackageCatalogNode {
	owner := r.packageOwner()
	if owner == nil {
		return nil
	}
	state := owner.packages
	if state.catalogNode == nil {
		state.catalogNode = &settings.PackageCatalogNode{
			Context: owner.ctx, Run: owner.async, Revision: func() uint64 { return state.catalogProjection },
			Locale: owner.packageLocale, SearchPlugins: owner.searchPluginCatalog, SearchSkills: owner.searchSkillCatalog,
			RememberPlugins: owner.rememberCatalogPlugins, RememberSkills: owner.rememberCatalogSkills,
			ProjectPlugins: owner.projectCatalogPlugins, ProjectSkills: owner.projectCatalogSkills,
			InstallPlugin: owner.installMarketPlugin, InstallSkill: owner.installMarketSkill,
		}
	}
	return state.catalogNode
}

func (r *Runtime) searchPluginCatalog(ctx context.Context, query protocol.CommandSearchPluginCatalogPayload) (protocol.PluginCatalogPage, error) {
	out, err := r.controller.Do(ctx, protocol.Command{Kind: "search_plugin_catalog", Data: query})
	page, valid := detail[protocol.PluginCatalogPage](out.Data)
	if err == nil && (!valid || out.Kind != "plugin_catalog" || page.Page != query.Page) {
		err = errors.New("unexpected plugin catalog response")
	}
	return page, err
}

func (r *Runtime) searchSkillCatalog(ctx context.Context, query protocol.CommandSearchSkillCatalogPayload) (protocol.PluginSkillcatalogPage, error) {
	out, err := r.controller.Do(ctx, protocol.Command{Kind: "search_skill_catalog", Data: query})
	page, valid := detail[protocol.PluginSkillcatalogPage](out.Data)
	if err == nil && (!valid || out.Kind != "skill_catalog" || page.Page != query.Page || page.Next != nil && *page.Next <= query.Page) {
		err = errors.New("unexpected skill catalog response")
	}
	return page, err
}

func (r *Runtime) rememberCatalogPlugins(source protocol.PluginCatalogSource, entries []protocol.PluginCatalogEntry) {
	state := r.packages
	for _, entry := range entries {
		state.catalogRevisions[entry.ID]++
		if state.catalogSources[entry.ID] != source {
			delete(state.catalogInfos, entry.ID)
		}
		state.catalog[entry.ID], state.catalogSources[entry.ID] = entry, source
	}
	if len(entries) > 0 {
		r.projectPackages()
	}
}

func (r *Runtime) rememberCatalogSkills(entries []protocol.PluginSkillcatalogEntry) {
	for _, entry := range entries {
		r.packages.skillCatalog[entry.ID] = entry
	}
}

func (r *Runtime) projectCatalogPlugins(rows []viewmodel.Package) {
	var entries []protocol.PluginCatalogEntry
	for _, row := range rows {
		if entry, known := r.packages.catalog[row.ID]; known {
			entries = append(entries, entry)
		}
	}
	r.readInstalledCatalogCandidates(entries)
	r.projectCatalogPluginRows(rows)
	var names []string
	for _, row := range rows {
		if row.Installed {
			names = append(names, row.InstallName)
		}
	}
	r.readPackageUpdates(names)
}

func (r *Runtime) projectCatalogSkills(rows []viewmodel.Skill) {
	var names []string
	for i := range rows {
		row := &rows[i]
		entry, known := r.packages.skillCatalog[row.ID]
		if !known {
			continue
		}
		name, info, installed := r.installedCatalogSkill(entry)
		row.Installed, row.Update, row.Content = installed, false, r.packages.documents[row.ID]
		if report, exists := r.packages.updates[name]; installed && exists {
			row.Update = !info.Summary.Pinned && samePackageVersion(info.Summary, report.Current) && packageUpdateAvailable(report)
		}
		if installed && info.Skill != nil {
			names = append(names, name)
		}
	}
	r.readPackageUpdates(names)
}
