package composer

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/uitest"
)

func TestUnsyncedConversationPausesComposerAnimations(t *testing.T) {
	uitest.Parallel(t)
	for _, name := range []string{"indicator", "progress", "children"} {
		t.Run(name, func(t *testing.T) {
			uitest.Parallel(t)
			state := New("")
			ctx := &Context{Activity: Activity{Connected: true, Unsynced: true}}
			switch name {
			case "indicator":
				ctx.Indicators = []Indicator{{ID: "context", Loading: true, Percent: 30}}
			case "progress":
				ctx.Activity.Progress = []conversation.Step{{Text: "Inspect changes", State: conversation.StepInProgress}}
				state.act.progress = true
			case "children":
				ctx.Activity.Children = []conversation.Child{{ID: "child", Title: "Inspect", Status: conversation.ChildRunning}}
				state.act.children = true
			}
			tester := uitest.New(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				ui.Column(c).Fill().Justify(ui.End).Padding(20).Children(func() {
					if name == "indicator" {
						state.indicators(c, ctx, false)
					} else {
						state.activity(c, ctx, nil)
					}
				})
			}, 600, 400, uitest.Scale, "en", ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
			tester.Advance(time.Second)
			tester.Frame()
			tester.Frame()
			if tester.FramePending() {
				t.Fatal("an unsynced conversation still schedules activity animation")
			}
			if name == "indicator" && !ctx.Indicators[0].Loading {
				t.Fatal("presentation mutated the package indicator")
			}
			ctx.Activity.Unsynced = false
			tester.Frame()
			tester.Frame()
			if !tester.FramePending() {
				t.Fatal("valid conversation observation did not resume activity animation")
			}
		})
	}
}

func TestUnsyncedConversationKeepsStopAvailable(t *testing.T) {
	uitest.Parallel(t)
	state := New("")
	state.Running = true
	ctx := &Context{Activity: Activity{Connected: true, Unsynced: true}}
	stopped := false
	tester := uitest.Fast(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		state.sendButton(c, ctx, func() bool { return false }, func() { stopped = true })
	}, 200, 100, "en")
	button, ok := tester.AccessByRole(ui.RoleButton, i18n.In("en", "chat_stop"))
	if !ok || button.Disabled {
		t.Fatal("conversation failure disabled the reachable Host's Stop action", button)
	}
	tester.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
	tester.Frame()
	if !stopped {
		t.Fatal("Stop did not reach its action")
	}
}
