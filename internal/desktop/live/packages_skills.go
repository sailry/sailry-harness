package live

import (
	"errors"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) installMarketSkill(id string) {
	entry, exists := r.packages.skillCatalog[id]
	if !exists || r.App.Settings.Busy[settings.BusyPackage+id] {
		return
	}
	finish := r.packageWork(id)
	if name, _, installed := r.installedCatalogSkill(entry); installed {
		r.updatePackage(name, finish)
		return
	}
	if entry.Bundled && entry.Package != nil {
		r.packageCommand(protocol.Command{Kind: "install_bundled_plugin", Data: protocol.CommandInstallBundledPluginPayload{Name: *entry.Package, ExpectedRevision: r.packages.infos[*entry.Package].Summary.Revision}}, func(protocol.Output, bool) { finish() })
		return
	}
	if entry.Source == nil {
		finish()
		return
	}
	draft := settings.SkillSourceDraft{}
	switch entry.Source.Kind {
	case "git":
		if source, valid := detail[protocol.PluginSkillsSource](entry.Source.Data); valid {
			draft.Repository, draft.Revision, draft.Path = source.Repository, stringValue(source.GitRef), stringValue(source.Path)
			r.installSkillSource(draft, func(bool) { finish() })
		} else {
			finish()
		}
	case "registry":
		source, valid := detail[protocol.PluginSkillsRegistrySource](entry.Source.Data)
		if !valid {
			finish()
			return
		}
		r.async(func() {
			out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "discover_registry_skill", Data: protocol.CommandDiscoverRegistrySkillPayload{Source: source}})
			value, valid := detail[protocol.PluginSkillsRegistryDiscovery](out.Data)
			if err == nil && (!valid || out.Kind != "registry_skill_discovery") {
				err = errors.New("unexpected registry skill response")
			}
			r.enqueue(func(c *ui.Context) {
				if err != nil {
					finish()
					r.packageReport(c, err)
					return
				}
				r.chooseDiscoveredSkills(skillInstallation{registry: &value.Source, candidates: value.Skills}, func(bool) { finish() })
			})
		})
	default:
		finish()
	}
}

func (r *Runtime) reimportSkill(id string) {
	info, exists := r.packages.infos[r.packageID(id)]
	if !exists || info.Skill == nil || info.Origin == nil || r.App.Settings.Busy[settings.BusyPackage+id] {
		return
	}
	switch info.Origin.Kind {
	case "online":
		if origin, valid := detail[protocol.PluginOriginOnlinePayload](info.Origin.Data); valid {
			finish := r.packageWork(id)
			r.installSkillSource(settings.SkillSourceDraft{Repository: origin.Source.Repository, Revision: origin.Source.GitRef, Path: origin.Path}, func(bool) { finish() })
		}
	case "registry":
		if origin, valid := detail[protocol.PluginOriginRegistryPayload](info.Origin.Data); valid {
			source := protocol.PluginSkillsRegistrySource{Namespace: &origin.Source.Namespace, Slug: origin.Source.Slug}
			r.packages.skillCatalog[id] = protocol.PluginSkillcatalogEntry{Source: &protocol.PluginSkillcatalogInstallSource{Kind: "registry", Data: source}}
			r.installMarketSkill(id)
		}
	default:
		r.chooseSkillSource(3, func(path string) {
			finish := r.packageWork(id)
			r.installSkillSource(settings.SkillSourceDraft{Source: 3, Path: path}, func(bool) { finish() })
		})
	}
}

