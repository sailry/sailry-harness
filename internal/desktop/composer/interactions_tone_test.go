package composer

import (
	"image/color"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// TestInteractionCapsuleTones checks that a pending approval washes its
// capsule in Warning and a pending question in Info, on both themes, while
// an approval capsule holding only receipts keeps the muted chip.
func TestInteractionCapsuleTones(t *testing.T) {
	const width, height = 480, 200
	const none, warm, cool = 0, 1, -1 // the wash's lean: none, amber, blue
	for _, mode := range []theme.Mode{theme.Light, theme.Dark} {
		for _, tc := range []struct {
			name, label string
			view        func(s *State, c *ui.Context)
			lean        int
		}{
			{"pending approval", i18n.In("en", "approval_live_count", "count", 1),
				func(s *State, c *ui.Context) { s.approvals(c, []Approval{{ID: "a", Title: "Run"}}, nil) }, warm},
			{"approval receipt", i18n.In("en", "approval_receipt"),
				func(s *State, c *ui.Context) { s.approvals(c, []Approval{{ID: "a", Title: "Run", Receipt: true}}, nil) }, none},
			{"pending question", i18n.In("en", "question_reply"),
				func(s *State, c *ui.Context) { s.ask(c, &Ask{ID: "q", Prompt: "Which?"}, nil) }, cool},
		} {
			t.Run(tc.name, func(t *testing.T) {
				s := New("local/model")
				tt := ui.NewTester(func(c *ui.Context) {
					theme.Apply(c, "", mode)
					ui.Column(c).Fill().Padding(40).AlignItems(ui.Start).Background(theme.Of(c).Background).
						Children(func() { tc.view(s, c) })
				}, width, height)
				tt.SetLocale("en")
				tt.Frame()
				capsule, ok := tt.Access(tc.label)
				if !ok {
					t.Fatalf("capsule %q not drawn; texts=%v", tc.label, tt.Texts())
				}
				img := tt.Image()
				scale := float32(img.Bounds().Dx()) / width
				at := func(x, y float32) color.RGBA { return img.RGBAAt(int(x*scale), int(y*scale)) }
				mid := capsule.Bounds.Y + capsule.Bounds.H/2
				// Inside the round end, clear of the border, icon and text,
				// against the page just left of the capsule.
				got, ground := at(capsule.Bounds.X+5, mid), at(capsule.Bounds.X-10, mid)
				washed := far(got, ground, 3)
				if washed != (tc.lean != none) {
					t.Fatalf("washed=%v, want %v: got %v over %v", washed, tc.lean != none, got, ground)
				}
				if tc.lean == none {
					return
				}
				// Amber is red over blue; Info blue the other way.
				lean := int(got.R) - int(got.B) - (int(ground.R) - int(ground.B))
				if (lean > 0) != (tc.lean == warm) {
					t.Fatalf("wash %v over %v leans the wrong way", got, ground)
				}
			})
		}
	}
}

func far(a, b color.RGBA, d int) bool {
	abs := func(v int) int { return max(v, -v) }
	return abs(int(a.R)-int(b.R)) > d || abs(int(a.G)-int(b.G)) > d || abs(int(a.B)-int(b.B)) > d
}
