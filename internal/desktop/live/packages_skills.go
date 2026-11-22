package live

import (
	"errors"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
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
			source := packageRepositoryUpdateSource(info, origin)
			r.installSkillSource(settings.SkillSourceDraft{Repository: source.Repository, Revision: source.GitRef, Path: origin.Path}, func(bool) { finish() })
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

func (r *Runtime) loadSkillDocument(id string, done func(string, string)) {
	name := r.packageID(id)
	if info, exists := r.packages.infos[name]; exists {
		skillName := strings.TrimPrefix(id, name+":")
		source := protocol.PluginSkillsDocumentSource{Kind: "installed", Data: protocol.PluginSkillsDocumentSourceInstalledPayload{Package: packageReference(info.Summary), Skill: skillName}}
		r.readSkillDocument(id, source, done)
		return
	}
	entry, exists := r.packages.skillCatalog[id]
	if !exists {
		done("market_skill_document_missing", "")
		return
	}
	if entry.Bundled && entry.Package != nil {
		r.readSkillDocument(id, protocol.PluginSkillsDocumentSource{Kind: "bundled", Data: protocol.PluginSkillsDocumentSourceBundledPayload{Package: *entry.Package, Skill: entry.Name}}, done)
		return
	}
	if entry.Source == nil {
		done("market_skill_document_unavailable", "")
		return
	}
	detailView := r.App.Settings.Details
	if !r.async(func() {
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
				if err == nil && (!valid || out.Kind != "skill_discovery") {
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
				if err == nil && (!valid || out.Kind != "registry_skill_discovery") {
					err = errors.New("unexpected registry skill response")
				}
				source = protocol.PluginSkillsDocumentSource{Kind: "registry", Data: value.Source}
			}
		}
		r.enqueue(func(*ui.Context) {
			if r.App.Settings.Details != detailView {
				return
			}
			if r.packageOwner() != r {
				done("market_skill_document_host_changed", "")
				return
			}
			if err != nil {
				done(skillDocumentFailure(err))
				return
			}
			if source.Kind == "" {
				done("market_skill_document_unavailable", "")
				return
			}
			r.readSkillDocument(id, source, done)
		})
	}) {
		done("resource_offline_action", "")
	}
}

func (r *Runtime) readSkillDocument(id string, source protocol.PluginSkillsDocumentSource, done func(string, string)) {
	detailView := r.App.Settings.Details
	if !r.async(func() {
		out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_skill_document", Data: protocol.CommandReadSkillDocumentPayload{Source: source}})
		document, valid := detail[protocol.PluginSkillsDocument](out.Data)
		if err == nil && (!valid || out.Kind != "skill_document") {
			err = errors.New("unexpected skill document response")
		}
		r.enqueue(func(*ui.Context) {
			if r.App.Settings.Details != detailView {
				return
			}
			if r.packageOwner() != r {
				done("market_skill_document_host_changed", "")
				return
			}
			if err != nil {
				done(skillDocumentFailure(err))
				return
			}
			body := skillBody(document.Content)
			r.packages.documents[id] = body
			r.App.Settings.SetSkillContent(id, body)
			done("", "")
		})
	}) {
		done("resource_offline_action", "")
	}
}

// skillDocumentFailure is a skill document's failure for its details: an
// offline host as such, any other host fault with the host's own reason.
func skillDocumentFailure(err error) (string, string) {
	reason := faultMessage(err)
	if reason == "resource host is offline" {
		return "resource_offline_action", ""
	}
	return "market_skill_document_failed", reason
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