func (r *Runtime) loadSkillDocument(id string, done func()) {
	name := r.packageID(id)
	if info, exists := r.packages.infos[name]; exists {
		skillName := strings.TrimPrefix(id, name+":")
		source := protocol.PluginSkillsDocumentSource{Kind: "installed", Data: protocol.PluginSkillsDocumentSourceInstalledPayload{Package: packageReference(info.Summary), Skill: skillName}}
		r.readSkillDocument(id, source, done)
		return
	}
	entry, exists := r.packages.skillCatalog[id]
	if !exists {
		done()
		return
	}
	if entry.Bundled && entry.Package != nil {
		r.readSkillDocument(id, protocol.PluginSkillsDocumentSource{Kind: "bundled", Data: protocol.PluginSkillsDocumentSourceBundledPayload{Package: *entry.Package, Skill: entry.Name}}, done)
		return
	}
	if entry.Source == nil {
		done()
		return
	}
	r.async(func() {
		var source protocol.PluginSkillsDocumentSource
		var err error
		if entry.Source.Kind == "git" {
			input, valid := detail[protocol.PluginSkillsSource](entry.Source.Data)
			if !valid {
				err = errors.New("invalid catalog skill source")
			} else {
				out, operationErr := r.controller.Do(r.ctx, protocol.Command{Kind: "discover_skills", Data: protocol.CommandDiscoverSkillsPayload{Source: input}})
				err = operationErr
				value, valid := detail[protocol.PluginSkillsDiscovery](out.Data)
				if err == nil && !valid {
					err = errors.New("unexpected skill discovery response")
				}
				for _, candidate := range value.Skills {
					if candidate.Skill.Name == entry.Name || candidate.Name == entry.Name {
						source = protocol.PluginSkillsDocumentSource{Kind: "git", Data: protocol.PluginSkillsDocumentSourceGitPayload{Source: value.Source, Path: candidate.Path}}
						break
					}
				}
			}
		} else if entry.Source.Kind == "registry" {
			input, valid := detail[protocol.PluginSkillsRegistrySource](entry.Source.Data)
			if !valid {
				err = errors.New("invalid registry skill source")
			} else {
				out, operationErr := r.controller.Do(r.ctx, protocol.Command{Kind: "discover_registry_skill", Data: protocol.CommandDiscoverRegistrySkillPayload{Source: input}})
				err = operationErr
				value, valid := detail[protocol.PluginSkillsRegistryDiscovery](out.Data)
				if err == nil && !valid {
					err = errors.New("unexpected registry skill response")
				}
				source = protocol.PluginSkillsDocumentSource{Kind: "registry", Data: value.Source}
			}
		}
		r.enqueue(func(c *ui.Context) {
			if err != nil || source.Kind == "" {
				if err == nil {
					err = errors.New("the catalog skill document is unavailable")
				}
				r.packageReport(c, err)
				done()
				return
			}
			r.readSkillDocument(id, source, done)
		})
	})
}

func (r *Runtime) readSkillDocument(id string, source protocol.PluginSkillsDocumentSource, done func()) {
	r.async(func() {
		out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_skill_document", Data: protocol.CommandReadSkillDocumentPayload{Source: source}})
		document, valid := detail[protocol.PluginSkillsDocument](out.Data)
		if err == nil && (!valid || out.Kind != "skill_document") {
			err = errors.New("unexpected skill document response")
		}
		r.enqueue(func(c *ui.Context) {
			if err == nil {
				body := skillBody(document.Content)
				r.packages.documents[id] = body
				for _, list := range [][]viewmodel.Skill{r.App.Settings.SkillList, r.App.Settings.MarketSkills} {
					if index := slices.IndexFunc(list, func(row viewmodel.Skill) bool { return row.ID == id }); index >= 0 {
						list[index].Content = body
					}
				}
			}
			r.packageReport(c, err)
			done()
		})
	})
}

// skillBody is a skill document without its YAML frontmatter: the name,
// description and display names it declares are already shown above the body.
func skillBody(content string) string {
	rest, found := strings.CutPrefix(strings.TrimLeft(content, "\ufeff \t\r\n"), "---")
	if !found {
		return content
	}
	for _, end := range []string{"\n---", "\n..."} {
		if index := strings.Index(rest, end); index >= 0 {
			if line := strings.IndexByte(rest[index+1:], '\n'); line >= 0 {
				return strings.TrimLeft(rest[index+1+line:], "\r\n")
			}
			return ""
		}
	}
	return content
}
