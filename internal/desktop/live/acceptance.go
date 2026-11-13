package live

import (
	"context"
	"errors"
	"slices"

	"github.com/egoist/mygo/ui"
	core "github.com/sailry/sailry-harness/internal/acceptance"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// AcceptanceRunner is supplied by cmd: live never starts Nodes or headless UI
// processes. Experimental acceptance belongs to this device's local Client,
// independently of the host currently shown by the other settings pages.
type AcceptanceRunner func(context.Context, core.Options, func(core.CaseResult)) (core.Report, core.ReportPaths, error)

type acceptanceState struct {
	runner        AcceptanceRunner
	open          func(string) error
	cancel        context.CancelFunc
	report        string
	models        map[string]protocol.SessionConfig
	modelRevision uint64
	modelsRead    bool
}

func (r *Runtime) initAcceptance() {
	a := &r.App.Settings.Acceptance
	a.Suite = "quick"
	a.Run = r.startAcceptance
	a.Cancel = func() {
		if r.acceptance.cancel != nil {
			r.acceptance.cancel()
		}
	}
	a.OpenReport = func() {
		if r.acceptance.report == "" || r.acceptance.open == nil {
			return
		}
		path, open := r.acceptance.report, r.acceptance.open
		r.async(func() {
			if err := open(path); err != nil {
				r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "acceptance_run_failed")) })
			}
		})
	}
	r.acceptanceFrame()
}

func (r *Runtime) SetAcceptanceRunner(run AcceptanceRunner, open func(string) error) {
	r.acceptance.runner, r.acceptance.open = run, open
}

func acceptanceModelID(provider protocol.ProviderID, model string) string {
	return provider.String() + "/" + model
}

func (r *Runtime) acceptanceFrame() {
	if r.parent != nil {
		return
	}
	revision := r.controller.Revision()
	if r.acceptance.modelsRead && r.acceptance.modelRevision == revision {
		return
	}
	r.acceptance.modelsRead, r.acceptance.modelRevision = true, revision
	a := &r.App.Settings.Acceptance
	a.Models = nil
	r.acceptance.models = map[string]protocol.SessionConfig{}
	snapshot := r.snapshot()
	for _, provider := range snapshot.Providers {
		if !provider.Enabled {
			continue
		}
		models := slices.Clone(provider.Models)
		if provider.DefaultModel != "" && !slices.ContainsFunc(models, func(m protocol.ConversationModel) bool { return m.ID == provider.DefaultModel }) {
			model := protocol.DefaultConversationModel()
			model.ID = provider.DefaultModel
			models = append(models, model)
		}
		for _, model := range models {
			id := acceptanceModelID(provider.ID, model.ID)
			config := protocol.DefaultSessionConfig()
			config.Provider, config.Model, config.Credential = provider.ID, model.ID, provider.Credential
			config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
			config.Effort = model.DefaultEffort
			if config.Effort.Kind == "" {
				config.Effort.Kind = "default"
			}
			r.acceptance.models[id] = config
			name := model.ID
			if model.DisplayName != nil && *model.DisplayName != "" {
				name = *model.DisplayName
			}
			choice := settings.AcceptanceModel{ID: id, Label: provider.Name + " / " + name, DefaultEffort: effortName(config.Effort)}
			for _, effort := range model.Efforts {
				choice.Efforts = append(choice.Efforts, effortName(effort))
			}
			if model.Reasoning && len(choice.Efforts) == 0 {
				choice.Efforts = slices.Clone(composer.Efforts)
			}
			if model.Reasoning && !slices.Contains(choice.Efforts, choice.DefaultEffort) {
				choice.Efforts = append(choice.Efforts, choice.DefaultEffort)
			}
			a.Models = append(a.Models, choice)
		}
	}
	if _, exists := r.acceptance.models[a.Model]; exists || a.Running {
		return
	}
	a.Model = ""
	if saved := r.Preferences.Data.Composer; saved != nil && saved.Model != nil && saved.Model.Source == r.controller.Target() {
		id := acceptanceModelID(saved.Model.Provider, saved.Model.Model)
		if _, ok := r.acceptance.models[id]; ok {
			a.Model = id
		}
	}
	if a.Model == "" && snapshot.Defaults.Config != nil {
		id := acceptanceModelID(snapshot.Defaults.Config.Provider, snapshot.Defaults.Config.Model)
		if _, ok := r.acceptance.models[id]; ok {
			a.Model = id
		}
	}
	if a.Model == "" && len(a.Models) == 1 {
		a.Model = a.Models[0].ID
	}
}

func (r *Runtime) startAcceptance() {
	m := r.App.Settings.Model
	a := &m.Acceptance
	if r.parent != nil || !m.Live || m.DemoMode || !m.TestMode || a.Running || r.acceptance.runner == nil {
		return
	}
	if !slices.Contains([]string{"quick", "backend", "ui", "full"}, a.Suite) {
		return
	}
	r.acceptanceFrame()
	options := core.Options{Suite: a.Suite}
	if a.Suite != "ui" {
		config, valid := r.acceptance.models[a.Model]
		if !valid {
			return
		}
		if a.Effort != "" {
			index := slices.IndexFunc(a.Models, func(model settings.AcceptanceModel) bool { return model.ID == a.Model })
			if index < 0 || !slices.Contains(a.Models[index].Efforts, a.Effort) {
				return
			}
			config.Effort = parseEffort(a.Effort)
		}
		options.Config = &config
	}
	ctx, cancel := context.WithCancel(r.ctx)
	r.acceptance.cancel = cancel
	a.Running, a.ReportReady, a.Cases = true, false, nil
	r.acceptance.report = ""
	runner := r.acceptance.runner
	emit := func(result core.CaseResult) {
		r.enqueue(func(*ui.Context) {
			value := settings.AcceptanceCase{ID: result.ID, Group: result.Group, Title: result.Title, Status: result.Status, Detail: result.Detail}
			if i := slices.IndexFunc(a.Cases, func(old settings.AcceptanceCase) bool { return old.ID == value.ID && old.Group == value.Group }); i >= 0 {
				a.Cases[i] = value
			} else {
				a.Cases = append(a.Cases, value)
			}
		})
	}
	if !r.async(func() {
		defer cancel()
		report, paths, err := runner(ctx, options, emit)
		canceled := ctx.Err() != nil || errors.Is(err, context.Canceled)
		r.enqueue(func(c *ui.Context) {
			a.Running = false
			r.acceptance.cancel = nil
			if paths.HTML != "" {
				r.acceptance.report, a.ReportReady = paths.HTML, true
			}
			key, outcome := "acceptance_finished", kit.Done
			switch {
			case canceled:
				key = "acceptance_run_canceled"
			case err != nil:
				key, outcome = "acceptance_run_failed", kit.Failed
			case !report.OK():
				key, outcome = "acceptance_issues", kit.Failed
			}
			kit.Toast(c, outcome, i18n.T(c, key))
		})
	}) {
		cancel()
		a.Running = false
		r.acceptance.cancel = nil
	}
}
