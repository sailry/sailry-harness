package kit

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/theme"
)

// Code leaves out trailing line breaks, so no empty last line shows, and
// its theme draws no caret.
func TestCodeTrimsAndHidesCaret(t *testing.T) {
	var th ui.Color
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		Code(c, ui.Column(c).FillWidth(), "a\nb\n\n", CodeOptions{})
		th = CodeTheme(c, false).Cursor
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
}
