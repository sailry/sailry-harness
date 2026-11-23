package kit

import (
	"bytes"
	"fmt"
	"math"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/plugins/loading"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/uitest"
)

// MyGo repaints the Draw callbacks that asked for another paint, including
// ones waiting on After. This witness is due long after the sample ends: it
// observes paints driven by other components, without requesting a refresh.
func witnessPaints(c *ui.Context, paints *int) {
	ui.Box(c.Key("paint-witness")).Absolute().Size(1, 1).Draw(func(p *ui.Painter, _ ui.Rect) {
		*paints++
		p.After(24 * time.Hour)
	})
}

func nextMotionPhase(tt *ui.Tester, period, phase time.Duration) {
	now := tt.Now()
	tt.Advance(now.Truncate(period).Add(period + phase).Sub(now))
}

func TestAgentRestsUntilNextRevolution(t *testing.T) {
	uitest.Parallel(t)
	turning, paints := true, 0
	tt := uitest.New(func(c *ui.Context) {
		ui.Column(c).Fill().Children(func() {
			Agent(c, "agent-7", ui.RGB(89, 127, 251), turning, 32)
			witnessPaints(c, &paints)
		})
	}, 80, 60, 1, "", ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	nextMotionPhase(tt, time.Second, 500*time.Millisecond)
	still := bytes.Clone(tt.Image().Pix)
	paints = 0
	tt.Advance(499 * time.Millisecond)
	if paints != 0 || tt.FramePending() {
		t.Fatalf("resting agent: %d paints, refresh pending: %v", paints, tt.FramePending())
	}
	tt.Advance(101 * time.Millisecond)
	if paints == 0 || !tt.FramePending() || bytes.Equal(still, tt.Image().Pix) {
		t.Fatal("agent did not wake and rotate")
	}
	nextMotionPhase(tt, time.Second, 500*time.Millisecond)
	if !bytes.Equal(still, tt.Image().Pix) {
		t.Fatal("the revolution did not end in its resting pose")
	}
	turning = false
	tt.Frame()
	paints = 0
	tt.Advance(2 * time.Second)
	if paints != 0 || tt.FramePending() {
		t.Fatal("completed agent retained its wake")
	}
}

func TestSweepSleepsBetweenPasses(t *testing.T) {
	uitest.Parallel(t)
	paints := 0
	tt := uitest.New(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() {
			var sw Sweep
			row := ui.Row(c).Width(400)
			row.Children(func() {
				span := ui.Span{Text: "Working", Size: 14, Color: theme.Of(c).Foreground}
				sw.Text(c, ui.RichText(c, span).SingleLine(), span)
			})
			sw.Frame(row)
			witnessPaints(c, &paints)
		})
	}, 400, 32, uitest.Scale, "", ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	nextMotionPhase(tt, sweepPeriod, sweepPeriod*7/8)
	paints = 0
	tt.Advance(sweepPeriod / 4)
	if paints != 0 || tt.FramePending() {
		t.Fatal("sweep repainting after it has left the label")
	}
	tt.Advance(sweepPeriod / 4)
	if paints == 0 || !tt.FramePending() {
		t.Fatal("sweep did not wake on re-entering the label")
	}
}

