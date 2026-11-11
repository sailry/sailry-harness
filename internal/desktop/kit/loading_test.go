package kit

import (
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/plugins/loading"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The loading marks have their sizes and the progress role, ask for frames
// while they show, and for none with Reduce Motion, out of view or not
// built; while they move nothing is rebuilt.
func testLoadingMarks(t *testing.T) {
	show, marks, scrolled, builds := true, 1, false, 0
	tt := ui.NewTester(func(c *ui.Context) {
		builds++
		theme.Apply(c, "", theme.Dark)
		p := theme.Of(c)
		ui.Scroll(c.Key("page")).Fill().Background(p.Background).Padding(20).Gap(8).Children(func() {
			if scrolled {
				ui.Box(c).Height(600).Shrink(0)
			}
			if !show {
				return
			}
			for i := range marks {
				Loader(c.Key(i), 12).Label(fmt.Sprintf("loader %d", i))
			}
			Busy(c).Label("busy")
			for st, name := range []string{Breathing: "breathing", Working: "working", Searching: "searching", Composing: "composing", Waiting: "waiting"} {
				Indicator(c.Key(name), Thinking(st), 16).Label(name)
			}
		})
	}, 300, 300)
	settle := func() {
		tt.Frame()
		tt.Frame()
	}
	settle()
	for name, size := range map[string]float32{"loader 0": 12, "busy": BusySize, "searching": 16, "working": 16, "breathing": 16, "composing": 16, "waiting": 16} {
		info, ok := tt.AccessByRole(ui.RoleProgress, name)
		if !ok || info.Bounds.W != size || info.Bounds.H != size {
			t.Errorf("%s: %+v, want a progress mark %v across", name, info.Bounds, size)
		}
	}
	if !tt.FramePending() {
		t.Fatal("the marks ask for no frame")
	}
	// A second of twenty loaders, in steps finer than any display's
	// refresh: the frame changes at the display's rate, and nothing is
	// rebuilt.
	marks = 20
	settle()
	builds = 0
	frames, last := 0, append([]byte(nil), tt.Image().Pix...)
	for range 480 {
		tt.Advance(time.Second / 480)
		if now := tt.Image().Pix; string(now) != string(last) {
			frames++
			last = append(last[:0], now...)
		}
	}
	t.Logf("20 loaders and the other marks for 1 s: %d frames, %d builds", frames, builds)
	if builds != 0 || frames < 30 {
		t.Errorf("a second of marks: %d frames, %d builds; want frames that only paint", frames, builds)
	}
	marks = 1
	scrolled = true
	settle()
	if tt.FramePending() {
		t.Error("marks out of view ask for frames")
	}
	scrolled = false
	tt.SetPreferences(ui.Preferences{ReduceMotion: true})
	settle()
	if tt.FramePending() {
		t.Error("the marks move with Reduce Motion")
	}
	still := append([]byte(nil), tt.Image().Pix...)
	tt.Advance(700 * time.Millisecond)
	tt.Frame()
	if string(still) != string(tt.Image().Pix) {
		t.Error("the marks' pose changes with Reduce Motion")
	}
	tt.SetPreferences(ui.Preferences{})
	settle()
	if !tt.FramePending() {
		t.Error("the marks do not resume")
	}
	show = false
	settle()
	if tt.FramePending() {
		t.Error("a view without marks asks for frames")
	}
}

// loadingSheet writes, when $SAILRY_SNAPSHOTS is set, the frames the
// review's contact sheet takes from Kit: the marks considered for a running
// turn beside 14 DIP text at four moments, and the marks chosen in a busy
// button, a code block's header and rows with and without Level.
func loadingSheet(t *testing.T) {
	dir := os.Getenv("SAILRY_SNAPSHOTS")
	if dir == "" {
		t.Skip("SAILRY_SNAPSHOTS is not set")
	}
	save := func(tt *ui.Tester, name string) {
		f, err := os.Create(filepath.Join(dir, name+".png"))
		if err != nil {
			t.Fatal(err)
		}
		defer f.Close()
		if err := png.Encode(f, tt.Image()); err != nil {
			t.Fatal(err)
		}
	}
	considered := map[string]bool{"Swirl": true, "Pulse": true, "Ripple": true, "Linear dots": true, "Bouncing dots": true,
		"Ring": true, "Arc": true, "Circular dots": true, "Classic": true}
	for _, mode := range []theme.Mode{theme.Dark, theme.Light} {
		name := map[theme.Mode]string{theme.Dark: "dark", theme.Light: "light"}[mode]
		tt := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", mode)
			p := theme.Of(c)
			ui.Row(c).Fill().Background(p.Background).Padding(12).Gap(12).Wrap().AlignItems(ui.Start).Children(func() {
				for _, s := range loading.All() {
					if !considered[s.Name] {
						continue
					}
					ui.Column(c.Key(s.Name)).Width(150).Gap(2).Children(func() {
						ui.Text(c, s.Name).FontSize(10).TextColor(p.MutedForeground.Alpha(0.6))
						ui.Row(c).Gap(8).Height(24).Children(func() {
							s.New(c, loading.Options{Size: 16, Color: p.MutedForeground})
							Level(c, ui.Text(c, "正在思考 Thinking").FontSize(14).SingleLine().TextColor(p.MutedForeground), 14)
						})
					})
				}
			})
		}, 820, 170)
		tt.SetScale(2)
		tt.Frame()
		for i := range 4 {
			tt.Advance(140 * time.Millisecond)
			save(tt, fmt.Sprintf("loading-candidates-%s-%d", name, i))
		}

		tt = ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", mode)
			p := theme.Of(c)
			ui.Column(c).Fill().Background(p.Background).Padding(16).Gap(12).Children(func() {
				ui.Row(c.Key("busy")).Gap(8).AlignItems(ui.Center).Children(func() {
					PrimaryBusy(c.Key("primary"), "保存", false, true)
					SecondaryBusy(c.Key("secondary"), "检查更新", true)
					DangerBusy(c.Key("danger"), "删除", true)
					BusyAction(c.Key("action"), "刷新")
					Loader(c, 20)
					Loader(c, 14)
					LoaderIn(c, 12, p.Foreground)
				})
				ui.Row(c.Key("phases")).Gap(24).Children(func() {
					for _, phase := range []struct {
						st    Thinking
						label string
					}{{Searching, "正在思考"}, {Composing, "正在回复"}, {Working, "等待 2 个子代理"}, {Waiting, "等待处理"}} {
						ui.Row(c.Key(phase.label)).Gap(8).Height(24).Children(func() {
							Indicator(c, phase.st, 16)
							Level(c, ui.Text(c, phase.label).FontSize(14).SingleLine().TextColor(p.MutedForeground), 14)
						})
					}
				})
				for _, level := range []bool{false, true} {
					ui.Row(c.Key(level)).Gap(8).Height(24).Children(func() {
						ui.Icon(c, icons.Help).FontSize(16).Shrink(0).TextColor(p.Foreground)
						label := ui.Text(c, "命令 Command").FontSize(14).SingleLine().Shrink(0).TextColor(p.Foreground)
						detail := ui.RichText(c, ui.Span{Text: "make migrate", Size: 13, Font: "monospace", Color: p.MutedForeground}).SingleLine()
						note := "MyGo's centring"
						if level {
							Level(c, label, 14)
							Level(c, detail, 14)
							note = "kit.Level"
						}
						ui.Text(c, note).FontSize(10).TextColor(p.MutedForeground.Alpha(0.6))
					})
				}
				ui.Column(c.Key("code")).Width(420).Children(func() {
					Markdown(c, "```go\nfunc main() {\n\tfmt.Println(\"hello\")\n}\n```", MarkdownProps{Variant: Chat})
				})
			})
		}, 560, 290)
		tt.SetLocale("zh-CN")
		tt.SetScale(2)
		tt.Frame()
		tt.Advance(300 * time.Millisecond)
		save(tt, "loading-kit-"+name)
	}
}
