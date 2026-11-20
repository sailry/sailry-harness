package conversation

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// foldTester shows one sent message without a reply in a window width wide.
func foldTester(text string, width int) *ui.Tester {
	state := New()
	turns := []Turn{{ID: "turn", User: text, InputOnly: true}}
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() { View(c, state, "session", turns, nil, still{}) })
	}, width, 800)
	tester.SetLocale("en")
	tester.SetPreferences(ui.Preferences{ReduceMotion: true})
	tester.Frame()
	tester.Frame()
	return tester
}

// foldButton returns the message's Expand or Collapse button, if it has one.
func foldButton(tester *ui.Tester) (ui.AccessInfo, bool) {
	for _, key := range []string{"message_expand", "message_collapse"} {
		if b, ok := tester.AccessByRole(ui.RoleButton, i18n.In("en", key)); ok {
			return b, true
		}
	}
	return ui.AccessInfo{}, false
}

// copyTop is the top of the message's Copy button: the footer follows the
// lines the bubble shows.
func copyTop(t *testing.T, tester *ui.Tester) float32 {
	t.Helper()
	b, ok := tester.AccessByRole(ui.RoleButton, i18n.In("en", "message_copy"))
	if !ok {
		t.Fatalf("no Copy button: %q", tester.Texts())
	}
	return b.Bounds.Y
}

// A sent message shows at most five of the lines its text lays out in, by
// newlines or wrapping at the bubble's width, until it is expanded; Copy
// takes the original text either way.
func TestLongUserMessageFolds(t *testing.T) {
	lines := func(n int) string {
		var b strings.Builder
		for i := range n {
			if i > 0 {
				b.WriteByte('\n')
			}
			b.WriteString("line")
		}
		return b.String()
	}
	five := foldTester(lines(5), 900)
	if _, ok := foldButton(five); ok {
		t.Fatal("a message of five lines offers to expand")
	}
	short := foldTester("Short question", 900)
	if _, ok := foldButton(short); ok {
		t.Fatal("a one-line message offers to expand")
	}
	fiveTop := copyTop(t, five)

	soft := strings.Repeat("Sailry 会把很长的一段话按气泡宽度自动换行 and keeps wrapping softly. ", 30)
	for name, text := range map[string]string{"newlines": lines(6), "wrapped": soft} {
		t.Run(name, func(t *testing.T) {
			tester := foldTester(text, 900)
			b, ok := foldButton(tester)
			if !ok || b.Name != i18n.In("en", "message_expand") || b.Expanded {
				t.Fatalf("a long message has no collapsed Expand button: %+v %v", b, ok)
			}
			collapsed := copyTop(t, tester)
			if collapsed != fiveTop {
				t.Fatalf("collapsed footer at %v, want %v below five lines", collapsed, fiveTop)
			}
			if err := tester.Click(i18n.In("en", "message_expand")); err != nil {
				t.Fatal(err)
			}
			tester.Frame()
			b, ok = foldButton(tester)
			if !ok || b.Name != i18n.In("en", "message_collapse") || !b.Expanded {
				t.Fatalf("expanding did not offer Collapse: %+v %v", b, ok)
			}
			if expanded := copyTop(t, tester); expanded <= collapsed {
				t.Fatalf("expanded footer at %v, not below the collapsed %v", expanded, collapsed)
			}
			if err := tester.Click(i18n.In("en", "message_copy")); err != nil {
				t.Fatal(err)
			}
			tester.Frame()
			if got := tester.Clipboard(); got != text {
				t.Fatalf("Copy took %d bytes, want the original %d", len(got), len(text))
			}
			if err := tester.Click(i18n.In("en", "message_collapse")); err != nil {
				t.Fatal(err)
			}
			tester.Frame()
			if b, ok = foldButton(tester); !ok || b.Expanded || copyTop(t, tester) != collapsed {
				t.Fatalf("collapsing did not restore five lines: %+v %v", b, ok)
			}
			tester.SetClipboard("")
			if err := tester.Click(i18n.In("en", "message_copy")); err != nil {
				t.Fatal(err)
			}
			tester.Frame()
			if got := tester.Clipboard(); got != text {
				t.Fatalf("collapsed Copy took %d bytes, want the original %d", len(got), len(text))
			}
		})
	}
}

// Whether a message folds follows the width its text wraps at: a few
// lines in a wide window take more than five in a narrow pane, and fewer
// again as it widens.
func TestUserMessageFoldFollowsWidth(t *testing.T) {
	text := strings.Repeat("word ", 30)
	tester := foldTester(text, 900)
	if _, ok := foldButton(tester); ok {
		t.Fatal("a message of a few lines offers to expand in a wide window")
	}
	tester.SetSize(200, 800)
	tester.Frame()
	tester.Frame()
	if b, ok := foldButton(tester); !ok || b.Expanded {
		t.Fatalf("the message wrapped past five lines in a narrow pane does not fold: %+v %v", b, ok)
	}
	tester.SetSize(900, 800)
	tester.Frame()
	tester.Frame()
	if _, ok := foldButton(tester); ok {
		t.Fatal("the message still folds once the window is wide again")
	}
}
