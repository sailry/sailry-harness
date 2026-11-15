package shell

import (
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func conversationLayoutFixture(t *testing.T, width int) (*App, *ui.Tester) {
	t.Helper()
	cv := &viewmodel.Conversation{
		ID: "layout-session", Title: "Layout fixture",
		Turns: []conversation.Turn{{
			ID: "layout-turn", User: "Describe the layout",
			Reply: conversation.Reply{Status: conversation.Completed, Blocks: []conversation.Block{{
				Kind: conversation.TextBlock,
				Text: strings.Repeat("The transcript and composer share the space beside the activity column. ", 12),
			}}},
		}},
	}
	a := NewLive(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{cv}, Workspace: &viewmodel.Workspace{}})
	a.Sidebar.Selected = cv.ID
	a.Hooks.Overview = func(*ui.Context, *viewmodel.Conversation) *composer.Overview {
		return &composer.Overview{Session: true}
	}
	tt := ui.NewTesterWith(a.View, width, 820, opaque)
	tt.SetLocale("en")
	tt.Advance(time.Second)
	tt.Frame()
	return a, tt
}

func conversationLayoutRects(t *testing.T, tt *ui.Tester) (list, input ui.Rect) {
	t.Helper()
	count := 0
	for _, item := range tt.AccessAll() {
		if item.Role == ui.RoleLog && item.Bounds.H > 0 {
			list = item.Bounds
			count++
		}
	}
	if count != 1 {
		t.Fatalf("found %d transcript lists, want one", count)
	}
	field, ok := tt.AccessByRole(ui.RoleTextField, i18n.In("en", "composer"))
	if !ok {
		t.Fatal("the transcript has no composer")
	}
	return list, field.Bounds
}

func layoutNear(first, second float32) bool { return first-second < 0.5 && second-first < 0.5 }

// The navigation can slide while message layout takes its final width once.
// The list and composer move together instead of reflowing every frame.
func TestConversationWidthDuringNavigation(t *testing.T) {
	const width = 1850
	a, tt := conversationLayoutFixture(t, width)
	for _, open := range []bool{false, true} {
		before, _ := conversationLayoutRects(t, tt)
		cardRight := func() float32 {
			t.Helper()
			card, ok := tt.AccessByRole(ui.RoleComplementary, i18n.In("en", "chat_assets_title"))
			if !ok {
				t.Fatal("the activity column disappeared during sidebar motion")
			}
			return card.Bounds.X + card.Bounds.W
		}
		edge := cardRight()
		beforeCenter := before.X + before.W/2
		a.NavOpen = open
		tt.Frame()
		start, startInput := conversationLayoutRects(t, tt)
		wantWidth := float32(width - RailWidth - 1 - overviewSide)
		wantLeft := float32(RailWidth + 1)
		if open {
			wantWidth -= NavWidth
			wantLeft += NavWidth
		}
		if !layoutNear(start.W, wantWidth) {
			t.Fatalf("navigation open %v starts at list width %.1f, want destination %.1f", open, start.W, wantWidth)
		}
		if !layoutNear(start.X+start.W/2, beforeCenter) || !layoutNear(startInput.X+startInput.W/2, beforeCenter) {
			t.Fatalf("navigation open %v jumped the content center from %.1f: list %v, composer %v", open, beforeCenter, start, startInput)
		}
		if !layoutNear(cardRight(), edge) {
			t.Fatal("sidebar motion moved the activity card away from the right edge")
		}
		tt.Advance(60 * time.Millisecond)
		middle, middleInput := conversationLayoutRects(t, tt)
		if !layoutNear(middle.W, wantWidth) || !layoutNear(middleInput.W, startInput.W) {
			t.Fatalf("navigation open %v reflowed while moving: list %v -> %v, composer %v -> %v", open, start, middle, startInput, middleInput)
		}
		if !layoutNear(cardRight(), edge) {
			t.Fatal("the activity card drifted during sidebar motion")
		}
		tt.Advance(time.Second)
		end, endInput := conversationLayoutRects(t, tt)
		if !layoutNear(end.W, wantWidth) || !layoutNear(end.X, wantLeft) || !layoutNear(endInput.W, startInput.W) {
			t.Fatalf("navigation open %v ended at list %v and composer %v", open, end, endInput)
		}
		startCenter, middleCenter, endCenter := start.X+start.W/2, middle.X+middle.W/2, end.X+end.W/2
		if middleCenter <= min(startCenter, endCenter) || middleCenter >= max(startCenter, endCenter) || !layoutNear(middleInput.X+middleInput.W/2, middleCenter) {
			t.Fatalf("navigation open %v did not move list and composer together: centers %.1f -> %.1f -> %.1f; composer %v", open, startCenter, middleCenter, endCenter, middleInput)
		}
	}
}

// Window resizing follows the supplied width immediately, including crossing
// the card/popover boundary. It adds no trailing content-width animation.
func TestConversationWindowResizeIsImmediate(t *testing.T) {
	a, tt := conversationLayoutFixture(t, 1850)
	for _, width := range []int{1500, 1320, 1700, 1850} {
		tt.SetSize(width, 820)
		list, input := conversationLayoutRects(t, tt)
		mainWidth := float32(width - RailWidth - 1 - NavWidth)
		wantWidth := mainWidth
		if mainWidth >= overviewSideMin {
			wantWidth -= overviewSide
		}
		wantCenter := float32(RailWidth+1+NavWidth) + wantWidth/2
		if !layoutNear(a.mainLayoutWidth, mainWidth) || !layoutNear(list.W, wantWidth) || !layoutNear(list.X+list.W/2, wantCenter) || !layoutNear(input.X+input.W/2, wantCenter) {
			t.Fatalf("window width %d did not apply immediately: main %.1f, list %v, composer %v; want list width %.1f centered %.1f", width, a.mainLayoutWidth, list, input, wantWidth, wantCenter)
		}
		tt.Advance(300 * time.Millisecond)
		after, afterInput := conversationLayoutRects(t, tt)
		if !layoutNear(after.X, list.X) || !layoutNear(after.W, list.W) || !layoutNear(afterInput.X, input.X) || !layoutNear(afterInput.W, input.W) {
			t.Fatalf("window width %d left a trailing resize animation: list %v -> %v, composer %v -> %v", width, list, after, input, afterInput)
		}
	}
}
