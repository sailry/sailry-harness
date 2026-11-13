package settings

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// Acceptance is the controller-owned draft and report for explicit acceptance
// runs. Its callbacks are bound only by the live composition; preview and demo
// fixtures never start real work.
type Acceptance struct {
	Suite       string
	Model       string
	Effort      string // Empty inherits the selected model's default.
	Models      []AcceptanceModel
	Running     bool
	ReportReady bool
	Cases       []AcceptanceCase
	Run         func()
	Cancel      func()
	OpenReport  func()
}

type AcceptanceModel struct {
	ID            string
	Label         string
	Efforts       []string
	DefaultEffort string
}

type AcceptanceCase struct {
	ID     string
	Group  string
	Title  string
	Status string
	Detail string
}

func acceptance(c *ui.Context, m *Model) {
	a := &m.Acceptance
	if a.Suite == "" {
		a.Suite = "quick"
	}
	kit.Group(c, "acceptance", i18n.T(c, "acceptance_title"), i18n.T(c, "acceptance_description"), nil, func() {
		kit.Card(c, func() {
			title := i18n.T(c, "acceptance_suite")
			kit.WideRow(c, "suite", title, "", func() {
				b, _ := kit.Select(c.Key("select"), "acceptance-suite", title, &a.Suite,
					labels(c, []string{"acceptance_quick", "acceptance_backend", "acceptance_ui", "acceptance_full"}),
					a.Running, []string{"quick", "backend", "ui", "full"})
				b.FillWidth().MinWidth(0)
			})
			if a.Suite != "ui" {
				title = i18n.T(c, "acceptance_model")
				kit.WideRow(c, "model", title, "", func() {
					models := make([]kit.ModelOption, len(a.Models))
					for i, model := range a.Models {
						models[i] = kit.ModelOption{ID: model.ID, Name: model.Label}
					}
					kit.ModelSelect(c.Key("select"), kit.ModelSelectProps{
						Label: title, Placeholder: i18n.T(c, "composer_select_model"), Models: models,
						Model: a.Model, Disabled: a.Running,
						OnChange: func(id, _ string) {
							a.Model = id
							for _, model := range a.Models {
								if model.ID == id && !slices.Contains(model.Efforts, a.Effort) {
									a.Effort = ""
								}
							}
						},
					}).FillWidth().MinWidth(0)
				})
				title = i18n.T(c, "composer_effort")
				kit.WideRow(c, "effort", title, "", func() {
					values := []string{""}
					options := []string{i18n.T(c, "role_effort_inherit")}
					for _, model := range a.Models {
						if model.ID == a.Model {
							values = append(values, model.Efforts...)
							options = append(options, model.Efforts...)
						}
					}
					if !a.Running && !slices.Contains(values, a.Effort) {
						a.Effort = ""
					}
					b, _ := kit.Select(c.Key("select"), "acceptance-effort", title, &a.Effort, options,
						a.Running || len(values) == 1, values)
					b.FillWidth().MinWidth(0)
				})
			}
			kit.Row(c, "run", i18n.T(c, "acceptance_run"), "", func() {
				if a.Running {
					kit.Secondary(c.Key("stop"), i18n.T(c, "acceptance_stop")).Disabled(a.Cancel == nil).OnClick(func() {
						if a.Cancel != nil {
							a.Cancel()
						}
					})
					return
				}
				model := a.Suite == "ui"
				for _, option := range a.Models {
					model = model || option.ID == a.Model
				}
				kit.Primary(c.Key("start"), i18n.T(c, "acceptance_run"), !m.Live || m.DemoMode || a.Run == nil || !model).OnClick(func() {
					if m.Live && !m.DemoMode && a.Run != nil {
						a.Run()
					}
				})
			})
		})
	})
	if len(a.Cases) == 0 && !a.Running && !a.ReportReady {
		return
	}
	// These are the contents of a test report. General operation outcomes
	// still belong to the live adapter's toasts.
	kit.Group(c, "acceptance-report", i18n.T(c, "acceptance_results"), "", func() {
		kit.Secondary(c.Key("open"), i18n.T(c, "acceptance_report")).Disabled(!a.ReportReady || a.OpenReport == nil).OnClick(func() {
			if a.OpenReport != nil {
				a.OpenReport()
			}
		})
	}, func() {
		if a.Running {
			ui.Progress(c.Key("progress"), -1).FillWidth().Height(4).Label(i18n.T(c, "acceptance_running"))
		}
		if len(a.Cases) == 0 {
			return
		}
		kit.Card(c, func() {
			for _, result := range a.Cases {
				kit.Entry(c, result.ID, func() {
					kit.Record(c, result.ID+" · "+result.Title, result.Detail)
				}, func() {
					key, tone := acceptanceStatus(result.Status)
					kit.Badge(c, i18n.T(c, key), tone)
				}, nil).Tooltip(result.Detail)
			}
		})
	})
}

func acceptanceStatus(status string) (string, kit.Tone) {
	switch status {
	case "passed":
		return "acceptance_passed", kit.Positive
	case "failed":
		return "acceptance_failed", kit.Negative
	case "blocked":
		return "acceptance_blocked", kit.Caution
	case "skipped":
		return "acceptance_skipped", kit.Neutral
	case "canceled":
		return "acceptance_canceled", kit.Neutral
	default:
		return "acceptance_running", kit.Neutral
	}
}
