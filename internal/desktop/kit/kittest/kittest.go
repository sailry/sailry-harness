// Package kittest holds the assertions tests of Kit's hosts share.
package kittest

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

// Centred fails unless the empty state titled title (kit.Result with its
// icon and title alone) is centred both ways in area, within 1 DIP: a
// state standing for a whole empty area fills it. The state is its 48 DIP
// tile, a 12 DIP gap and the title.
func Centred(t *testing.T, tt *ui.Tester, title string, area ui.Rect) {
	t.Helper()
	text, ok := tt.Find(title)
	if !ok {
		t.Errorf("no empty state %q: %q", title, tt.Texts())
		return
	}
	const tile, gap = 48, 12
	x, y := text.X+text.W/2, (text.Y-gap-tile+text.Y+text.H)/2
	wantX, wantY := area.X+area.W/2, area.Y+area.H/2
	if x < wantX-1 || x > wantX+1 || y < wantY-1 || y > wantY+1 {
		t.Errorf("the empty state %q is centred at %v, %v, its area %v at %v, %v", title, x, y, area, wantX, wantY)
	}
}
