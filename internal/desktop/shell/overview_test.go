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
	"github.com/sailry/sailry-harness/internal/desktop/theme"
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

func TestOverviewToggleKeepsConversationCentered(t *testing.T) {
	f := newOverviewLifecycleFixture(t, 1800)
	f.loading, f.complete = false, true
	f.settle()
	before, input := overviewLayout(t, f.tester)
	if !layoutNear(input.X+input.W/2, before.X+before.W/2) {
		t.Fatalf("the visible activity card moved the composer off the pane's center: %v, %v", before, input)
	}
	for _, hidden := range []bool{true, false, true} {
		clickOverviewHeader(t, f.tester)
		if f.app.OverviewHidden != hidden || f.app.overviews["overview-session"].state.Open() {
			t.Fatalf("the wide activity button did not toggle its card to hidden=%v", hidden)
		}
		_, card := f.tester.AccessByRole(ui.RoleComplementary, i18n.In("en", "chat_assets_title"))
		if card == hidden {
			t.Fatalf("activity visibility does not match hidden=%v", hidden)
		}
		after, afterInput := overviewLayout(t, f.tester)
		if after != before || afterInput != input {
			t.Fatalf("toggling activity shifted the conversation: %v -> %v, %v -> %v", before, after, input, afterInput)
		}
	}
	if !slices.Equal(f.opened, []bool{true, false, true, false}) {
		t.Fatalf("activity toggle notified its inventory owner %v", f.opened)
	}
	prefs := f.app.prefs()
	restored := NewLive(f.app.Data)
	restored.SetPrefs(prefs)
	if !restored.OverviewHidden {
		t.Fatal("the activity button's visibility choice was not restored")
	}
	f.tester.SetSize(1100, 820)
	settleOverviewLayout(t, f.tester)
	clickOverviewHeader(t, f.tester)
	if !f.app.OverviewHidden || !f.app.overviews["overview-session"].state.Open() {
		t.Fatal("the narrow activity button changed the wide choice or failed to open its popover")
	}
	f.tester.SetSize(1800, 820)
	settleOverviewLayout(t, f.tester)
	if _, card := f.tester.AccessByRole(ui.RoleComplementary, i18n.In("en", "chat_assets_title")); card || !f.app.OverviewHidden {
		t.Fatal("returning to a wide pane lost the explicit hidden choice")
	}
}

// Crossing the activity breakpoint must not move or rewrap the conversation.
func TestOverviewBreakpointPreservesCenter(t *testing.T) {
	boundary := int(RailWidth + 1 + overviewSideMin)
	a, tt := overviewLayoutFixture(t, boundary)
	var previousInput ui.Rect
	for index, width := range []int{boundary - 1, boundary, boundary + 1, boundary - 1} {
		tt.SetSize(width, 820)
		log, input := overviewLayout(t, tt)
		if !layoutNear(input.X+input.W/2, log.X+log.W/2) || !layoutNear(log.X+log.W, float32(width)) {
			t.Fatalf("crossing the activity breakpoint moved the content or narrowed scrolling: %v, %v", log, input)
		}
		_, card := tt.AccessByRole(ui.RoleComplementary, i18n.In("en", "chat_assets_title"))
		if card != (width >= boundary) || a.OverviewHidden {
			t.Fatalf("width %d did not adapt activity without changing its visibility choice", width)
		}
		if index > 0 && !layoutNear(previousInput.W, input.W) {
			t.Fatalf("crossing the activity breakpoint rewrapped content: composer width %.1f -> %.1f", previousInput.W, input.W)
		}
		previousInput = input
	}
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

func overviewHasToggle(t *testing.T, tt *ui.Tester) {
	t.Helper()
	if _, found := headerButton(tt, i18n.In("en", "chat_assets_title")); !found {
		t.Fatal("the wide conversation has no activity toggle in the header")
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

// The activity card fits beside fully centered content at wide sizes and
// never narrows the transcript's scroll viewport.
func TestOverviewDocksWhereItFits(t *testing.T) {
	for _, width := range []int{1700, 2100} {
		t.Run(strconv.Itoa(width), func(t *testing.T) {
			a, tt := overviewLayoutFixture(t, width)
			log, input := overviewLayout(t, tt)
			card := overviewLayoutCard(t, tt)
			if math.Abs(float64(a.mainLayoutWidth-log.W)) > 0.5 || math.Abs(float64(card.W-composer.OverviewWidth)) > 0.5 || math.Abs(float64(log.X+log.W-float32(width))) > 0.5 {
				t.Fatalf("main width %v has scroll viewport %v and card %v; the viewport must reach the window's right edge", a.mainLayoutWidth, log, card)
			}
			contentRight := log.X + (log.W+theme.ContentWidth)/2
			if math.Abs(float64(log.X+log.W/2-input.X-input.W/2)) > 0.5 {
				t.Fatalf("the composer %v is not centered in the full viewport %v", input, log)
			}
			if input.X+input.W > contentRight || card.X < contentRight || card.X+card.W > log.X+log.W || card.H > overviewCardMax+8 {
				t.Fatalf("the card %v overlaps the content or lies outside the viewport %v; composer %v", card, log, input)
			}
			if !tt.HasText("Fixture output") || tt.HasText(i18n.In("en", "chat_assets_waiting")) {
				t.Fatalf("the card does not show its inventory: %q", tt.Texts())
			}
			overviewHasToggle(t, tt)
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
	for _, width := range []int{1700, 2100, 1700, 1100} {
		tt.SetSize(width, 820)
		tt.Frame()
		immediate, immediateInput := overviewLayout(t, tt)
		if width >= 1700 {
			card := overviewLayoutCard(t, tt)
			overviewHasToggle(t, tt)
			if card.X < immediate.X+(immediate.W+theme.ContentWidth)/2 {
				t.Fatalf("resizing to %d overlapped the overview and transcript", width)
			}
		}
		if math.Abs(float64(a.mainLayoutWidth-immediate.W)) > 0.5 || math.Abs(float64(immediate.X+immediate.W-float32(width))) > 0.5 {
			t.Fatalf("resizing to %d left a narrowed scroll viewport %v; main width %v", width, immediate, a.mainLayoutWidth)
		}
		if math.Abs(float64(immediate.X+immediate.W/2-immediateInput.X-immediateInput.W/2)) > 0.5 {
			t.Fatalf("resizing to %d left composer %v outside the content center of viewport %v", width, immediateInput, immediate)
		}
		settleOverviewLayout(t, tt)
		settled, settledInput := overviewLayout(t, tt)
		if immediate != settled || immediateInput != settledInput {
			t.Fatalf("resizing to %d lagged: transcript %v -> %v, composer %v -> %v", width, immediate, settled, immediateInput, settledInput)
		}
		entry := a.overviews["overview-layout"]
		if width >= 1700 {
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
