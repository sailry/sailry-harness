package conversation_test

import (
	"math"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// sized shows turns at a message text size. Its second result measures a
// text at a size, as the frame that drew the turns does.
func sized(size float32, turns []conversation.Turn) (*ui.Tester, func(text string, at float32) float32) {
	s := conversation.New()
	type probe struct {
		text string
		at   float32
	}
	var asked *probe
	var width float32
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		kit.SetPreferences(c, kit.Preferences{MessageTextSize: uint8(size)})
		if asked != nil {
			width, _ = c.MeasureText(0, ui.Span{Text: asked.text, Size: asked.at})
		}
		ui.Column(c).Fill().Children(func() { conversation.View(c, s, "sizes", turns, nil, noActions{}) })
	}, chartStageWidth, 700)
	tt.SetLocale("en")
	tt.Frame()
	tt.Frame()
	tt.Advance(time.Second)
	return tt, func(text string, at float32) float32 {
		asked = &probe{text, at}
		tt.Frame()
		asked = nil
		return width
	}
}

// checkContentSizes is the proof that what a reply draws beside its text
// follows the message text size at 12, 14 and 18: a chart's axis labels and
// legend are three DIPs under it and its plot as much higher as the
// text is larger, a pie's legend is two under it on rows six higher, the
// pie itself and the tip over a mark keep their size, and a compaction's
// row is the size and ten high.
func checkContentSizes(t *testing.T) {
	chart := func(src string) []conversation.Turn {
		return []conversation.Turn{{ID: "chart", User: "Draw it", Reply: conversation.Reply{Status: conversation.Completed,
			Blocks: []conversation.Block{{Kind: conversation.TextBlock, Text: fence(src)}}}}}
	}
	near := func(got, want float32) bool { return math.Abs(float64(got-want)) <= 0.75 }
	sizes := []float32{12, 14, 18}
	t.Run("xychart", func(t *testing.T) {
		plots, tips := map[float32]float32{}, map[float32]float32{}
		for _, size := range sizes {
			tt, measure := sized(size, chart(lineSource))
			plot, ok := tt.AccessByRole(ui.RoleImage, "Week-4 retention")
			if !ok {
				t.Fatalf("size %v: no chart: %q", size, tt.Texts())
			}
			plots[size] = plot.Bounds.H
			// A category of the axis and a series of the legend.
			for _, text := range []string{"W36", "Free"} {
				label, shown := tt.Find(text)
				if want := measure(text, size-3); !shown || !near(label.W, want) {
					t.Errorf("size %v: %q is %v wide (shown %v), want %v as text of %v", size, text, label.W, shown, want, size-3)
				}
			}
			b := plot.Bounds
			tt.Move(b.X+b.W*0.92, b.Y+b.H*0.5)
			tt.Frame()
			value, shown := tt.Find("65")
			if !shown {
				t.Fatalf("size %v: no tip under the pointer: %q", size, tt.Texts())
			}
			tips[size] = value.H
		}
		for _, size := range []float32{12, 18} {
			if want := plots[14] * size / 14; !near(plots[size], want) {
				t.Errorf("size %v: the plot is %v high, want %v as %v at 14", size, plots[size], want, plots[14])
			}
			if tips[size] != tips[14] {
				t.Errorf("size %v: the tip's value is %v high, %v at 14", size, tips[size], tips[14])
			}
		}
	})
	t.Run("pie", func(t *testing.T) {
		for _, size := range sizes {
			tt, measure := sized(size, chart(pieSource))
			wheel, ok := tt.AccessByRole(ui.RoleImage, "流量来源")
			if !ok || wheel.Bounds.W != 180 || wheel.Bounds.H != 180 {
				t.Fatalf("size %v: the pie is %+v (%v)", size, wheel.Bounds, ok)
			}
			first, ok1 := tt.Find("搜索")
			second, ok2 := tt.Find("直接访问")
			if !ok1 || !ok2 {
				t.Fatalf("size %v: no legend: %q", size, tt.Texts())
			}
			if want := measure("直接访问", size-2); !near(second.W, want) {
				t.Errorf("size %v: a legend label is %v wide, want %v as text of %v", size, second.W, want, size-2)
			}
			if step := second.Y - first.Y; !near(step, size+6) {
				t.Errorf("size %v: the legend's rows are %v apart, want %v", size, step, size+6)
			}
		}
	})
	// A compaction's row among the turns: what it adds between the reply
	// above it and the message below it grows a DIP for each of the text.
	t.Run("compaction", func(t *testing.T) {
		reply := func(text string) conversation.Reply {
			return conversation.Reply{Status: conversation.Completed, Blocks: []conversation.Block{{Kind: conversation.TextBlock, Text: text}}}
		}
		label := i18n.In("en", "chat_compacted")
		between := func(size float32, compacted bool) float32 {
			turns := []conversation.Turn{{ID: "above", User: "First", Reply: reply("Above")}}
			if compacted {
				turns = append(turns, conversation.Turn{ID: "compaction", Compaction: &conversation.Compaction{State: conversation.Compacted}})
			}
			turns = append(turns, conversation.Turn{ID: "below", User: "Below", Reply: reply("Done")})
			tt, measure := sized(size, turns)
			above, ok1 := tt.Find("Above")
			below, ok2 := tt.Find("Below")
			if !ok1 || !ok2 {
				t.Fatalf("size %v: the turns show %q", size, tt.Texts())
			}
			if row, shown := tt.Find(label); compacted && (!shown || !near(row.W, measure(label, size))) {
				t.Fatalf("size %v: the compaction's label is %v wide (shown %v): %q", size, row.W, shown, tt.Texts())
			}
			return below.Y - above.Y - above.H
		}
		rows := map[float32]float32{}
		for _, size := range sizes {
			rows[size] = between(size, true) - between(size, false)
		}
		for _, size := range []float32{12, 18} {
			// A line of text alone would grow by its line height, more than a DIP each.
			if got, want := rows[size]-rows[14], size-14; math.Abs(float64(got-want)) > 0.25 {
				t.Errorf("size %v: the compaction's row takes %v, %v at 14: it grew %v, want %v", size, rows[size], rows[14], got, want)
			}
		}
	})
}
