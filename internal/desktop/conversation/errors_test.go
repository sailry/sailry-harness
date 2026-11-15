package conversation

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestReplyFailureStaysAboveActionsOutsideWork(t *testing.T) {
	const cause = "HTTP 400: Original provider response"
	const output = "Original command output"
	for _, language := range []string{"en", "zh-CN"} {
		for _, segmented := range []bool{false, true} {
			name := language
			if segmented {
				name += "/guidance"
			}
			t.Run(name, func(t *testing.T) {
				title := i18n.In(language, "chat_model_failed")
				turn := Turn{ID: "failed", User: "Task", Reply: Reply{Status: Failed, Blocks: []Block{
					{Kind: TextBlock, Text: "Earlier answer"},
					{Kind: ErrorBlock, Label: title, Fields: [][2]string{{"code", "unavailable"}}, Text: cause},
					{Kind: ToolsBlock, Calls: []Call{{ID: "command", Name: "command", Label: "Command", Detail: "make check", Output: &Output{Text: output}}}},
					{Kind: TextBlock, Text: "Visible partial answer"},
				}, Changes: &Changes{Files: []ChangedFile{{Path: "main.go", Added: 1}}}}}
				if segmented {
					turn.Segments = []Segment{
						{Inputs: []Input{{ID: turn.ID, User: "Task"}}},
						{From: 1, Inputs: []Input{{ID: "guide", User: "Constraint"}}},
					}
				}
				state := New()
				tester := ui.NewTester(func(c *ui.Context) {
					theme.Apply(c, "", theme.Light)
					ui.Column(c).Fill().Children(func() { View(c, state, "session", []Turn{turn}, nil, still{}) })
				}, 900, 1000)
				tester.SetLocale(language)
				tester.SetPreferences(ui.Preferences{ReduceMotion: true})
				tester.Frame()
				tester.Frame()
				failure, found := tester.Find(cause)
				if !found || !tester.HasText(title) || !tester.HasText("unavailable") || tester.HasText(output) {
					t.Fatalf("the failure was folded or the original tool result was exposed: %q", tester.Texts())
				}
				for _, key := range []string{"turn_copy", "chat_retry"} {
					button, ok := tester.AccessByRole(ui.RoleButton, i18n.In(language, key))
					if !ok || button.Bounds.Y < failure.Y+failure.H {
						t.Fatalf("%s is missing or above the failure: %+v, %+v", key, button, failure)
					}
				}
				more, ok := tester.AccessByRole(ui.RoleMenuButton, i18n.In(language, "more"))
				if !ok || more.HasPopup != ui.PopupMenu || more.Bounds.Y < failure.Y+failure.H {
					t.Fatalf("the More menu is missing or above the failure: %+v, %+v", more, failure)
				}
				if review, ok := tester.AccessByRole(ui.RoleButton, i18n.In(language, "turn_changes_review")); !ok || review.Bounds.Y+review.Bounds.H > failure.Y {
					t.Fatal("the failure was separated from the footer by the changes card")
				}
				header, ok := tester.Find(title)
				if !ok {
					t.Fatal("the failure has no header")
				}
				copies := 0
				for _, button := range tester.AccessAll() {
					middle := header.Y + header.H/2
					if button.Role == ui.RoleButton && button.Name == i18n.In(language, "content_copy") && button.Bounds.Y <= middle && button.Bounds.Y+button.Bounds.H >= middle {
						tester.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
						copies++
					}
				}
				if copies != 1 {
					t.Fatalf("the failure header has %d copy actions", copies)
				}
				if got := tester.Clipboard(); got != title+"\ncode: unavailable\n\n"+cause {
					t.Fatalf("failure Copy changed the original detail: %q", got)
				}
				if err := tester.Click(i18n.In(language, "turn_failed")); err != nil {
					t.Fatal(err)
				}
				tester.Frame()
				if err := tester.Click("Command"); err != nil {
					t.Fatal(err)
				}
				tester.Frame()
				if !tester.HasText(output) || !tester.HasText(cause) {
					t.Fatal("opening work lost the original tool output or provider failure")
				}
			})
		}
	}
}
