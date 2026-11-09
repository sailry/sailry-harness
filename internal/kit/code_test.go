package kit

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/theme"
)

// Code leaves out trailing line breaks, so no empty last line shows, and
// its theme draws no caret and no background but in messages' blocks.
func TestCodeTrimsAndHidesCaret(t *testing.T) {
	var th, bg, block ui.Color
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		Code(c, ui.Column(c).FillWidth(), "a\nb\n\n", CodeOptions{})
		th, bg, block = CodeTheme(c, false).Cursor, CodeTheme(c, false).Background, CodeTheme(c, true).Background
	}, 400, 300)
	tt.Frame()
	e, ok := tt.Access("Code editor")
	if !ok {
		t.Fatal("no editor")
	}
	if e.Value != "a\nb" {
		t.Errorf("code shows %q", e.Value)
	}
	if th.A != 0 {
		t.Errorf("the caret shows: %+v", th)
	}
	if bg.A != 0 || block.A == 0 {
		t.Errorf("backgrounds: code %+v, message block %+v", bg, block)
	}
}

// Editable code shows the caret; a diff's background is the opaque
// surface, so MyGo tints its changed lines over it rather than white.
func TestCodeLooks(t *testing.T) {
	var caret, diff, surface ui.Color
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		caret, diff = codeTheme(c, lookEditable).Cursor, codeTheme(c, lookDiff).Background
		surface = theme.Of(c).Background.Alpha(1)
	}, 400, 300)
	tt.Frame()
	if caret.A == 0 {
		t.Error("an editable view hides the caret")
	}
	if diff != surface {
		t.Errorf("diff background %+v, surface %+v", diff, surface)
	}
}
