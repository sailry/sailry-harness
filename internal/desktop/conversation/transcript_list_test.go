package conversation

import (
	"fmt"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestFinishFrameUnbindsOnlyUnmountedTranscripts(t *testing.T) {
	s := New()
	left := []Turn{{ID: "left-old", User: "Left transcript", InputOnly: true}}
	right := []Turn{{ID: "right", User: "Right transcript", InputOnly: true}}
	leftSession, rightSession := &Session{}, &Session{}
	leftActions, rightActions := &still{}, &still{}
	showLeft, showRightRail := true, true
	tt := ui.NewTester(func(c *ui.Context) {
		defer s.FinishFrame()
		theme.Apply(c, "", theme.Light)
		ui.Row(c).Fill().Children(func() {
			ui.Column(c.Key("right-pane")).Width(300).FillHeight().Children(func() {
				View(c, s, "right", right, rightSession, rightActions)
				if showRightRail {
					Navigation(c, s)
				}
			})
			if showLeft {
				ui.Column(c.Key("left-pane")).Grow(1).FillHeight().Children(func() {
					View(c, s, "left", left, leftSession, leftActions)
					Navigation(c, s)
				})
			}
		})
	}, 1260, 360)
	tt.SetLocale("en")
	leftLayout, rightLayout := s.List("left"), s.List("right")
	oldKey := leftLayout.Key
	leftList, rightList := s.lists["left"], s.lists["right"]
	leftRail, rightRail := leftList.nav, rightList.nav
	if oldKey(0) != "left-old" || leftRail == nil || leftRail.transcript != leftList || rightRail == nil || rightRail.transcript != rightList {
		t.Fatal("both transcripts and rails must be bound before hiding one")
	}
	assertDetached := func() {
		t.Helper()
		f := leftList.frame
		if f.turns != nil || f.session != nil || f.actions != nil || f.state != nil || f.c != nil {
			t.Error("the unmounted list still owns its render payload")
		}
		if leftRail.transcript != nil || leftRail.c != nil || oldKey(0) != 0 {
			t.Error("the old rail or key can still reach the unmounted transcript")
		}
		if s.List("left") != leftLayout || !leftLayout.FollowEnd {
			t.Error("detaching the payload changed the persistent layout state")
		}
	}
	assertRightMounted := func() {
		t.Helper()
		if s.List("right") != rightLayout || rightLayout.Key(0) != "right" || !rightLayout.AtEnd() ||
			rightList.frame.actions != rightActions || !tt.HasText(right[0].User) {
			t.Error("a sibling still mounted in this frame lost its content or layout state")
		}
		if rightRail.transcript != nil || rightRail.c != nil {
			t.Error("the right rail keeps a binding even though it was not built")
		}
	}

	showLeft, s.Split = false, true
	tt.Frame()
	assertDetached()
	assertRightMounted()

	showLeft, s.Split, showRightRail = true, false, false
	left = []Turn{{ID: "left-new", User: "Reopened transcript", InputOnly: true}}
	leftActions = &still{}
	tt.Frame()
	if s.lists["left"] != leftList || oldKey(0) != "left-new" || leftList.frame.actions != leftActions ||
		leftList.nav != leftRail || leftRail.transcript != leftList || !tt.HasText(left[0].User) {
		t.Fatal("the same list and rail must bind the new keys, content, and actions on reopening")
	}
	assertRightMounted()

	// The right transcript leaves a pending rail; the loading pane must not
	// mount it as its own rail or keep the left list's previous payload.
	leftSession.Loading = true
	tt.Frame()
	if tt.HasText(left[0].User) || !tt.HasText(i18n.In("en", "chat_history_loading")) {
		t.Fatal("the loading pane must show only its loader, not the previous transcript")
	}
	assertDetached()
	assertRightMounted()
}

func TestFinishFramePreservesPixelAnchorAndRailKeys(t *testing.T) {
	s := New()
	sess := &Session{}
	shown := true
	turns := make([]Turn, 36)
	for i := range turns {
		id := fmt.Sprintf("Message %02d", i)
		turns[i] = Turn{ID: id, User: id, InputOnly: true, Controls: Controls{ReadOnly: true}}
	}
	tt := ui.NewTesterWith(func(c *ui.Context) {
		defer s.FinishFrame()
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() {
			if shown {
				View(c, s, "conversation", turns, sess, still{})
				Navigation(c, s)
			}
		})
	}, 1100, 440, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true}})
	tt.SetLocale("en")
	layout := s.List("conversation")
	const anchor = 12
	layout.ScrollTo(anchor, ui.Start)
	tt.Frame()
	label := turns[anchor+1].User // completely visible even with a partial first row
	yOfLabel := func() float32 {
		t.Helper()
		r, ok := tt.Find(label)
		if !ok {
			t.Fatalf("the row after the anchor (%q) is not visible", label)
		}
		return r.Y
	}
	alignedY := yOfLabel()
	tt.Scroll(550, 200, 0, 17)
	tt.Frame()
	first, _ := layout.Visible()
	y := yOfLabel()
	if first != anchor || alignedY-y < 1 || layout.AtEnd() {
		t.Fatalf("expected a partially scrolled first row %d, got row %d at Y %.1f (from %.1f)", anchor, first, y, alignedY)
	}
	key := layout.Key(first)

	shown = false
	tt.Frame()
	turns = append([]Turn{
		{ID: "older-1", User: "First older message", InputOnly: true},
		{ID: "older-2", User: "Second older message", InputOnly: true},
	}, turns...)
	shown = true
	tt.Frame()
	first, _ = layout.Visible()
	if s.List("conversation") != layout || first != anchor+2 || layout.Key(first) != key || layout.AtEnd() {
		t.Fatalf("reopening did not keep the same anchored row: row %d key %v, want row %d key %v", first, layout.Key(first), anchor+2, key)
	}
	if diff := yOfLabel() - y; diff < -0.5 || diff > 0.5 {
		t.Errorf("reopening kept the row but moved its pixel position by %.1f DIPs", diff)
	}

	rail, ok := tt.AccessByRole(ui.RoleSlider, i18n.In("en", "chat_navigation"))
	if !ok {
		t.Fatal("the navigation rail did not return")
	}
	r := rail.Bounds
	tt.ClickAt(r.X+trackPad+tickInset+2, r.Y+tickY(r.H, len(turns), first)+tickHeight/2)
	tt.Key(0, ui.KeyHome)
	tt.Frame()
	if first, _ := layout.Visible(); first != 0 || !tt.HasText(turns[0].User) {
		t.Errorf("Home on the rail should build and show the prepended first row, got row %d", first)
	}
	tt.Key(0, ui.KeyEnd)
	tt.Frame()
	if _, last := layout.Visible(); last != len(turns)-1 || !tt.HasText(turns[len(turns)-1].User) {
		t.Errorf("End on the rail should build and show the last row, got row %d", last)
	}
}
