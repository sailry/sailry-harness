package kit

import (
	"fmt"
	"image"
	"math"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// inkRows is the ink of each device row of img across r, from 6 DIPs above
// it to 6 below: how far the row's pixels are from the color beside r's
// left, summed. The first row is device row first.
func inkRows(img *image.RGBA, scale float32, r ui.Rect) (first int, rows []float64) {
	bg := img.RGBAAt(int((r.X-3)*scale), int((r.Y+r.H/2)*scale))
	x0, x1 := int(r.X*scale), int(math.Ceil(float64((r.X+r.W)*scale)))
	first = int((r.Y - 6) * scale)
	for y := first; y < int(math.Ceil(float64((r.Y+r.H+6)*scale))); y++ {
		sum := 0.0
		for x := x0; x < x1; x++ {
			p := img.RGBAAt(x, y)
			sum += math.Abs(float64(p.R)-float64(bg.R)) + math.Abs(float64(p.G)-float64(bg.G)) + math.Abs(float64(p.B)-float64(bg.B))
		}
		rows = append(rows, sum)
	}
	return first, rows
}

// inkCentre is the height, in DIPs, of the middle of the ink in r by its
// weight: the middle of the capitals of a text of capitals, and of the
// body of ideographs.
func inkCentre(img *image.RGBA, scale float32, r ui.Rect) float64 {
	first, rows := inkRows(img, scale, r)
	var sum, weighted float64
	for i, v := range rows {
		sum += v
		weighted += v * (float64(first+i) + 0.5)
	}
	return weighted / sum / float64(scale)
}

// inkMiddle is the height, in DIPs, halfway between the ink's first row and
// its last in r: the middle of an icon, whatever is drawn inside its
// outline.
func inkMiddle(img *image.RGBA, scale float32, r ui.Rect) float64 {
	first, rows := inkRows(img, scale, r)
	peak := 0.0
	for _, v := range rows {
		peak = max(peak, v)
	}
	top, bottom := -1, -1
	for i, v := range rows {
		if v > peak/20 {
			if top < 0 {
				top = i
			}
			bottom = i
		}
	}
	return (float64(first+top) + float64(first+bottom) + 1) / 2 / float64(scale)
}

// A row's texts are level with its icon (Level): the middle of a Latin
// label's capitals, of a Chinese label's ideographs and of a mono detail's
// capitals are within half a DIP of the icon's middle and of each other, at
// scale 1 and 2, in a centred row and on the first line of a row that wraps
// (LevelTop). Unlevelled, as MyGo centres the line box, they are not.
//
// The label laid out as a sweep takes it, a RichText of its span, and the
// detail, a RichText a DIP smaller in the mono font, are levelled from
// their spans (LevelSpan, LevelSpanTop): their line starts from the
// theme's 14 whatever their size. A row of another text size, as the
// transcript's at 12 and 18, has an icon 2 DIPs larger than its text and is
// 10 higher; there the capitals' baseline, a whole DIP, is up to half a
// DIP from their middle's, so the texts are within one DIP of the icon.
func testLevel(t *testing.T) {
	row := func(c *ui.Context, key, label string, size float32, level, wrap bool) {
		p := theme.Of(c)
		height, mark := size+10, size+2
		r := ui.Row(c.Key(key)).Gap(8).MaxWidthPercent(100)
		if wrap {
			r.FillWidth().MinHeight(height).AlignItems(ui.Start)
		} else {
			r.Height(height)
		}
		r.Children(func() {
			icon := ui.Icon(c, icons.Help).FontSize(mark).Shrink(0).TextColor(p.Foreground).Label(key + " icon")
			text := ui.Text(c, label).FontSize(size).Shrink(0).TextColor(p.Foreground).Label(key + " label")
			run := ui.Span{Text: label, Size: size, Color: p.Foreground}
			swept := ui.RichText(c, run).SingleLine().Shrink(0).Label(key + " swept")
			span := ui.Span{Text: "HIDE", Size: size - 1, Font: "monospace", Color: p.Foreground}
			detail := ui.RichText(c, span).SingleLine().Label(key + " detail")
			switch {
			case wrap:
				icon.Margin((height-mark)/2, 0, 0, 0)
				LevelTop(c, text, size, height)
				LevelSpanTop(c, swept, run, size, height)
				LevelSpanTop(c, detail, span, size, height)
			case level:
				Level(c, text.SingleLine(), size)
				LevelSpan(c, swept, run, size)
				LevelSpan(c, detail, span, size)
			}
		})
	}
	for _, size := range []float32{12, 14, 18} {
		limit := 0.5
		if size != 14 {
			limit = 1
		}
		for _, scale := range []float32{1, 2} {
			for _, label := range []string{"HIDE", "田口"} {
				tt := ui.NewTester(func(c *ui.Context) {
					theme.Apply(c, "", theme.Dark)
					ui.Column(c).Fill().Background(theme.Of(c).Background).Padding(20).Gap(12).Children(func() {
						row(c, "plain", label, size, false, false)
						row(c, "level", label, size, true, false)
						row(c, "wrap", label, size, true, true)
					})
				}, 400, 160)
				tt.SetScale(scale)
				tt.Frame()
				img := tt.Image()
				at := func(name string, measure func(*image.RGBA, float32, ui.Rect) float64) float64 {
					r, ok := tt.Find(name)
					if !ok {
						t.Fatalf("no %q", name)
					}
					return measure(img, scale, r)
				}
				var report string
				for _, key := range []string{"plain", "level", "wrap"} {
					icon, text, swept, detail := at(key+" icon", inkMiddle), at(key+" label", inkCentre), at(key+" swept", inkCentre), at(key+" detail", inkCentre)
					report += fmt.Sprintf(" %s: icon %.2f, label %+.2f, swept %+.2f, detail %+.2f;", key, icon, text-icon, swept-icon, detail-icon)
					off := max(math.Abs(text-icon), math.Abs(swept-icon), math.Abs(detail-icon), math.Abs(text-detail), math.Abs(text-swept))
					if key != "plain" && off > limit {
						t.Errorf("size %v, scale %v, %q, %s row: icon %.2f, label %.2f, swept %.2f, detail %.2f: %.2f DIPs apart", size, scale, label, key, icon, text, swept, detail, off)
					}
				}
				t.Logf("size %v, scale %v, %q:%s", size, scale, label, report)
			}
		}
	}
}

// A code block's Copy is an icon button with no text, named for its tooltip
// and assistive technology; it copies the code and says so in a toast.
func testCodeCopy(t *testing.T) {
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Padding(20).Children(func() {
			Markdown(c, "```go\nfmt.Println(1)\n```", MarkdownProps{})
		})
		ToastHost(c)
	}, 400, 300)
	tt.SetLocale("en")
	tt.Frame()
	name, copied := i18n.In("en", "content_copy"), i18n.In("en", "content_copied")
	b, ok := tt.AccessByRole(ui.RoleButton, name)
	if !ok {
		t.Fatalf("no button named %q", name)
	}
	if b.Bounds.W != 24 || b.Bounds.H != 24 {
		t.Errorf("Copy is %vx%v, want the 24 DIP icon button", b.Bounds.W, b.Bounds.H)
	}
	// The name is the button's alone: no text beside the icon repeats it.
	named := 0
	for _, s := range tt.Texts() {
		if s == name {
			named++
		}
	}
	if named != 1 || tt.HasText(copied) {
		t.Errorf("Copy shows a text: %q", tt.Texts())
	}
	tt.ClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
	tt.Frame()
	if got := tt.Clipboard(); got != "fmt.Println(1)\n" {
		t.Errorf("Copy wrote %q", got)
	}
	if !tt.HasText(copied) {
		t.Errorf("no toast: %q", tt.Texts())
	}
}

