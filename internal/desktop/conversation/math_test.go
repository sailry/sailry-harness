package conversation_test

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// checkMath is the headless smoke of math in a reply: every delimiter typesets a picture in light and dark, a
// price stays text, and a formula with no end draws without failing.
func checkMath(t *testing.T) {
	t.Helper()
	const reply = "Inline $x^2$ and \\(2+3\\), then\n\n" +
		"$$P(A\\mid B)=\\frac{P(B\\mid A)P(A)}{P(B)}$$\n\n" +
		"\\[\\begin{aligned}a &= b \\\\ c &= d\\end{aligned}\\]\n\n" +
		"Plans cost $5 and $10 per month."
	mode := theme.Light
	tt := replyTester(reply, &mode)
	// Display formulas are pictures; inline ones are objects in their line,
	// which replace their source text.
	light := pictures(tt)
	if light != 2 {
		t.Errorf("%d display formulas typeset, want 2", light)
	}
	if !tt.HasText("Inline \uFFFC and \uFFFC, then") {
		t.Errorf("inline formulas kept their source: %q", tt.Texts())
	}
	if !tt.HasText("Plans cost $5 and $10 per month.") {
		t.Errorf("the prices did not stay text: %q", tt.Texts())
	}
	mode = theme.Dark
	tt.Frame()
	if dark := pictures(tt); dark != light {
		t.Errorf("%d formulas in dark, %d in light", dark, light)
	}
	for _, partial := range []string{"\\(unfinished", "\\[unfinished", "$$\\frac{1}{", "\\(\\)"} {
		mode = theme.Light
		replyTester("Streaming "+partial, &mode).Frame()
	}
}
