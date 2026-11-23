package composer_test

import (
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// A session's card keeps its place before there is anything to list: its
// head over one line saying nothing is there yet.
func TestOverviewCardWithNothingToList(t *testing.T) {
	o := &composer.Overview{Session: true}
	if !composer.OverviewCardShown(o) {
		t.Fatal("a session with nothing to list has no card")
	}
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		ui.Column(c).Fill().Padding(24).Children(func() {
			composer.OverviewCard(c, &composer.OverviewState{}, o, 520)
		})
	}, 400, 400)
	tt.SetLocale("en")
	tt.Frame()
	tt.Frame()
	if _, ok := tt.AccessByRole(ui.RoleComplementary, i18n.In("en", "chat_assets_title")); !ok {
		t.Fatalf("no card: %q", tt.Texts())
	}
	if !tt.HasText(i18n.In("en", "chat_assets_waiting")) {
		t.Errorf("the empty card does not say it has nothing yet: %q", tt.Texts())
	}
}

// The overview header shows the run's state and elapsed time on one quiet
// line without flickering tool-call descriptions in between, in both the
// docked card and a narrow window's popover.
func TestOverviewHeadShowsStateAndElapsed(t *testing.T) {
	const elapsed = "5s"
	state := i18n.In("en", "chat_running")
	progress := i18n.In("en", "chat_assets_progress")
	for _, view := range []struct {
		name   string
		docked bool
		width  int
	}{{"card", true, 400}, {"narrow popover", false, 360}} {
		t.Run(view.name, func(t *testing.T) {
			o := &composer.Overview{
				Session: true, Turn: true, Status: conversation.Running, Elapsed: 5 * time.Second,
				Steps: []conversation.Step{{Text: "Check the head", State: conversation.StepInProgress}},
			}
			s := &composer.OverviewState{Docked: view.docked}
			var hidden bool
			tt := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Dark)
				ui.Column(c).Fill().Padding(24).AlignItems(ui.End).Children(func() {
					if s.Docked {
						composer.OverviewCard(c, s, o, 520)
					} else {
						composer.OverviewButton(c, s, o, 32, &hidden)
					}
				})
			}, view.width, 600)
			tt.SetLocale("en")
			tt.Frame()
			if !view.docked {
				if err := tt.Click(i18n.In("en", "chat_assets_title")); err != nil {
					t.Fatal(err)
				}
				tt.Advance(200 * time.Millisecond)
			}
			tt.Frame()
			find := func(text string) ui.Rect {
				t.Helper()
				r, ok := tt.Find(text)
				if !ok {
					t.Fatalf("no %q: %q", text, tt.Texts())
				}
				return r
			}
			title, clock, heading := find(state), find(elapsed), find(progress)
			if title.H > 24 || clock.H > 24 {
				t.Errorf("the head wraps: state %v, time %v", title, clock)
			}
			if title.X+title.W > clock.X {
				t.Errorf("the state %v overlaps the time %v", title, clock)
			}
			if heading.Y <= title.Y+title.H {
				t.Errorf("the progress heading %v is not below the head %v", heading, title)
			}
		})
	}
}

// A long step is one line of the card's progress, cut short, not a block.
func TestOverviewCardStepsOneLine(t *testing.T) {
	long := strings.Repeat("Read the architecture, ownership and permission boundaries ", 4)
	o := &composer.Overview{Session: true, Steps: []conversation.Step{{Text: long, State: conversation.StepCompleted}}}
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		ui.Column(c).Fill().Padding(24).Children(func() {
			composer.OverviewCard(c, &composer.OverviewState{}, o, 520)
		})
	}, 400, 400)
	tt.SetLocale("en")
	tt.Frame()
	tt.Frame()
	found := false
	for _, e := range tt.AccessAll() {
		if strings.HasPrefix(e.Name, "Read the architecture") {
			found = true
			if e.Bounds.H > 24 {
				t.Errorf("the step wraps to %v", e.Bounds)
			}
		}
	}
	if !found {
		t.Fatalf("no step: %q", tt.Texts())
	}
}