// A failure or an uncertain outcome with something to run again offers
// Retry on its toast; a conflict and a failure without it do not.
func testRecover(t *testing.T) {
	var show func(*ui.Context)
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		if show != nil {
			show(c)
			show = nil
		}
		ToastHost(c)
	}, 500, 400)
	tt.SetLocale("en")
	retry := i18n.In("en", "chat_retry")
	ran := 0
	again := func() { ran++ }
	for _, sample := range []struct {
		outcome Outcome
		again   func()
		offered bool
	}{{Failed, again, true}, {Uncertain, again, true}, {Conflict, again, false}, {Failed, nil, false}, {Done, again, false}} {
		message := fmt.Sprintf("outcome %d %v", sample.outcome, sample.again != nil)
		show = func(c *ui.Context) { Recover(c, sample.outcome, message, sample.again) }
		tt.Frame()
		tt.Advance(time.Second)
		if !tt.HasText(message) {
			t.Fatalf("no toast %q: %q", message, tt.Texts())
		}
		b, ok := tt.AccessByRole(ui.RoleButton, retry)
		if ok != sample.offered {
			t.Fatalf("%q: Retry offered %v, want %v", message, ok, sample.offered)
		}
		if !ok {
			continue
		}
		before := ran
		tt.ClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
		tt.Frame()
		tt.Advance(time.Second)
		if ran != before+1 || tt.HasText(message) {
			t.Fatalf("%q: Retry ran %d times and left %q", message, ran-before, tt.Texts())
		}
	}
}
