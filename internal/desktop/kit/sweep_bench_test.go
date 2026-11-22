package kit

import (
	"runtime"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func BenchmarkCommandSweepFrames(b *testing.B) {
	for _, animated := range []bool{false, true} {
		name := "still"
		if animated {
			name = "sweep"
		}
		b.Run(name, func(b *testing.B) {
			const frame = time.Second / FPS
			builds, frames := 0, 0
			tt := ui.NewTester(func(c *ui.Context) {
				builds++
				theme.Apply(c, "", theme.Dark)
				p := theme.Of(c)
				var sw Sweep
				row := ui.Row(c).Width(220).Height(24).Gap(8).Background(p.Background)
				row.Children(func() {
					for i, span := range []ui.Span{
						{Text: "Command", Size: 14, Color: p.MutedForeground},
						{Text: "go test ./internal/desktop/conversation/... ./internal/desktop/kit/...", Font: "monospace", Size: 13, Color: p.MutedForeground.Alpha(0.65)},
					} {
						e := LevelSpan(c, ui.RichText(c, span).SingleLine(), span, 14)
						if i == 0 {
							e.Shrink(0)
						} else {
							e.Grow(1).Shrink(1).MinWidth(0)
						}
						if animated {
							sw.Text(c, e, span)
						}
					}
				})
				sw.Frame(row)
				// Both cases repaint at the same rate without rebuilding the view.
				ui.Box(c).Absolute().Size(1, 1).Draw(func(p *ui.Painter, _ ui.Rect) {
					frames++
					p.AnimationFrameRate(FPS)
				})
			}, 220, 30)
			tt.SetLocale("en")
			tt.Advance(3 * sweepPeriod)
			initialBuilds := builds
			frames = 0
			var before, after runtime.MemStats
			runtime.GC()
			runtime.ReadMemStats(&before)
			b.ReportAllocs()
			for b.Loop() {
				tt.Advance(frame)
			}
			runtime.GC()
			runtime.ReadMemStats(&after)
			runtime.KeepAlive(tt)
			if builds != initialBuilds || frames == 0 {
				b.Fatalf("view built %d times during %d animation frames", builds-initialBuilds, frames)
			}
			b.ReportMetric(float64(frames)/float64(b.N), "frame/op")
			b.ReportMetric(float64(int64(after.HeapAlloc)-int64(before.HeapAlloc)), "live-heap-delta-B")
		})
	}
}
