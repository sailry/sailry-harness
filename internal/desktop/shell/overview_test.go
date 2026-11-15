package shell

import (
	"math"
	"slices"
	"strconv"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func overviewLayoutFixture(t *testing.T, width int) (*App, *ui.Tester) {
	t.Helper()
	cv := &viewmodel.Conversation{
		ID: "overview-layout", Title: "Overview layout",
		Turns: []conversation.Turn{{ID: "turn", User: "Fixture input", Reply: conversation.Reply{Status: conversation.Completed}}},
	}
	a := NewLive(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{cv}, Workspace: &viewmodel.Workspace{}})
	a.Sidebar.Selected = cv.ID
	a.NavOpen = false
	a.Hooks.Overview = func(*ui.Context, *viewmodel.Conversation) *composer.Overview {
		return &composer.Overview{Session: true, Output: []composer.OverviewAsset{{ID: "output", Name: "Fixture output"}}}
	}
	tt := ui.NewTesterWith(a.View, width, 820, opaque)
	tt.SetLocale("en")
	settleOverviewLayout(t, tt)
	return a, tt
}

func settleOverviewLayout(t *testing.T, tt *ui.Tester) {
	t.Helper()
	for range 4 {
		tt.Frame()
		tt.Advance(300 * time.Millisecond)
	}
	tt.Frame()
	if tt.FramePending() {
		t.Fatal("the settled overview keeps requesting frames")
	}
}

func overviewLayout(t *testing.T, tt *ui.Tester) (ui.Rect, ui.Rect) {
	t.Helper()
	log, found := tt.AccessByRole(ui.RoleLog, "")
	if !found || log.Bounds.W <= 0 {
		t.Fatalf("the conversation has no transcript: %q", tt.Texts())
	}
	input, found := tt.AccessByRole(ui.RoleTextField, i18n.In("en", "composer"))
	if !found || input.Bounds.W <= 0 {
		t.Fatalf("the conversation has no composer: %q", tt.Texts())
	}
	return log.Bounds, input.Bounds
}

func overviewLayoutCard(t *testing.T, tt *ui.Tester) ui.Rect {
	t.Helper()
	card, found := tt.AccessByRole(ui.RoleComplementary, i18n.In("en", "chat_assets_title"))
	if !found {
		t.Fatalf("the conversation has no overview card: %q", tt.Texts())
	}
	return card.Bounds
}

func overviewHasNoCollapse(t *testing.T, tt *ui.Tester) {
	t.Helper()
	if _, found := headerButton(tt, i18n.In("en", "chat_assets_title")); found {
		t.Fatal("the wide conversation keeps an overview button in the header")
	}
	for _, entry := range tt.AccessAll() {
		if entry.Name == i18n.In("en", "chat_assets_fold") || entry.Name == i18n.In("en", "chat_assets_unfold") {
			t.Fatalf("the width-adapted overview has a manual collapse control: %+v", entry)
		}
	}
}

func clickOverviewHeader(t *testing.T, tt *ui.Tester) {
	t.Helper()
	button, found := headerButton(tt, i18n.In("en", "chat_assets_title"))
	if !found {
		t.Fatalf("the header has no overview button: %q", tt.Texts())
	}
	tt.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
	settleOverviewLayout(t, tt)
}

// The activity column reserves the same width at medium and wide sizes;
// it follows the available width without a manual visibility control.
func TestOverviewDocksWhereItFits(t *testing.T) {
	for _, width := range []int{1500, 2100} {
		t.Run(strconv.Itoa(width), func(t *testing.T) {
			a, tt := overviewLayoutFixture(t, width)
			log, input := overviewLayout(t, tt)
			card := overviewLayoutCard(t, tt)
			if math.Abs(float64(a.mainLayoutWidth-log.W)) > 0.5 || math.Abs(float64(card.W-composer.OverviewWidth)) > 0.5 || math.Abs(float64(log.X+log.W-float32(width))) > 0.5 {
				t.Fatalf("main width %v has scroll viewport %v and card %v; the viewport must reach the window's right edge", a.mainLayoutWidth, log, card)
			}
			contentRight := log.X + log.W - overviewSide
			if math.Abs(float64(log.X+(log.W-overviewSide)/2-input.X-input.W/2)) > 0.5 {
				t.Fatalf("the composer %v is not centered beside the activity card in viewport %v", input, log)
			}
			if input.X+input.W > contentRight || card.X < contentRight || card.X+card.W > log.X+log.W || card.H > overviewCardMax+8 {
				t.Fatalf("the card %v overlaps the content or lies outside the viewport %v; composer %v", card, log, input)
			}
			if !tt.HasText("Fixture output") || tt.HasText(i18n.In("en", "chat_assets_waiting")) {
				t.Fatalf("the card does not show its inventory: %q", tt.Texts())
			}
			overviewHasNoCollapse(t, tt)
			snapshot(t, tt, "overview-column-"+strconv.Itoa(width))
		})
	}
}

