package conversation

import (
	"cmp"
	"fmt"
	"math"
	"slices"
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
			// A sequence's rows sit one rail gap apart, the first under its
			// summary as the others under each other.
			t.Run("sequence rows", func(t *testing.T) {
				state := New()
				calls := []Call{
					{ID: "scope-1", Name: "scope", Label: "Read scope", Output: &Output{Text: "First output"}},
					{ID: "scope-2", Name: "scope", Label: "Review scope", Output: &Output{Text: "Second output"}},
					{ID: "plan", Name: "plan", Label: "Update plan", Output: &Output{Text: "Plan output"}},
					{ID: "command", Name: "command", Label: "Command", Output: &Output{Text: "Command output"}},
				}
				state.toggle("tool-sequence-scope-1")
				tester := ui.NewTester(func(c *ui.Context) {
					theme.Apply(c, "", theme.Light)
					kit.SetPreferences(c, kit.Preferences{MessageTextSize: size})
					state.tools(c, calls, false)
				}, 900, 700)
				tester.Frame()
				scope, plan := spacingRow(t, tester, "Review scope"), spacingRow(t, tester, "Update plan")
				var rows []ui.Rect
				for _, item := range tester.AccessAll() {
					if item.Role == ui.RoleButton && item.Bounds.H > 0 {
						rows = append(rows, item.Bounds)
					}
				}
				slices.SortFunc(rows, func(a, b ui.Rect) int { return cmp.Compare(a.Y, b.Y) })
				if len(rows) != 4 {
					t.Fatalf("sequence rows=%d, want its summary and three runs", len(rows))
				}
				for index, gap := range []float32{scope.Y - (rows[0].Y + rows[0].H), plan.Y - (scope.Y + scope.H), rows[3].Y - (plan.Y + plan.H)} {
					if math.Abs(float64(gap-railGap)) > 0.5 {
						t.Fatalf("sequence row %d spacing=%v, want %v", index, gap, railGap)
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
