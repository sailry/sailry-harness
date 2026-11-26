package shell

import (
	"context"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestMetricContributionLifecycle(t *testing.T) {
	type idKey struct{}
	type state struct {
		visible func() bool
		synced  int
		closed  int
	}
	bound := map[string]*state{}
	assembly := Assemble(func(a *Assembly) {
		a.MetricContributions = append(a.MetricContributions, MetricContribution{
			Bind: func(window WindowContext) MetricBinding {
				id, _ := window.Context.Value(idKey{}).(string)
				s := &state{visible: window.Visible}
				bound[id] = s
				return MetricBinding{
					Lead: func(_ *ui.Context, scope ComposerMetricScope) []conversation.Metric {
						if scope.Provider == "" {
							return nil
						}
						return []conversation.Metric{{ID: "lead:" + id, Value: scope.Provider}}
					},
					Statistics: func(_ *ui.Context, scope ComposerMetricScope) []conversation.Metric {
						if !scope.HasMessages {
							return nil
						}
						return []conversation.Metric{{ID: "stat:" + id, Value: "42"}}
					},
					Provider: func(_ *ui.Context, node, channel string) *settings.ChannelUsage {
						if channel != "chan-1" {
							return nil
						}
						return &settings.ChannelUsage{Lines: []settings.UsageLine{{Value: node + ":" + channel}}}
					},
					Sync:  func() { s.synced++ },
					Close: func() { s.closed++ },
				}
			},
		})
	})

	fixture := NewWithAssembly(preview.Load(), assembly)
	fixture.BindWindow(context.WithValue(t.Context(), idKey{}, "preview"))
	if len(bound) != 0 {
		t.Fatal("preview window bound live metric contribution")
	}

	first := NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, assembly)
	second := NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, assembly)
	first.BindWindow(context.WithValue(t.Context(), idKey{}, "first"))
	first.BindWindow(context.WithValue(t.Context(), idKey{}, "first-duplicate"))
	second.BindWindow(context.WithValue(t.Context(), idKey{}, "second"))

	if len(bound) != 2 || bound["first"] == nil || bound["second"] == nil {
		t.Fatalf("expected exactly two window bindings, got %+v", bound)
	}
	if !bound["first"].visible() || !bound["second"].visible() {
		t.Fatal("bound metric contributions reported hidden while open")
	}

	first.SyncNativePages()
	if bound["first"].synced != 1 || bound["second"].synced != 0 {
		t.Fatalf("sync counts = first:%d second:%d", bound["first"].synced, bound["second"].synced)
	}

	lead := first.MetricLead(nil, ComposerMetricScope{Provider: "prov-1"})
	if len(lead) != 1 || lead[0].ID != "lead:first" || lead[0].Value != "prov-1" {
		t.Fatalf("MetricLead = %+v", lead)
	}
	stats := first.MetricStatistics(nil, ComposerMetricScope{HasMessages: true})
	if len(stats) != 1 || len(stats[0]) != 1 || stats[0][0].ID != "stat:first" {
		t.Fatalf("MetricStatistics = %+v", stats)
	}
	if usage := first.ProviderUsage(nil, "node-a", "chan-1"); usage == nil || len(usage.Lines) != 1 || usage.Lines[0].Value != "node-a:chan-1" {
		t.Fatalf("ProviderUsage = %+v", usage)
	}
	if usage := first.ProviderUsage(nil, "node-a", "other"); usage != nil {
		t.Fatalf("unexpected ProviderUsage for unknown channel: %+v", usage)
	}

	first.CloseWindowBindings()
	first.CloseWindowBindings()
	if bound["first"].closed != 1 || bound["first"].visible() || bound["second"].closed != 0 {
		t.Fatalf("close did not release only first window cleanly: first=%+v second=%+v", bound["first"], bound["second"])
	}
	if len(first.MetricLead(nil, ComposerMetricScope{Provider: "prov-1"})) != 0 ||
		len(first.MetricStatistics(nil, ComposerMetricScope{HasMessages: true})) != 0 ||
		first.ProviderUsage(nil, "node-a", "chan-1") != nil {
		t.Fatal("closed window retained metric bindings")
	}
	second.CloseWindowBindings()
	if bound["second"].closed != 1 {
		t.Fatal("second window close did not run metric binding Close")
	}
}

func TestNativePageSupportsDemo(t *testing.T) {
	const demoRoute Page = "example.com/demo-aware"
	const liveRoute Page = "example.com/live-only"
	var demoVisible, liveVisible func() bool
	demoPage := NativePage{
		Page:         demoRoute,
		Feature:      Feature{ID: string(demoRoute), Label: "Demo Aware", Icon: icons.ChartBarNav, DefaultPinned: true},
		SupportsDemo: true,
		BindWindow: func(window WindowContext) WindowBinding {
			demoVisible = window.Visible
			return WindowBinding{
				Render: func(c *ui.Context) { ui.Text(c, "Demo Aware Content") },
			}
		},
	}
	livePage := NativePage{
		Page:    liveRoute,
		Feature: Feature{ID: string(liveRoute), Label: "Live Only", Icon: icons.CalendarNav, DefaultPinned: true},
		BindWindow: func(window WindowContext) WindowBinding {
			liveVisible = window.Visible
			return WindowBinding{
				Render: func(c *ui.Context) { ui.Text(c, "Live Only Content") },
			}
		},
	}

	app := NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, demoPage, livePage)
	app.BindWindow(t.Context())
	defer app.CloseWindowBindings()

	app.Settings.Model.DemoMode = true
	tester := ui.NewTesterWith(app.View, 960, 600, opaque)
	tester.SetLocale("en")
	tester.Frame()

	app.Navigate(demoRoute)
	tester.Frame()
	if !demoVisible() || !tester.HasText("Demo Aware Content") {
		t.Fatalf("SupportsDemo page did not render in Demo mode: page=%q demo=%v visible=%v", app.Page, app.DemoOn(), demoVisible())
	}

	app.Navigate(liveRoute)
	tester.Frame()
	if liveVisible() || tester.HasText("Live Only Content") {
		t.Fatal("non-demo page rendered live binding in Demo mode")
	}
}