func TestOverviewPopoverAndDirectResize(t *testing.T) {
	a, tt := overviewLayoutFixture(t, 1100)
	var notifications []bool
	a.Hooks.Overview = func(*ui.Context, *viewmodel.Conversation) *composer.Overview {
		return &composer.Overview{
			Session: true, Output: []composer.OverviewAsset{{ID: "output", Name: "Fixture output"}},
			Opened: func(open bool) { notifications = append(notifications, open) },
		}
	}
	tt.Frame()
	log, input := overviewLayout(t, tt)
	if _, found := tt.AccessByRole(ui.RoleComplementary, i18n.In("en", "chat_assets_title")); found {
		t.Fatal("the narrow conversation keeps an overview column")
	}
	if math.Abs(float64(log.W-a.mainLayoutWidth)) > 0.5 {
		t.Fatalf("the narrow transcript %v does not fill its main column %v", log, a.mainLayoutWidth)
	}
	if math.Abs(float64(log.X+log.W/2-input.X-input.W/2)) > 0.5 {
		t.Fatalf("the narrow composer %v is not centered in its viewport %v", input, log)
	}
	clickOverviewHeader(t, tt)
	if !a.overviews["overview-layout"].state.Open() || !tt.HasText("Fixture output") {
		t.Fatalf("the first click did not retain the overview popover: %q", tt.Texts())
	}
	opened, openedInput := overviewLayout(t, tt)
	if opened != log || openedInput != input {
		t.Fatalf("opening the popover moved the transcript: %v, %v", opened, openedInput)
	}
	clickOverviewHeader(t, tt)
	if a.overviews["overview-layout"].state.Open() {
		t.Fatal("the second click did not close the popover")
	}
	clickOverviewHeader(t, tt)
	if !a.overviews["overview-layout"].state.Open() {
		t.Fatal("the closed popover could not reopen")
	}
	if !slices.Equal(notifications, []bool{true, false, true}) {
		t.Fatalf("opening and closing the popover notified its owner %v", notifications)
	}

	// A window resize lays out at its destination immediately. Advancing past
	// the former 200 ms aside animation must not move either column again.
	for _, width := range []int{1500, 2100, 1500, 1100} {
		tt.SetSize(width, 820)
		tt.Frame()
		immediate, immediateInput := overviewLayout(t, tt)
		reserved := float32(0)
		if width >= 1500 {
			reserved = overviewSide
			card := overviewLayoutCard(t, tt)
			overviewHasNoCollapse(t, tt)
			if card.X < immediate.X+immediate.W-reserved {
				t.Fatalf("resizing to %d overlapped the overview and transcript", width)
			}
		}
		if math.Abs(float64(a.mainLayoutWidth-immediate.W)) > 0.5 || math.Abs(float64(immediate.X+immediate.W-float32(width))) > 0.5 {
			t.Fatalf("resizing to %d left a narrowed scroll viewport %v; main width %v", width, immediate, a.mainLayoutWidth)
		}
		if math.Abs(float64(immediate.X+(immediate.W-reserved)/2-immediateInput.X-immediateInput.W/2)) > 0.5 {
			t.Fatalf("resizing to %d left composer %v outside the content center of viewport %v", width, immediateInput, immediate)
		}
		settleOverviewLayout(t, tt)
		settled, settledInput := overviewLayout(t, tt)
		if immediate != settled || immediateInput != settledInput {
			t.Fatalf("resizing to %d lagged: transcript %v -> %v, composer %v -> %v", width, immediate, settled, immediateInput, settledInput)
		}
		entry := a.overviews["overview-layout"]
		if width >= 1500 {
			if entry.state.Open() || !entry.state.Docked || !slices.Equal(notifications, []bool{true, false, true}) {
				t.Fatalf("the automatic overview column changed ownership: %+v, %v", entry, notifications)
			}
		} else {
			if entry.state.Docked || entry.state.Open() || !slices.Equal(notifications, []bool{true, false, true, false}) {
				t.Fatalf("narrowing did not close the column owner: %+v, %v", entry, notifications)
			}
			if _, found := tt.AccessByRole(ui.RoleComplementary, i18n.In("en", "chat_assets_title")); found {
				t.Fatal("narrowing kept the overview column")
			}
			clickOverviewHeader(t, tt)
			if !entry.state.Open() || !slices.Equal(notifications, []bool{true, false, true, false, true}) {
				t.Fatalf("the first click after narrowing did not open the popover: %+v, %v", entry, notifications)
			}
		}
	}
}
