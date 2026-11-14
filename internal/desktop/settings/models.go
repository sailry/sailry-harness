package settings

import (
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// dictation is the microphone's access, the microphone and the language, and
// the speech models.
func dictation(c *ui.Context, m *Model) {
	p := theme.Of(c)
	kit.Group(c, "input", i18n.T(c, "dictation_input"), "", nil, func() {
		kit.Card(c, func() {
			kit.WideRow(c, "access", i18n.T(c, "dictation_access"), i18n.T(c, "dictation_access_hint"), func() {
				if m.MicrophoneGranted {
					ui.Text(c, i18n.T(c, "dictation_access_granted")).FontSize(14).TextColor(p.MutedForeground)
					return
				}
				// Asking the system for the microphone is the system's
				// prompt, which the preview does not raise.
				kit.Secondary(c.Key("authorize"), i18n.T(c, "permission_request")).OnClick(func() {
					m.modelAction(c, func(a *ModelActions) {
						if a.Microphone != nil {
							a.Microphone()
						}
					})
				})
			})
			mics := append([]string{i18n.T(c, "dictation_microphone_default")}, m.Microphones...)
			title := i18n.T(c, "dictation_microphone_label")
			kit.WideRow(c, "microphone", title, "", func() { pick(c, "select", title, &m.Microphone, mics, false).Fill() })
			title = i18n.T(c, "dictation_language")
			kit.WideRow(c, "language", title, "", func() {
				pick(c, "select", title, &m.SpeechLanguage, labels(c, []string{"dictation_language_auto", "dictation_language_chinese", "dictation_language_english"}), false).Fill()
			})
		})
	})
	// The speech models in the page's Installed and Available tabs, as the
	// other model pages show theirs: Available is the recommended models
	// and the hub's.
	kit.Group(c, "models", "", "", nil, func() {
		libraryBar(c, m, ModelsSpeech, i18n.T(c, "dictation_local_model"), &m.SpeechTab, &m.SpeechQuery)
		q := strings.ToLower(strings.TrimSpace(m.SpeechQuery))
		var shown []int
		repos := map[string]bool{}
		for i, x := range m.Speech {
			installed := x.Status == preview.ModelInstalled
			if !installed {
				repos[x.Repo] = true
			}
			if m.SpeechTab == 0 && installed || m.SpeechTab == 1 && !installed && (q == "" || strings.Contains(strings.ToLower(x.Name+" "+x.Repo), q)) {
				shown = append(shown, i)
			}
		}
		rows := func() {
			for _, i := range shown {
				x := &m.Speech[i]
				name := x.Name
				if x.ID == "" {
					name = i18n.T(c, "dictation_model_name")
				}
				modelRow(c, m, ModelsSpeech, x, name, i == m.SpeechModel, false, func() { m.SpeechModel = i }, func() { m.info = &modelInfo{name, ModelsSpeech, *x} })
			}
		}
		if m.SpeechTab == 1 {
			recommendedModels(c, len(shown), rows)
			hubModels(c, m, repos)
			return
		}
		if len(shown) == 0 {
			kit.Result(c, kit.ResultProps{Icon: icons.Cpu, Title: i18n.T(c, "local_models_empty"), Card: true})
			return
		}
		kit.Card(c, rows)
	})
	m.followModel(m.Speech)
	modelDetails(c, m)
}

// localModels is whether it is on, the model it uses, and its Installed and
// Available tabs. Knowledge (Embedding) has no switch: enable is "" and its
// first choice, none, turns it to keywords only.
//
// A live page is locked (Enabled.Pending) until its host's settings are
// read, while a save is under way and where the host cannot run the kind,
// which a status row then says. Its switch, picker and rows save through
// ModelActions; a chosen model the host no longer has stays the picker's
// last choice (Missing) rather than changing the setting silently.
func localModels(c *ui.Context, m *Model, lm *LocalModels, title, label, enable, none string) {
	installed := lm.Installed()
	locked := lm.Enabled.Pending
	save := func() {
		if m.Live && m.ModelActions != nil {
			m.ModelActions.Save(lm.Kind)
		}
	}
	kit.Group(c, "controls", i18n.T(c, title), "", nil, func() {
		kit.Card(c, func() {
			if enable != "" && settingSwitch(c, "enabled", i18n.T(c, enable), "", &lm.Enabled.Draft, locked) {
				save()
			}
			// The model select names a model by its place among the
			// installed ones, which is what lm.Model holds: -1 for none,
			// and the place after the last for the one that is Missing.
			models := make([]kit.ModelOption, len(installed))
			for i, x := range installed {
				models[i] = kit.ModelOption{ID: strconv.Itoa(i), Name: x.Name}
			}
			chosen := ""
			if lm.Model >= 0 {
				chosen = strconv.Itoa(lm.Model)
			}
			t := i18n.T(c, label)
			kit.WideRow(c, "model", t, "", func() {
				kit.ModelSelect(c.Key("select"), kit.ModelSelectProps{Label: t, None: i18n.T(c, none), Models: models, Model: chosen,
					Stored: lm.Missing, Disabled: !lm.Enabled.Draft || locked,
					OnChange: func(id, _ string) {
						if lm.Model = -1; id != "" {
							lm.Model, _ = strconv.Atoi(id)
						}
						save()
					}}).FillWidth().MinWidth(0)
			})
			if lm.Unavailable {
				kit.Row(c, "status", i18n.T(c, "local_models_status"), "", func() {
					kit.Badge(c, i18n.T(c, "local_models_unavailable"), kit.Neutral)
				})
			}
		})
	})
	ui.Column(c.Key("library")).Gap(16).Children(func() {
		libraryBar(c, m, lm.Kind, i18n.T(c, title), &lm.Tab, &lm.Query)
		if lm.Tab == 1 {
			// Available: the recommended models the query matches, then
			// the hub's others.
			var list []preview.LocalModel
			repos := map[string]bool{}
			q := strings.ToLower(strings.TrimSpace(lm.Query))
			for _, x := range lm.Catalog {
				repos[x.Repo] = true
				if q == "" || strings.Contains(strings.ToLower(x.Name+" "+x.Repo), q) {
					list = append(list, x)
				}
			}
			recommendedModels(c, len(list), func() {
				for i := range list {
					x := list[i]
					modelRow(c, m, lm.Kind, &x, x.Name, false, locked, func() {}, func() { m.info = &modelInfo{x.Name, lm.Kind, x} })
				}
			})
			hubModels(c, m, repos)
			return
		}
		if len(installed) == 0 {
			kit.Result(c, kit.ResultProps{Icon: icons.Cpu, Title: i18n.T(c, "local_models_empty"), Card: true})
			return
		}
		kit.Card(c, func() {
			for i := range installed {
				x := installed[i]
				modelRow(c, m, lm.Kind, &x, x.Name, i == lm.Model, locked, func() { lm.Model = i }, func() { m.info = &modelInfo{x.Name, lm.Kind, x} })
			}
		})
	})
	m.followModel(lm.Models)
	modelDetails(c, m)
}

// modelRow is a managed model: its name and repository, its details, the
// install or the download's progress and cancel, and choosing it. Installing
// and canceling download models, which the preview does not: a live row
// asks its host through ModelActions, and chooses through them too, where
// the preview's choose changes the page in memory.
func modelRow(c *ui.Context, m *Model, kind string, x *preview.LocalModel, name string, selected, locked bool, choose, show func()) {
	key := x.ID
	if key == "" {
		key = "default"
	}
	kit.Entry(c, key, func() { kit.Record(c, name, x.Repo) }, nil, func() {
		kit.Action(c.Key("details"), icons.Settings2, i18n.T(c, "knowledge_model_details"), false).OnClick(show)
		switch x.Status {
		case preview.ModelMissing:
			kit.Action(c.Key("install"), icons.Download, i18n.T(c, "knowledge_install"), false).OnClick(func() {
				m.modelAction(c, func(a *ModelActions) { a.Install(kind, x.ID) })
			})
		case preview.ModelDownloading:
			ui.Textf(c, "%.0f%%", x.Percent).FontSize(14).Width(40).TextAlign(ui.End).TextColor(theme.Of(c).MutedForeground)
			kit.Action(c.Key("cancel"), icons.CircleX, i18n.T(c, "settings_cancel"), false).OnClick(func() {
				m.modelAction(c, func(a *ModelActions) { a.Cancel(kind, x.ID) })
			})
		}
		if x.Status == preview.ModelInstalled {
			label := "model_library_select"
			if selected {
				label = "model_library_selected"
			}
			kit.Action(c.Key("select"), icons.Check, i18n.T(c, label), selected || locked).OnClick(func() {
				if !m.Live {
					choose()
				} else if m.ModelActions != nil {
					m.ModelActions.Select(kind, x.ID)
				}
			})
		}
	})
}

// modelInfo is the model whose details show, of a page's kind.
type modelInfo struct {
	name, kind string
	model      preview.LocalModel
}

// modelDetails is the details dialog of a managed model: its state, source,
// version, license and size, with its download's progress, over Close and
// what its state allows: Cancel for a download, Update for an installed
// model that can be removed, and Delete, which asks first.
func modelDetails(c *ui.Context, m *Model) {
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	if d := m.info; d != nil {
		x := &d.model
		open := true
		status := map[preview.ModelStatus]string{preview.ModelMissing: "knowledge_missing",
			preview.ModelDownloading: "knowledge_downloading", preview.ModelInstalled: "knowledge_installed"}[x.Status]
		kit.Dialog(c.Key("model-details"), &open, func() {
			ui.Text(c, d.name).FontSize(16).FontWeight(600)
			var rows [][2]string
			for _, row := range [][2]string{{t("local_models_status"), t(status)}, {t("model_library_source"), x.Repo},
				{t("model_library_version"), x.Version}, {t("settings_license"), x.License}, {t("knowledge_model_size"), x.Size}} {
				if row[1] != "" {
					rows = append(rows, row)
				}
			}
			facts(c, rows)
			if x.Status == preview.ModelDownloading {
				ui.Row(c.Key("progress")).Gap(12).AlignItems(ui.Center).Children(func() {
					ui.Progress(c, x.Percent/100).Grow(1).Height(4)
					ui.Textf(c, "%.0f%%", x.Percent).FontSize(14).TextColor(theme.Of(c).MutedForeground)
				})
			}
			ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
				kit.Secondary(c.Key("close"), t("close")).OnClick(func() { m.info = nil })
				switch {
				case x.Status == preview.ModelDownloading:
					kit.Secondary(c.Key("cancel"), t("settings_cancel")).OnClick(func() {
						m.modelAction(c, func(a *ModelActions) { a.Cancel(d.kind, x.ID) })
					})
				case x.Removable && x.Status == preview.ModelInstalled:
					kit.Secondary(c.Key("update"), t("settings_update")).OnClick(func() {
						m.modelAction(c, func(a *ModelActions) { a.Update(d.kind, x.ID) })
					})
				}
				if x.Removable && x.Status != preview.ModelDownloading {
					kit.Danger(c.Key("delete"), t("knowledge_delete")).OnClick(func() { m.dropping, m.info = d, nil })
				}
			})
		}).Width(min(480, width(c)-48)).Label(t("knowledge_model_details"))
		if !open {
			m.info = nil
		}
	}
	if d := m.dropping; d != nil {
		open := true
		// Only an embedding model's removal changes collections; the others
		// name the model, as the reference's prompt does.
		title, consequence := t("knowledge_delete"), d.name
		if d.kind == ModelsEmbedding {
			title, consequence = t("knowledge_delete"), t("knowledge_delete_model_effect", "name", d.name)
		}
		if confirm(c, "model-delete", &open, title, consequence, t("knowledge_delete")) {
			m.modelAction(c, func(a *ModelActions) { a.Delete(d.kind, d.model.ID) })
		}
		if !open {
			m.dropping = nil
		}
	}
}
