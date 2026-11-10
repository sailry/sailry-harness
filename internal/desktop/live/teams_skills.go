package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func skillPackages(snapshot protocol.Snapshot, assistant *protocol.PluginConversationBinding) []protocol.PluginReference {
	var packages []protocol.PluginReference
	for _, installed := range snapshot.Plugins {
		if !installed.Enabled {
			continue
		}
		if assistant != nil {
			if assistant.Package.Name == installed.Name {
				packages = append(packages, assistant.Package)
			}
			continue
		}
		packages = append(packages, protocol.PluginReference{Name: installed.Name, Digest: installed.Digest, SettingsRevision: installed.SettingsRevision})
	}
	return packages
}

func skillRows(reference protocol.PluginReference, info protocol.PluginInfo, locale string) []composer.Reference {
	if info.Summary.Name != reference.Name || info.Summary.Digest != reference.Digest || info.Summary.SettingsRevision != reference.SettingsRevision || info.Extension != nil && info.Extension.Scope == protocol.PluginScopeDesktop {
		return nil
	}
	var rows []composer.Reference
	for _, skill := range info.Skills {
		label := skill.Name
		if skill.DisplayName != nil && *skill.DisplayName != "" {
			label = *skill.DisplayName
		}
		if translated := skill.DisplayLocales[i18n.Match(locale)]; translated != "" {
			label = translated
		}
		rows = append(rows, composer.Reference{ID: reference.Name + ":" + skill.Name, Kind: composer.Skill, Label: label, Description: skill.Description, Group: "reference_group_skills", Icon: icons.BookOpen})
	}
	return rows
}

func (state *teamState) retrySkillReads(packages []protocol.PluginReference) {
	for _, reference := range packages {
		if state.skillFailures[reference] {
			delete(state.skillReads, reference)
			delete(state.skillFailures, reference)
		}
	}
}

func (r *Runtime) bindSkillReferences(cv *viewmodel.Conversation, comp *composer.State) {
	comp.SetReferencePickerOpen(func() {
		if r.teams == nil {
			return
		}
		var assistant *protocol.PluginConversationBinding
		if session, ok := r.session(cv.ID); ok {
			assistant = session.Config.Assistant
		}
		r.teams.retrySkillReads(skillPackages(r.controller.Snapshot(), assistant))
		r.skillReferences(cv)
	})
}

func (r *Runtime) skillReferences(cv *viewmodel.Conversation) []composer.Reference {
	if r.teams == nil {
		return nil
	}
	var assistant *protocol.PluginConversationBinding
	if session, ok := r.session(cv.ID); ok {
		assistant = session.Config.Assistant
	}
	var rows []composer.Reference
	for _, reference := range skillPackages(r.controller.Snapshot(), assistant) {
		if info, ok := r.teams.skills[reference]; ok {
			rows = append(rows, skillRows(reference, info, r.teams.locale)...)
			continue
		}
		if r.teams.skillReads[reference] {
			continue
		}
		r.teams.skillReads[reference] = true
		r.async(func() {
			out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_plugin_version", Data: protocol.CommandReadPluginVersionPayload{Package: reference}})
			r.enqueue(func(c *ui.Context) {
				info, valid := detail[protocol.PluginInfo](out.Data)
				if err == nil && (!valid || out.Kind != "plugin" || info.Summary.Name != reference.Name || info.Summary.Digest != reference.Digest || info.Summary.SettingsRevision != reference.SettingsRevision) {
					err = fileActionFault(protocol.ErrorCodeUnavailable, "skill catalog response is unavailable")
				}
				if err != nil {
					r.teams.skillFailures[reference] = true
					r.reportKeys(c, err, "reference_load_failed", "reference_load_failed", "reference_load_failed")
					return
				}
				delete(r.teams.skillFailures, reference)
				r.teams.skills[reference] = info
			})
		})
	}
	return rows
}
