package composer_test

import (
	"strings"
	"testing"

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
