package acceptance

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestLiveAnswerVisibilityAcrossParagraphs(t *testing.T) {
	const answer = "A fictional red kite rises over a quiet field and returns to its owner.\n\nKITE-FINISHED"
	for _, show := range []bool{false, true} {
		tester := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			ui.Column(c).Fill().Children(func() {
				ui.Text(c, "Describe a kite and end with KITE-FINISHED")
				if show {
					kit.Markdown(c, answer, kit.MarkdownProps{})
				}
			})
		}, 600, 400)
		tester.Frame()
		w := &liveWindow{window: &window{tester: tester}}
		if w.visibleAnswer(answer) != show {
			t.Fatalf("separate final paragraph shown=%t was confused with prompt text", show)
		}
	}
}
