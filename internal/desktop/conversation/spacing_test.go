package conversation

import (
	"fmt"
	"math"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestReplyContentSpacing(t *testing.T) {
	for _, size := range []uint8{12, 14, 18} {
		t.Run(fmt.Sprintf("text_size_%d", size), func(t *testing.T) {
			t.Run("work and answer boundaries", func(t *testing.T) {
				state := New()
				turns := []Turn{{ID: "spacing", User: "Review", Reply: Reply{Status: Running, Blocks: []Block{
					{Kind: TextBlock, Text: "Before tools"},
					{Kind: ToolsBlock, Calls: []Call{{ID: "scope", Name: "scope", Label: "Review scope", Output: &Output{Text: "Scope output"}}}},
					{Kind: TextBlock, Text: "Between tools"},
					{Kind: ToolsBlock, Calls: []Call{{ID: "complete", Name: "complete", Label: "Complete review", Output: &Output{Text: "Review output"}}}},
					{Kind: TextBlock, Text: "Final answer"},
				}}}}
				tester := ui.NewTester(func(c *ui.Context) {
					theme.Apply(c, "", theme.Light)
					kit.SetPreferences(c, kit.Preferences{MessageTextSize: size})
					View(c, state, "spacing", turns, nil, still{})
				}, 900, 700)
				tester.SetLocale("en")
				tester.SetPreferences(ui.Preferences{ReduceMotion: true})
				tester.Frame()
				before := spacingText(t, tester, "Before tools")
				between := spacingText(t, tester, "Between tools")
				after := spacingText(t, tester, "Final answer")
				scope := spacingRow(t, tester, "Review scope")
				complete := spacingRow(t, tester, "Complete review")
				toolToText := between.Y - (scope.Y + scope.H)
				workToAnswer := after.Y - (complete.Y + complete.H)
				if toolToText < workGap-0.5 || math.Abs(float64(workToAnswer-toolToText)) > 0.5 {
					t.Fatalf("tool-to-text spacing changes at the answer boundary: work=%v answer=%v", toolToText, workToAnswer)
				}
				for _, gap := range []float32{scope.Y - (before.Y + before.H), complete.Y - (between.Y + between.H)} {
					if gap < workGap-0.5 {
						t.Fatalf("text-to-tool spacing=%v, want at least %v", gap, workGap)
					}
				}
			})
			t.Run("adjacent tool groups", func(t *testing.T) {
				state := New()
				calls := []Call{
					{ID: "scope-1", Name: "scope", Label: "Read scope", Output: &Output{Text: "First output"}},
					{ID: "scope-2", Name: "scope", Label: "Review scope", Output: &Output{Text: "Second output"}},
					{ID: "plan", Name: "plan", Label: "Update plan", Output: &Output{Text: "Plan output"}},
					{ID: "command", Name: "command", Label: "Command", Output: &Output{Text: "Command output"}},
				}
				tester := ui.NewTester(func(c *ui.Context) {
					theme.Apply(c, "", theme.Light)
					kit.SetPreferences(c, kit.Preferences{MessageTextSize: size})
					state.groups(c, calls)
				}, 900, 700)
				tester.Frame()
				rows := []ui.Rect{spacingRow(t, tester, "Review scope"), spacingRow(t, tester, "Update plan"), spacingRow(t, tester, "Command")}
				for index := 1; index < len(rows); index++ {
					gap := rows[index].Y - (rows[index-1].Y + rows[index-1].H)
					if math.Abs(float64(gap-workGap)) > 0.5 {
						t.Fatalf("tool group spacing=%v, want %v", gap, workGap)
					}
				}
			})
		})
	}
}

func spacingText(t *testing.T, tester *ui.Tester, label string) ui.Rect {
	t.Helper()
	bounds, ok := tester.Find(label)
	if !ok {
		t.Fatalf("missing transcript text %q", label)
	}
	return bounds
}

func spacingRow(t *testing.T, tester *ui.Tester, label string) ui.Rect {
	t.Helper()
	text := spacingText(t, tester, label)
	var bounds ui.Rect
	for _, item := range tester.AccessAll() {
		candidate := item.Bounds
		middle := text.Y + text.H/2
		if item.Role == ui.RoleButton && candidate.X <= text.X && candidate.X+candidate.W >= text.X+text.W && candidate.Y <= middle && candidate.Y+candidate.H >= middle && (bounds.H == 0 || candidate.H < bounds.H) {
			bounds = candidate
		}
	}
	if bounds.H == 0 {
		t.Fatalf("missing transcript row %q", label)
	}
	return bounds
}