func TestShortSweepCrossesGlyphsOnWideRows(t *testing.T) {
	uitest.Parallel(t)
	for _, style := range []theme.Mode{theme.Light, theme.Dark} {
		tt := uitest.New(func(c *ui.Context) {
			theme.Apply(c, "", style)
			p := theme.Of(c)
			row := ui.Row(c).Width(800).Background(p.Background)
			var sw Sweep
			row.Children(func() {
				span := ui.Span{Text: "Run", Size: 14, Color: p.Foreground}
				sw.Text(c, ui.RichText(c, span).SingleLine(), span)
			})
			sw.Frame(row)
		}, 800, 32, 1, "", ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		nextMotionPhase(tt, sweepPeriod, 0)
		still := bytes.Clone(tt.Image().Pix)
		nextMotionPhase(tt, sweepPeriod, sweepPeriod/2)
		if bytes.Equal(still, tt.Image().Pix) {
			t.Fatalf("%v: sweep did not change the short label at mid-pass", style)
		}
	}
}

func TestSweepSoftBandPreservesBase(t *testing.T) {
	uitest.Parallel(t)
	for _, style := range []theme.Mode{theme.Light, theme.Dark} {
		for _, width := range []int{180, 480} {
			t.Run(fmt.Sprintf("%v/%d", style, width), func(t *testing.T) {
				uitest.Parallel(t)
				animated := false
				tt := uitest.New(func(c *ui.Context) {
					theme.Apply(c, "", style)
					p := theme.Of(c)
					ui.Column(c).Fill().Background(p.Background).Children(func() {
						span := ui.Span{Text: strings.Repeat("m", 80), Font: "monospace", Size: 14, Color: p.MutedForeground}
						e := ui.RichText(c, span).SingleLine().FillWidth()
						if animated {
							var sw Sweep
							sw.Text(c, e, span)
						}
					})
				}, width, 32, 1, "", ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
				tt.Frame()
				base := bytes.Clone(tt.Image().Pix)
				animated = true
				tt.SetPreferences(ui.Preferences{ReduceMotion: true})
				tt.Frame()
				if !bytes.Equal(base, tt.Image().Pix) || tt.FramePending() {
					t.Fatal("reduced motion changed the base text or requested animation")
				}
				tt.SetPreferences(ui.Preferences{})
				tt.Frame()
				nextMotionPhase(tt, sweepPeriod, 0)
				tt.Frame()
				if !bytes.Equal(base, tt.Image().Pix) {
					t.Fatal("resting sweep changed the theme's base text")
				}
				nextMotionPhase(tt, sweepPeriod, sweepPeriod/2)
				tt.Frame()
				image := tt.Image()
				lit := bytes.Clone(image.Pix)
				left, right, centerPeak, edgePeak := width, -1, 0, 0
				for y := 0; y < image.Bounds().Dy(); y++ {
					for x := 0; x < width; x++ {
						o := y*image.Stride + x*4
						delta := 0
						for channel := range 3 {
							delta = max(delta, int(math.Abs(float64(int(lit[o+channel])-int(base[o+channel])))))
						}
						if delta == 0 {
							continue
						}
						if bytes.Equal(base[o:o+3], base[:3]) {
							t.Fatal("highlight escaped the visible glyphs")
						}
						left, right = min(left, x), max(right, x)
						distance := math.Abs(float64(x)+0.5-float64(width)/2) / float64(width)
						if distance < 0.05 {
							centerPeak = max(centerPeak, delta)
						} else if distance > 0.2 && distance < 0.3 {
							edgePeak = max(edgePeak, delta)
						}
					}
				}
				if band := float64(right-left+1) / float64(width); band < 0.4 || band > 0.65 {
					t.Fatalf("highlight width = %.2f of the label; want a broad, bounded band", band)
				}
				if edgePeak == 0 || float64(edgePeak) >= float64(centerPeak)*0.65 {
					t.Fatalf("highlight edge = %d, center = %d; want soft edges", edgePeak, centerPeak)
				}
				tt.Advance(1500 * time.Millisecond)
				tt.Frame()
				if !bytes.Equal(lit, tt.Image().Pix) {
					t.Fatal("sweep did not repeat after 1.5 seconds")
				}
				tt.SetPreferences(ui.Preferences{ReduceMotion: true})
				tt.Frame()
				tt.Advance(2 * sweepPeriod)
				if !bytes.Equal(base, tt.Image().Pix) || tt.FramePending() {
					t.Fatal("reduced motion retained the highlight or its animation")
				}
			})
		}
	}
}

func TestSweepRetainedLabelMatchesRichText(t *testing.T) {
	uitest.Parallel(t)
	var label string
	build := func(explicitWeight bool) *ui.Tester {
		return uitest.New(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			span := ui.Span{Text: label, Size: 14, Color: theme.Of(c).Foreground}
			e := ui.RichText(c, span).SingleLine()
			if explicitWeight {
				span.Weight = 400 // the default, painted by the styled-span path
			}
			var sw Sweep
			sw.Text(c, e, span)
		}, 160, 32, 1, "", ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	}
	plain, rich := build(false), build(true)
	for _, text := range []string{"Checking references", "Neue Datei", "中文と emoji 🌊", "تقرير 42"} {
		label = text
		for _, tt := range []*ui.Tester{plain, rich} {
			tt.Frame()
			nextMotionPhase(tt, sweepPeriod, sweepPeriod/2)
			tt.Frame()
		}
		if !bytes.Equal(plain.Image().Pix, rich.Image().Pix) {
			t.Fatalf("retained glyphs differ from RichText after setting %q", text)
		}
	}
}

func TestMotionAtViewportEdge(t *testing.T) {
	uitest.Parallel(t)
	for _, item := range []struct {
		name string
		draw func(*ui.Context) ui.Element
	}{
		{"sweep", func(c *ui.Context) ui.Element { return Shimmer(c, "Working", 14, ui.RGB(89, 127, 251)) }},
		{"agent", func(c *ui.Context) ui.Element { return Identicon(c, "agent-7", true, 20) }},
		{"working", func(c *ui.Context) ui.Element { return Indicator(c, Working, 20) }},
		{"searching", func(c *ui.Context) ui.Element { return Indicator(c, Searching, 20) }},
		{"composing", func(c *ui.Context) ui.Element { return Indicator(c, Composing, 20) }},
		{"waiting", func(c *ui.Context) ui.Element { return Indicator(c, Waiting, 20) }},
	} {
		t.Run(item.name, func(t *testing.T) {
			uitest.Parallel(t)
			y, mounted, paints := float32(1), true, 0
			tt := uitest.New(func(c *ui.Context) {
				ui.Column(c).Fill().Children(func() {
					ui.Box(c).Size(100, 30).Clip().Children(func() {
						if mounted {
							item.draw(c).Absolute().Top(y).Left(0)
						}
					})
					witnessPaints(c, &paints)
				})
			}, 120, 80, uitest.Scale, "", ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
			check := func(wantMotion bool) {
				t.Helper()
				tt.Frame()
				paints = 0
				tt.Advance(2 * time.Second)
				if (paints > 0) != wantMotion {
					t.Fatalf("offset %v, mounted %v: %d paints, want motion %v", y, mounted, paints, wantMotion)
				}
			}
			check(true)
			y = 30 // touching the viewport, within MyGo's 4-DIP paint margin
			check(false)
			y = 28
			check(true)
			tt.SetPreferences(ui.Preferences{ReduceMotion: true})
			check(false)
			tt.SetPreferences(ui.Preferences{})
			y = 1
			check(true)
			mounted = false
			check(false)
		})
	}
}

// A Kit-level replay, not a whole workspace. All four controls retain the
// same labels, sizes and glyphs. Turn 500 of a 1000-row virtual transcript
// contains a running group and its two child rows; a fold keeps its summary.
// One operation advances six simulated seconds (whole sweep, spin and swirl
// periods), without forced frames or an observer in the timed interval.
// This file also builds against 04b89338f: copy it unmodified into that
// checkout and use -run '^$' -bench '^BenchmarkMotionReplay$' for a baseline.
func BenchmarkMotionReplay(b *testing.B) {
	for _, state := range []string{"visible", "scrolled", "folded", "switched", "panel-open", "panel-closed"} {
		for _, group := range []struct {
			name         string
			sweep, icons bool
		}{
			{"both", true, true}, {"no-sweep", false, true},
			{"no-icons", true, false}, {"neither", false, false},
		} {
			b.Run(state+"/"+group.name, func(b *testing.B) {
				const active, turns = 500, 1000
				const window = 6 * time.Second
				list := ui.ListState{}
				paints, sample := 0, true
				mounted, selected := false, "running"
				panel := state == "panel-open" || state == "panel-closed"
				sidebarOpen := panel
				var texts [turns]string
				for i := range texts {
					texts[i] = fmt.Sprintf("Turn %d: checked the workspace.", i)
				}
				tt := ui.NewTester(func(c *ui.Context) {
					theme.Apply(c, "", theme.Light)
					p := theme.Of(c)
					activity := func() {
						for i, label := range []string{"Waiting for 2 agents", "Checking references", "Reading the results"} {
							if state == "folded" && i > 0 {
								break
							}
							var sw Sweep
							row := ui.Row(c.Key(i)).Height(24).Gap(6)
							row.Children(func() {
								if i != 0 {
									Agent(c, label, AgentColor(c, label), group.icons, 16)
								} else if group.icons {
									Indicator(c, Working, 16)
								} else {
									loading.Swirl(c, loading.Options{Size: 16, Color: p.MutedForeground, Paused: true}).ClipInset(0, 0, 0, 0, 0)
								}
								span := ui.Span{Text: label, Size: 14, Color: p.MutedForeground}
								e := ui.RichText(c, span).SingleLine()
								if group.sweep {
									sw.Text(c, e, span)
								}
							})
							sw.Frame(row)
						}
					}
					ui.Row(c).Fill().Children(func() {
						ui.List(c.Key(selected), &list, turns, func(i int) {
							ui.Column(c.Key(i)).Children(func() {
								ui.Text(c, texts[i]).FontSize(14).SingleLine()
								if i == active && mounted && selected == "running" && !panel {
									activity()
								}
							})
						}).Grow(1).MinWidth(0).FillHeight()
						if mounted && sidebarOpen {
							ui.Column(c.Key("sidebar")).Width(180).Children(activity)
						}
						if sample {
							witnessPaints(c, &paints)
						}
					})
				}, 480, 160)
				nextMotionPhase(tt, window, 0)
				mounted = true
				list.ScrollTo(active, ui.Center)
				tt.Frame()
				if !tt.HasText("Waiting for 2 agents") {
					b.Fatal("active turn not mounted at the initial cut")
				}
				cut := active
				switch state {
				case "scrolled":
					cut = active - 100
				case "switched":
					selected = "completed"
				case "panel-closed":
					sidebarOpen = false
				}
				list.ScrollTo(cut, ui.Center)
				tt.Frame()
				first, last := list.Visible()
				if cut < first || cut > last || state == "scrolled" && first <= active && active <= last {
					b.Fatalf("cut %d with active turn %d: viewport %d..%d", cut, active, first, last)
				}
				visible := state == "visible" || state == "panel-open" || state == "folded"
				inView := func(label string) bool {
					r, ok := tt.Find(label) // Find also reports clipped boxes.
					return ok && r.X < 480 && r.Y < 160 && r.X+r.W > 0 && r.Y+r.H > 0
				}
				if inView("Waiting for 2 agents") != visible || inView("Checking references") != (visible && state != "folded") {
					b.Fatalf("unexpected visible activity at cut %d (viewport %d..%d)", cut, first, last)
				}
				paints = 0
				tt.Advance(window)
				if (paints != 0) != (visible && (group.icons || group.sweep)) {
					b.Fatalf("unexpected paint count at replay cut: %d", paints)
				}
				rate := float64(paints) / window.Seconds()
				sample = false
				tt.Frame()
				b.ReportAllocs()
				for b.Loop() {
					tt.Advance(window)
				}
				b.ReportMetric(rate, "paints/sim-s")
			})
		}
	}
}
