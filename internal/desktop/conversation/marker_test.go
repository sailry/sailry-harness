package conversation_test

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// A turn a package continued shows the package's marker in the window's
// language where a user message would be, one row high, and the generic
// label when the package declared none.
func TestContinuationMarker(t *testing.T) {
	marker := &conversation.Marker{Label: "Continue goal", Locales: map[string]string{"zh-CN": "继续执行目标"}}
	reply := func(text string) conversation.Reply {
		return conversation.Reply{Status: conversation.Completed, Blocks: []conversation.Block{{Kind: conversation.TextBlock, Text: text}}}
	}
	turns := []conversation.Turn{
		{ID: "set", User: "Ship the feature", Reply: reply("Started")},
		{ID: "continued", Marker: marker, Reply: reply("Step done")},
		{ID: "guided", Marker: marker, User: "Also update the docs", Reply: reply("Docs updated")},
		{ID: "generic", Marker: &conversation.Marker{}, Reply: reply("Another step")},
	}
	for _, row := range []struct {
		locale, label string
	}{
		{"en", "Continue goal"},
		{"zh-CN", "继续执行目标"},
		// A language the package does not ship reads its English label.
		{"fr", "Continue goal"},
	} {
		t.Run(row.locale, func(t *testing.T) {
			s := conversation.New()
			sess := &conversation.Session{}
			r := &recorder{}
			tt := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Dark)
				ui.Column(c).Fill().Children(func() { conversation.View(c, s, "t", turns, sess, r) })
			}, 1024, 1500)
			tt.SetLocale(row.locale)
			tt.Frame()
			tt.Frame()
			texts := tt.Texts()
			count := func(text string) int {
				n := 0
				for _, shown := range texts {
					if shown == text {
						n++
					}
				}
				return n
			}
			// Two continued turns carry the package's label, the third the generic one.
			if count(row.label) != 2 {
				t.Fatalf("the marker %q shows %d times in %v", row.label, count(row.label), texts)
			}
			if generic := i18n.In(i18n.Match(row.locale), "turn_continued"); count(generic) != 1 {
				t.Fatalf("the generic marker %q shows %d times in %v", generic, count(generic), texts)
			}
			// The user's own messages still show, guidance included.
			for _, text := range []string{"Ship the feature", "Also update the docs", "Step done", "Another step"} {
				if !slices.Contains(texts, text) {
					t.Fatalf("%q is not shown in %v", text, texts)
				}
			}
		})
	}
}
