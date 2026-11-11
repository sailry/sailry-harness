package preview

import (
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// The demo's goals (Demo mode) in both languages: one in pursuit whose
// continuations carry the package's marker in place of a user message, and
// one paused with its reason.
func TestDemoGoals(t *testing.T) {
	for _, row := range []struct {
		locale, marker, state, tail string
	}{
		{"en", "Continue goal", "Running", "12m"},
		{"zh-CN", "继续执行目标", "执行中", "12m"},
	} {
		t.Run("in pursuit "+row.locale, func(t *testing.T) {
			c := Demo(row.locale, "local").Find("lumen-web/pursuit")
			if c == nil || c.Status != Running || len(c.Turns) != 3 {
				t.Fatalf("the goal in pursuit is %+v", c)
			}
			// The user's turn has the objective; the turns after it have the
			// marker and no user text.
			if c.Turns[0].Marker != nil || c.Turns[0].User == "" {
				t.Fatalf("the first turn is %+v", c.Turns[0])
			}
			for _, turn := range c.Turns[1:] {
				if turn.Marker == nil || turn.User != "" {
					t.Fatalf("a continuation shows %q under %+v", turn.User, turn.Marker)
				}
				label := turn.Marker.Label
				if translated := turn.Marker.Locales[row.locale]; translated != "" {
					label = translated
				}
				if label != row.marker {
					t.Fatalf("the marker reads %q, want %q", label, row.marker)
				}
			}
			if c.Turns[2].Reply.Status != conversation.Running {
				t.Fatalf("the last continuation is %v", c.Turns[2].Reply.Status)
			}
			ctx := DemoComposer(Demo(row.locale, "local"), c, c.Worktree)
			if len(ctx.Activity.Badges) != 1 {
				t.Fatalf("the composer shows %d badges", len(ctx.Activity.Badges))
			}
			badge := ctx.Activity.Badges[0]
			text := func(index int) string {
				segment := badge.Segment[index].Text
				if translated := segment.Locales[row.locale]; translated != "" {
					return translated
				}
				return segment.Label
			}
			if len(badge.Segment) != 3 || text(1) != row.state || badge.Segment[1].Tone != kit.Info || text(2) != row.tail {
				t.Fatalf("the badge reads %+v", badge.Segment)
			}
			if len(badge.Details) != 4 || badge.Details[1].Value != "2 / 25" || badge.Panel == nil || len(badge.Panel.Buttons) != 3 || badge.Panel.Buttons[0].ID != "goal-pause" {
				t.Fatalf("the badge's details and panel are %+v, %+v", badge.Details, badge.Panel)
			}
		})
	}
	// Every goal badge has the one shape: the conversation the demo opens
	// on and the preview's, like the two above, read Goal, the state and
	// the time, and offer Pause, Edit and Clear.
	for name, badges := range map[string][]composer.StatusBadge{
		"the demo's first conversation": Demo("en", "local").Find(DemoStart).Badges,
		"the preview":                   {goal()},
	} {
		t.Run(name, func(t *testing.T) {
			if len(badges) != 1 {
				t.Fatalf("%d badges", len(badges))
			}
			badge := badges[0]
			if len(badge.Segment) != 3 || badge.Segment[0].Text.Label != "Goal" || badge.Segment[1].Text.Label != "Running" ||
				badge.Segment[1].Tone != kit.Info || badge.Segment[2].Text.Label == "" {
				t.Fatalf("the badge reads %+v", badge.Segment)
			}
			var buttons []string
			for _, button := range badge.Panel.Buttons {
				buttons = append(buttons, button.ID+" "+button.Label.Label+" "+button.Label.Locales["zh-CN"])
			}
			if want := []string{"goal-pause Pause 暂停", "goal-edit Edit 编辑", "goal-clear Clear 清除"}; !slices.Equal(buttons, want) {
				t.Fatalf("the panel offers %q, want %q", buttons, want)
			}
			if len(badge.Details) != 4 || badge.Details[0].Value == "" || badge.Details[0].Value != badge.Panel.Body.Label {
				t.Fatalf("the badge's details are %+v over %+v", badge.Details, badge.Panel.Body)
			}
		})
	}
	for _, row := range []struct {
		locale, state, reason, resume string
	}{
		{"en", "Paused", "Turn limit reached", "Resume"},
		{"zh-CN", "已暂停", "已达轮次上限", "继续"},
	} {
		t.Run("paused "+row.locale, func(t *testing.T) {
			c := Demo(row.locale, "local").Find("lumen-web/pursuit-limit")
			if c == nil || c.Status != Unread || len(c.Turns) != 2 || c.Turns[1].Marker == nil || len(c.Badges) != 1 {
				t.Fatalf("the paused goal is %+v", c)
			}
			badge := c.Badges[0]
			text := func(index int) string {
				segment := badge.Segment[index].Text
				if translated := segment.Locales[row.locale]; translated != "" {
					return translated
				}
				return segment.Label
			}
			if len(badge.Segment) != 3 || text(1) != row.state || badge.Segment[1].Tone != kit.Caution || text(2) != row.reason {
				t.Fatalf("the badge reads %+v", badge.Segment)
			}
			button := badge.Panel.Buttons[0]
			label := button.Label.Label
			if translated := button.Label.Locales[row.locale]; translated != "" {
				label = translated
			}
			if button.ID != "goal-resume" || label != row.resume || badge.Details[1].Value != "25 / 25" {
				t.Fatalf("the paused goal offers %+v with %+v", button, badge.Details)
			}
		})
	}
}
