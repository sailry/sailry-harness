package settings

import (
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
				kit.Secondary(c.Key("authorize"), i18n.T(c, "permission_request")).OnClick(func() { notice(c) })
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
	// The speech models in the page's Installed and Available tabs, as
	// the Decision and Knowledge pages show theirs, not in the library
	// dialog.
	kit.Group(c, "models", "", "", nil, func() {
		ui.Row(c).Gap(12).Wrap().Children(func() {
			kit.Segmented(c, "tabs", &m.SpeechTab, []string{i18n.T(c, "knowledge_installed"), i18n.T(c, "local_models_available")}, kit.SegmentedProps{}).
				Label(i18n.T(c, "dictation_local_model"))
			if m.SpeechTab == 1 {
				ui.SearchField(c.Key("search"), &m.SpeechQuery).Placeholder(i18n.T(c, "search")).Width(220)
			}
		})
		q := strings.ToLower(strings.TrimSpace(m.SpeechQuery))
		var shown []int
		for i, x := range m.Speech {
			installed := x.Status == preview.ModelInstalled
			if m.SpeechTab == 0 && installed || m.SpeechTab == 1 && !installed && (q == "" || strings.Contains(strings.ToLower(x.Name+" "+x.Repo), q)) {
				shown = append(shown, i)
			}
		}
		if len(shown) == 0 {
			kit.Result(c, kit.ResultProps{Icon: icons.Cpu, Title: i18n.T(c, "local_models_empty"), Card: true})
			return
		}
		kit.Card(c, func() {
			for _, i := range shown {
				x := &m.Speech[i]
				name := x.Name
				if x.ID == "" {
					name = i18n.T(c, "dictation_model_name")
				}
				modelRow(c, x, name, i == m.SpeechModel, func() { m.SpeechModel = i }, func() { m.info = &modelInfo{name, *x} })
			}
		})
	})
	modelDetails(c, &m.info)
}

// localModels is whether it is on, the model it uses, and its Installed and
// Available tabs. Knowledge (Embedding) has no switch: enable is "" and its
// first choice, none, turns it to keywords only.
func localModels(c *ui.Context, m *Model, lm *LocalModels, title, label, enable, none string) {
	installed := lm.Installed()
	kit.Group(c, "controls", i18n.T(c, title), "", nil, func() {
		kit.Card(c, func() {
			if enable != "" {
				kit.SwitchRow(c, "enabled", i18n.T(c, enable), "", &lm.Enabled.Draft)
			}
			names := []string{i18n.T(c, none)}
			for _, x := range installed {
				names = append(names, x.Name)
			}
			t := i18n.T(c, label)
			chosen := lm.Model + 1
			kit.WideRow(c, "model", t, "", func() {
				if pick(c, "select", t, &chosen, names, !lm.Enabled.Draft).Fill().Changed() {
					lm.Model = chosen - 1
				}
			})
		})
	})
	ui.Column(c.Key("library")).Gap(16).Children(func() {
		ui.Row(c).Gap(12).Wrap().Children(func() {
			kit.Segmented(c, "tabs", &lm.Tab, []string{i18n.T(c, "knowledge_installed"), i18n.T(c, "local_models_available")}, kit.SegmentedProps{}).
				Label(i18n.T(c, title))
			if lm.Tab == 1 {
				ui.SearchField(c.Key("search"), &lm.Query).Placeholder(i18n.T(c, "search")).Width(220)
			}
			ui.Spacer(c)
			kit.Secondary(c.Key("library"), i18n.T(c, "model_library_manage")).OnClick(func() { m.library.open = true })
		})
		list := installed
		if lm.Tab == 1 {
			list = nil
			q := strings.ToLower(strings.TrimSpace(lm.Query))
			for _, x := range lm.Catalog {
				if q == "" || strings.Contains(strings.ToLower(x.Name+" "+x.Repo), q) {
					list = append(list, x)
				}
			}
		}
		if len(list) == 0 {
			kit.Result(c, kit.ResultProps{Icon: icons.Cpu, Title: i18n.T(c, "local_models_empty"), Card: true})
			return
		}
		kit.Card(c, func() {
			for i := range list {
				x := list[i]
				selected := lm.Tab == 0 && i == lm.Model
				modelRow(c, &x, x.Name, selected, func() { lm.Model = i }, func() { m.info = &modelInfo{x.Name, x} })
			}
		})
	})
	modelDetails(c, &m.info)
}

// modelRow is a managed model: its name and repository, its details, the
// install or the download's progress and cancel, and choosing it. Installing
// and canceling download models, which the preview does not.
func modelRow(c *ui.Context, x *preview.LocalModel, name string, selected bool, choose, show func()) {
	key := x.ID
	if key == "" {
		key = "default"
	}
	kit.Entry(c, key, func() { kit.Record(c, name, x.Repo) }, nil, func() {
		kit.Action(c.Key("details"), icons.Settings2, i18n.T(c, "knowledge_model_details"), false).OnClick(show)
		switch x.Status {
		case preview.ModelMissing:
			kit.Action(c.Key("install"), icons.Download, i18n.T(c, "knowledge_install"), false).OnClick(func() { notice(c) })
		case preview.ModelDownloading:
			ui.Textf(c, "%.0f%%", x.Percent).FontSize(14).Width(40).TextAlign(ui.End).TextColor(theme.Of(c).MutedForeground)
			kit.Action(c.Key("cancel"), icons.CircleX, i18n.T(c, "settings_cancel"), false).OnClick(func() { notice(c) })
		}
		if x.Status == preview.ModelInstalled {
			label := "model_library_select"
			if selected {
				label = "model_library_selected"
			}
			kit.Action(c.Key("select"), icons.Check, i18n.T(c, label), selected).OnClick(choose)
		}
	})
}

// modelInfo is the model whose details show.
type modelInfo struct {
	name  string
	model preview.LocalModel
}

// modelDetails is the details dialog of a managed model: its name, source and
// state.
func modelDetails(c *ui.Context, info **modelInfo) {
	d := *info
	if d == nil {
		return
	}
	p := theme.Of(c)
	open := true
	status := map[preview.ModelStatus]string{preview.ModelMissing: "knowledge_missing",
		preview.ModelDownloading: "knowledge_downloading", preview.ModelInstalled: "knowledge_installed"}[d.model.Status]
	kit.Dialog(c.Key("model-details"), &open, func() {
		ui.Text(c, d.name).FontSize(16).FontWeight(600)
		for _, row := range [][2]string{{i18n.T(c, "model_library_source"), d.model.Repo}, {i18n.T(c, "local_models_status"), i18n.T(c, status)}} {
			if row[1] == "" {
				continue
			}
			ui.Row(c.Key(row[0])).Gap(12).Children(func() {
				ui.Text(c, row[0]).FontSize(14).Width(120).TextColor(p.MutedForeground)
				ui.Text(c, row[1]).FontSize(14).Grow(1).Shrink(1).MinWidth(0).Selectable()
			})
		}
		ui.Row(c.Key("footer")).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("close"), i18n.T(c, "close")).OnClick(func() { open = false })
		})
	}).Width(480).Label(i18n.T(c, "knowledge_model_details"))
	if !open {
		*info = nil
	}
}
