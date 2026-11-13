package live

import (
	"context"
	"path/filepath"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	core "github.com/sailry/sailry-harness/internal/acceptance"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAcceptanceUsesSelectedModelEffort(t *testing.T) {
	for _, test := range []struct {
		name, selected, expected string
	}{
		{"model default", "", "high"},
		{"explicit medium", "medium", "medium"},
		{"unsupported", "max", ""},
	} {
		t.Run(test.name, func(t *testing.T) {
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node = protocol.NodeID{1}
			model := protocol.DefaultConversationModel()
			model.ID, model.Reasoning = "model-fixture", true
			model.DefaultEffort = protocol.Effort{Kind: "high"}
			model.Efforts = []protocol.Effort{{Kind: "medium"}, {Kind: "high"}}
			provider := protocol.DefaultConversationProvider()
			provider.ID, provider.Enabled, provider.Models = protocol.ProviderID{2}, true, []protocol.ConversationModel{model}
			snapshot.Providers = []protocol.ConversationProvider{provider}
			controller := client.New(&conversationTestTransport{snapshot: snapshot})
			defer controller.Close()
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			settingsModel := &settings.Model{Live: true, TestMode: true}
			runtime := &Runtime{
				App: &shell.App{Settings: &settings.State{Model: settingsModel}},
				ctx: t.Context(), controller: controller,
				Preferences: preferences.Open(filepath.Join(t.TempDir(), "preferences.json")),
			}
			runtime.initAcceptance()
			a := &settingsModel.Acceptance
			if len(a.Models) != 1 || a.Model != acceptanceModelID(provider.ID, model.ID) || !slices.Equal(a.Models[0].Efforts, []string{"medium", "high"}) {
				t.Fatalf("acceptance did not project the configured model: %+v", a.Models)
			}
			a.Effort = test.selected
			var captured *protocol.SessionConfig
			runtime.SetAcceptanceRunner(func(_ context.Context, options core.Options, _ func(core.CaseResult)) (core.Report, core.ReportPaths, error) {
				captured = options.Config
				return core.Report{Version: 1, Cases: []core.CaseResult{{ID: "fixture", Status: "passed"}}}, core.ReportPaths{}, nil
			}, nil)
			runtime.startAcceptance()
			runtime.workers.Wait()
			if test.expected == "" {
				if captured != nil || a.Running {
					t.Fatal("an unsupported effort started acceptance")
				}
				return
			}
			if captured == nil || captured.Model != model.ID || captured.Effort.Kind != test.expected {
				t.Fatalf("acceptance received a different model or effort: %+v", captured)
			}
		})
	}
}

func TestAcceptanceRequiresExplicitLiveMode(t *testing.T) {
	for _, guard := range []string{"disabled", "demo", "preview", "running", "remote"} {
		t.Run(guard, func(t *testing.T) {
			model := &settings.Model{Live: true, TestMode: true}
			model.Acceptance.Suite = "ui"
			runtime := &Runtime{App: &shell.App{Settings: &settings.State{Model: model}}}
			called := false
			runtime.SetAcceptanceRunner(func(context.Context, core.Options, func(core.CaseResult)) (core.Report, core.ReportPaths, error) {
				called = true
				return core.Report{}, core.ReportPaths{}, nil
			}, nil)
			switch guard {
			case "disabled":
				model.TestMode = false
			case "demo":
				model.DemoMode = true
			case "preview":
				model.Live = false
			case "running":
				model.Acceptance.Running = true
			case "remote":
				runtime.parent = &Runtime{}
			}
			runtime.startAcceptance()
			if called || runtime.acceptance.cancel != nil {
				t.Fatal("a guarded action started acceptance")
			}
		})
	}
}

func TestAcceptanceCancellationKeepsItsReport(t *testing.T) {
	controller := client.New(&conversationTestTransport{snapshot: protocol.DefaultSnapshot()})
	defer controller.Close()
	model := &settings.Model{Live: true, TestMode: true}
	runtime := &Runtime{
		App: &shell.App{Settings: &settings.State{Model: model}},
		ctx: t.Context(), controller: controller,
		Preferences: preferences.Open(filepath.Join(t.TempDir(), "preferences.json")),
	}
	runtime.initAcceptance()
	model.Acceptance.Suite = "ui"
	started := make(chan struct{})
	count := 0
	directory := filepath.Join(t.TempDir(), "report")
	runtime.SetAcceptanceRunner(func(ctx context.Context, options core.Options, emit func(core.CaseResult)) (core.Report, core.ReportPaths, error) {
		count++
		if options.Suite != "ui" || options.Config != nil {
			t.Error("fixture-only acceptance received a model configuration")
		}
		close(started)
		result := core.CaseResult{ID: "fixture", Group: "ui", Status: "passed"}
		emit(result)
		<-ctx.Done()
		result.Status = "canceled"
		emit(result)
		report := core.Report{Version: 1, Suite: "ui", Cases: []core.CaseResult{result}}
		paths, err := core.WriteReport(directory, report)
		if err != nil {
			return report, paths, err
		}
		return report, paths, ctx.Err()
	}, nil)
	runtime.startAcceptance()
	<-started
	runtime.startAcceptance()
	model.Acceptance.Cancel()
	runtime.workers.Wait()
	if count != 1 || !model.Acceptance.Running {
		t.Fatal("acceptance overlapped or modified presentation off the UI thread")
	}
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		runtime.mu.Lock()
		queue := runtime.queue
		runtime.queue = nil
		runtime.mu.Unlock()
		for _, update := range queue {
			update(c)
		}
		kit.ToastHost(c)
	}, 640, 480)
	tester.SetLocale("en")
	tester.Frame()
	if model.Acceptance.Running || !model.Acceptance.ReportReady || len(model.Acceptance.Cases) != 1 || model.Acceptance.Cases[0].Status != "canceled" {
		t.Fatalf("canceled acceptance lost or duplicated its report: %+v", model.Acceptance)
	}
}
