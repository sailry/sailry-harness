package conversation_test

import (
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// replyTester shows one reply of Markdown, in a mode the test can switch.
func replyTester(text string, mode *theme.Mode) *ui.Tester {
	turns := []conversation.Turn{{User: "Draw it", Reply: conversation.Reply{Blocks: []conversation.Block{{Kind: conversation.TextBlock, Text: text}}}}}
	s := conversation.New()
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", *mode)
		ui.Column(c).Fill().Children(func() { conversation.View(c, s, "visuals", turns, nil, noActions{}) })
	}, 900, 900)
	tt.SetLocale("en")
	tt.Frame()
	tt.Frame()
	return tt
}

// pictures counts the pictures on screen.
func pictures(tt *ui.Tester) int {
	n := 0
	for _, a := range tt.AccessAll() {
		if a.Role == ui.RoleImage {
			n++
		}
	}
	return n
}

// checkMermaidBlocks is the headless smoke of ```mermaid blocks in a
// reply: limu draws a diagram that is not a data chart, a theme switch and
// a narrower window keep it, and a source limu cannot read or a block with
// no end stays code.
func checkMermaidBlocks(t *testing.T) {
	t.Helper()
	mode := theme.Light
	tt := replyTester("Here:\n\n```mermaid\nflowchart LR\nA[\"quote ' </script>\"] --> B\n```\n\nDone.", &mode)
	d, ok := tt.AccessByRole(ui.RoleImage, "flowchart")
	if !ok || d.Bounds.W <= 0 || d.Bounds.H <= 0 {
		t.Fatalf("no diagram: %v %v", ok, d.Bounds)
	}
	if !tt.HasText("Done.") {
		t.Error("the text after the diagram is gone")
	}
	if d.Focusable {
		t.Error("a flowchart is drawn as a data chart")
	}
	mode = theme.Dark
	tt.Frame()
	if dark, ok := tt.AccessByRole(ui.RoleImage, "flowchart"); !ok || dark.Bounds.W != d.Bounds.W || dark.Bounds.H != d.Bounds.H {
		t.Errorf("the theme switch changed the diagram: %v, was %v", dark.Bounds, d.Bounds)
	}
	tt.SetSize(420, 900)
	tt.Frame()
	tt.Frame()
	if narrow, ok := tt.AccessByRole(ui.RoleImage, "flowchart"); !ok || narrow.Bounds.W <= 0 {
		t.Errorf("no diagram in the narrow window: %v", narrow.Bounds)
	}

	for name, src := range map[string]string{
		"unreadable": "```mermaid\nthis is not a diagram\n```",
		"empty":      "```mermaid\n```",
		"open":       "```mermaid\nflowchart LR\nA --> B",
	} {
		mode = theme.Light
		if n := pictures(replyTester(src, &mode)); n != 0 {
			t.Errorf("%s source drew %d pictures, want its code", name, n)
		}
	}
}

// The charts of the proof frames: the demo's bars and lines, a pie and a
// flowchart.
const (
	barSource  = "xychart-beta\n    title \"Weekly active teams\"\n    x-axis [W40, W41, W42, W43, W44, W45]\n    y-axis \"teams\" 0 --> 2400\n    bar [1420, 1510, 1630, 1690, 1840, 2015]\n"
	lineSource = "xychart-beta\n    title \"Week-4 retention\"\n    x-axis [W36, W37, W38, W39, W40, W41]\n    y-axis \"%\" 40 --> 70\n    line \"Pro\" [58, 60, 59, 62, 63, 65]\n    line \"Free\" [47, 49, 48, 51, 52, 54]\n"
	pieSource  = "pie\n    title 流量来源\n    \"搜索\" : 4200\n    \"直接访问\" : 2600\n    \"社交媒体\" : 1900\n    \"邮件\" : 1300\n"
	flowSource = "flowchart LR\n    A[采集事件] --> B{去重}\n    B -->|新团队| C[计入周活]\n    B -->|重复| D[丢弃]\n    C --> E[周报]\n"
)

// The texts around a chart in the proof frames.
const (
	chartIntro = "过去六周的数据如下。"
	chartOutro = "以上数据每周一更新。"
)

func fence(src string) string { return "```mermaid\n" + src + "```" }

// chartStage is a window on one reply in Chinese, which counts its builds.
type chartStage struct {
	tt     *ui.Tester
	builds int
}

const chartStageWidth = 860

// stageCharts shows a reply of blocks between an intro and an outro.
func stageCharts(dark bool, blocks ...conversation.Block) *chartStage {
	st := &chartStage{}
	all := []conversation.Block{{Kind: conversation.TextBlock, Text: chartIntro}}
	all = append(append(all, blocks...), conversation.Block{Kind: conversation.TextBlock, Text: chartOutro})
	turns := []conversation.Turn{{ID: "chart", User: "画出来", Reply: conversation.Reply{Status: conversation.Completed, Blocks: all}}}
	mode := theme.Light
	if dark {
		mode = theme.Dark
	}
	s := conversation.New()
	st.tt = ui.NewTester(func(c *ui.Context) {
		st.builds++
		theme.Apply(c, "", mode)
		ui.Column(c).Fill().Children(func() { conversation.View(c, s, "charts", turns, nil, noActions{}) })
	}, chartStageWidth, 520)
	st.tt.SetLocale("zh-CN")
	st.tt.Frame()
	st.tt.Frame()
	st.tt.Advance(time.Second)
	return st
}

// moves reports whether the window builds or asks for a frame while d
// passes with no input.
func (st *chartStage) moves(d time.Duration) bool {
	builds, asked := st.builds, st.tt.FramePending()
	st.tt.Advance(d)
	return asked || st.tt.FramePending() || st.builds != builds
}

// shot writes the frame to $SAILRY_SNAPSHOTS/conversation-chart-<name>-zh-CN-<mode>.png, when set.
func (st *chartStage) shot(t *testing.T, name string, dark bool) {
	t.Helper()
	dir := os.Getenv("SAILRY_SNAPSHOTS")
	if dir == "" {
		return
	}
	mode := "light"
	if dark {
		mode = "dark"
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	f, err := os.Create(filepath.Join(dir, "conversation-chart-"+name+"-zh-CN-"+mode+".png"))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	if err := png.Encode(f, st.tt.Image()); err != nil {
		t.Fatal(err)
	}
}

// bare fails the test when the visual at b sits in a card: the strip
// beside it, from the text above to the text below, is the page alone,
// with no border across it and no fill behind it.
func (st *chartStage) bare(t *testing.T, b ui.Rect) {
	t.Helper()
	above, ok := st.tt.Find(chartIntro)
	below, ok2 := st.tt.Find(chartOutro)
	if !ok || !ok2 || below.Y <= above.Y {
		t.Fatalf("no text around the visual: %v %v", above, below)
	}
	if above.Y+above.H > b.Y || below.Y < b.Y+b.H {
		t.Errorf("the visual at %v is not between its texts at %v and %v", b, above, below)
	}
	img := st.tt.Image()
	k := float32(img.Bounds().Dx()) / chartStageWidth
	x0, x1 := int((b.X+b.W+12)*k), int((b.X+b.W+52)*k)
	y0, y1 := int(above.Y*k), int((below.Y+below.H)*k)
	if x1 > img.Bounds().Dx() {
		t.Fatalf("no room beside the visual at %v", b)
	}
	page := img.RGBAAt(x0, y0)
	for y := y0; y < y1; y++ {
		for x := x0; x < x1; x++ {
			if img.RGBAAt(x, y) != page {
				t.Fatalf("a card or border at %d,%d beside the visual at %v", x, y, b)
			}
		}
	}
}

// count is how many texts on screen are s.
func (st *chartStage) count(s string) int {
	n := 0
	for _, text := range st.tt.Texts() {
		if text == s {
			n++
		}
	}
	return n
}

// checkCharts is the proof of the charts in a reply, in Chinese, light
// and dark: bars, lines and a pie are drawn natively, small, with no card
// and one title, and show the value under the pointer; a flowchart stays
// limu's picture without a card; nothing moves at rest; and data the
// painters cannot show falls back without a panic.
func checkCharts(t *testing.T) {
	for _, dark := range []bool{false, true} {
		t.Run("bar", func(t *testing.T) {
			const title = "周活跃团队"
			st := stageCharts(dark, conversation.Block{Kind: conversation.VisualBlock, Visual: &conversation.Visual{Title: title, Mermaid: barSource}})
			tt := st.tt
			plot, ok := tt.AccessByRole(ui.RoleImage, title)
			if !ok || !plot.Focusable {
				t.Fatalf("no native chart named by its title: %v %+v", ok, plot)
			}
			if b := plot.Bounds; b.W < 300 || b.W > 560 || b.H < 120 || b.H > 200 {
				t.Errorf("the plot is %v, want within 560 by 200", b)
			}
			// One title: the block's, as a caption in line with the text.
			if st.count(title) != 1 || tt.HasText("Weekly active teams") {
				t.Errorf("titles: %q", tt.Texts())
			}
			caption, _ := tt.Find(title)
			if intro, _ := tt.Find(chartIntro); caption.X != intro.X {
				t.Errorf("the caption starts at %v, the text at %v", caption.X, intro.X)
			}
			for _, want := range []string{"W40", "W45", "teams", "2400"} {
				if !tt.HasText(want) {
					t.Errorf("no %q on the axes: %q", want, tt.Texts())
				}
			}
			st.bare(t, plot.Bounds)
			if tt.HasText("2015") {
				t.Error("a value shows with no pointer on its bar")
			}
			if st.moves(2 * time.Second) {
				t.Error("the chart moves at rest")
			}
			st.shot(t, "bar", dark)

			// The pointer over the last bar shows its value, at once.
			b := plot.Bounds
			tt.Move(b.X+b.W*0.92, b.Y+b.H*0.6)
			tt.Frame()
			if !tt.HasText("2015") || st.count("W45") != 2 {
				t.Fatalf("no value under the pointer: %q", tt.Texts())
			}
			if hot, _ := tt.AccessByRole(ui.RoleImage, title); hot.Value != "W45: 2015" {
				t.Errorf("the chart's value reads %q", hot.Value)
			}
			if st.moves(2 * time.Second) {
				t.Error("the chart moves under a resting pointer")
			}
			st.shot(t, "bar-hover", dark)
			tt.Move(b.X+b.W*0.5, b.Y-60)
			tt.Frame()
			if tt.HasText("2015") {
				t.Error("the value stays after the pointer left")
			}

			// A click takes the focus; the arrows move the pick and read it.
			tt.ClickAt(b.X+b.W*0.08, b.Y+b.H*0.6)
			tt.Frame()
			if !tt.HasText("1420") {
				t.Fatalf("no value on the first bar: %q", tt.Texts())
			}
			tt.Announcements()
			tt.Key(0, ui.KeyRight)
			tt.Frame()
			if said := tt.Announcements(); !tt.HasText("1510") || !slices.Contains(said, "W41: 1510") {
				t.Errorf("Right moved to %q and read %q", tt.Texts(), said)
			}
			tt.Key(0, ui.KeyEnd)
			tt.Frame()
			if !tt.HasText("2015") {
				t.Errorf("End did not move to the last bar: %q", tt.Texts())
			}
			tt.Key(0, ui.KeyEscape)
			tt.Frame()
			if tt.HasText("2015") {
				t.Error("Escape kept the value")
			}
		})
		t.Run("line", func(t *testing.T) {
			st := stageCharts(dark, conversation.Block{Kind: conversation.TextBlock, Text: fence(lineSource)})
			tt := st.tt
			// A block without a title shows the source's, once, as the caption.
			plot, ok := tt.AccessByRole(ui.RoleImage, "Week-4 retention")
			if !ok || !plot.Focusable || plot.Bounds.H > 200 || plot.Bounds.W > 560 {
				t.Fatalf("no native line chart: %v %+v", ok, plot)
			}
			if st.count("Week-4 retention") != 1 || !tt.HasText("Pro") || !tt.HasText("Free") || !tt.HasText("%") {
				t.Errorf("title, legend and axis title: %q", tt.Texts())
			}
			st.bare(t, plot.Bounds)
			if st.moves(2 * time.Second) {
				t.Error("the chart moves at rest")
			}
			st.shot(t, "line", dark)
			b := plot.Bounds
			tt.Move(b.X+b.W*0.92, b.Y+b.H*0.5)
			tt.Frame()
			if !tt.HasText("65") || !tt.HasText("54") {
				t.Errorf("no values under the pointer: %q", tt.Texts())
			}
			if hot, _ := tt.AccessByRole(ui.RoleImage, "Week-4 retention"); hot.Value != "W41: Pro 65, Free 54" {
				t.Errorf("the chart's value reads %q", hot.Value)
			}
			st.shot(t, "line-hover", dark)
		})
		t.Run("pie", func(t *testing.T) {
			st := stageCharts(dark, conversation.Block{Kind: conversation.TextBlock, Text: fence(pieSource)})
			tt := st.tt
			wheel, ok := tt.AccessByRole(ui.RoleImage, "流量来源")
			if !ok || !wheel.Focusable || wheel.Bounds.W != 180 || wheel.Bounds.H != 180 {
				t.Fatalf("no native pie: %v %+v", ok, wheel)
			}
			if st.count("流量来源") != 1 || !tt.HasText("直接访问") || !tt.HasText("42%") || tt.HasText("4200 · 42%") {
				t.Errorf("title and legend: %q", tt.Texts())
			}
			if st.moves(2 * time.Second) {
				t.Error("the chart moves at rest")
			}
			st.shot(t, "pie", dark)
			// The first slice runs clockwise from twelve o'clock.
			b := wheel.Bounds
			tt.Move(b.X+b.W/2+45, b.Y+b.H/2-10)
			tt.Frame()
			if !tt.HasText("4200 · 42%") {
				t.Errorf("no value under the pointer: %q", tt.Texts())
			}
			st.shot(t, "pie-hover", dark)
			// A pie that shows its data has the values in its legend.
			data := stageCharts(dark, conversation.Block{Kind: conversation.TextBlock, Text: fence(strings.Replace(pieSource, "pie\n", "pie showData\n", 1))})
			if !data.tt.HasText("4200 · 42%") {
				t.Errorf("no values in the legend: %q", data.tt.Texts())
			}
		})
		t.Run("flow", func(t *testing.T) {
			st := stageCharts(dark, conversation.Block{Kind: conversation.TextBlock, Text: fence(flowSource)})
			d, ok := st.tt.AccessByRole(ui.RoleImage, "flowchart")
			if !ok || d.Focusable || d.Bounds.W <= 0 || d.Bounds.H <= 0 || d.Bounds.H > 420 {
				t.Fatalf("no limu diagram: %v %+v", ok, d)
			}
			if intro, _ := st.tt.Find(chartIntro); d.Bounds.X != intro.X {
				t.Errorf("the diagram starts at %v, the text at %v", d.Bounds.X, intro.X)
			}
			st.bare(t, d.Bounds)
			if st.moves(2 * time.Second) {
				t.Error("the diagram moves at rest")
			}
			st.shot(t, "flow", dark)
		})
	}
	t.Run("tall", func(t *testing.T) {
		// A diagram taller than the cap is scaled down, then scrolls.
		src := "flowchart TD\n"
		for i := range 30 {
			src += "    N" + strconv.Itoa(i) + "[Step " + strconv.Itoa(i) + "] --> N" + strconv.Itoa(i+1) + "\n"
		}
		st := stageCharts(false, conversation.Block{Kind: conversation.TextBlock, Text: fence(src)})
		if d, ok := st.tt.AccessByRole(ui.RoleImage, "flowchart"); !ok || d.Bounds.H > 420 {
			t.Errorf("a tall diagram takes %v", d.Bounds)
		}
		if !st.tt.HasText(chartOutro) {
			t.Error("the text after a tall diagram is out of the window")
		}
	})
	t.Run("hostile", checkHostileCharts)
}

// checkHostileCharts draws data a model may write that a chart cannot
// show, or can only with care: none panics, each is a native chart or
// falls back to limu's picture or its code, and what is drawn keeps
// within the caps.
func checkHostileCharts(t *testing.T) {
	list := func(n int, item func(i int) string) string {
		parts := make([]string, n)
		for i := range parts {
			parts[i] = item(i)
		}
		return strings.Join(parts, ", ")
	}
	slicesOf := func(n int) string {
		src := "pie\n"
		for i := range n {
			src += "    \"s" + strconv.Itoa(i) + "\" : " + strconv.Itoa(i+1) + "\n"
		}
		return src
	}
	xy := "xychart-beta\n"
	long := strings.Repeat("很长的标签 label ", 400)
	for _, tc := range []struct {
		name, src string
		native    bool
	}{
		{"no series", xy + "    x-axis [a, b]\n", false},
		{"empty series", xy + "    bar []\n", false},
		{"not a number", xy + "    bar [1, NaN, 3]\n", false},
		{"infinite", xy + "    bar [1e999, 2]\n", false},
		{"500 points", xy + "    bar [" + list(500, strconv.Itoa) + "]\n", false},
		{"uneven on numbers", xy + "    x-axis 1 --> 4\n    bar [1, 2]\n    line [1, 2, 3]\n", false},
		{"nine series", xy + strings.Repeat("    line [1, 2, 3]\n", 9), false},
		{"uneven on categories", xy + "    x-axis [a, b, c]\n    bar [1, 2]\n    line [3, 2, 1, 4]\n", true},
		{"same labels", xy + "    x-axis [a, a, a]\n    bar [1, 2, 3]\n", true},
		{"huge label", xy + "    title \"" + long + "\"\n    x-axis [\"" + long + "\", b]\n    bar \"" + long + "\" [1, 2]\n    line \"" + long + "\" [2, 1]\n", true},
		{"below zero", xy + "    bar [-5, 3, -2]\n", true},
		{"all zero", xy + "    bar [0, 0, 0]\n", true},
		{"one point", xy + "    line [7]\n", true},
		{"366 points", xy + "    line [" + list(366, strconv.Itoa) + "]\n    bar [" + list(366, strconv.Itoa) + "]\n", true},
		{"horizontal", xy + "    horizontal\n    x-axis [" + list(40, func(i int) string { return "row" + strconv.Itoa(i) }) + "]\n    bar [" + list(40, strconv.Itoa) + "]\n", true},
		{"pie below zero", "pie\n    \"a\" : -1\n    \"b\" : 2\n", false},
		{"pie of nothing", "pie\n    \"a\" : 0\n    \"b\" : 0\n", false},
		{"pie of 13", slicesOf(13), false},
		{"pie of one", slicesOf(1), true},
		{"pie of 12", slicesOf(12), true},
		{"pie huge label", "pie\n    title " + strings.Repeat("x", 3000) + "\n    \"" + long + "\" : 3\n    \"b\" : 0\n", true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			mode := theme.Light
			tt := replyTester(fence(tc.src), &mode)
			native := false
			for _, a := range tt.AccessAll() {
				if a.Role != ui.RoleImage {
					continue
				}
				native = native || a.Focusable
				if b := a.Bounds; b.H > 420 || b.X+b.W > 900 || (a.Focusable && (b.H > 260 || b.W > 560)) {
					t.Errorf("the visual takes %v", b)
				}
				// The pointer across it and the keys on it pick without a panic.
				b := a.Bounds
				for _, f := range []float32{0.02, 0.5, 0.98} {
					tt.Move(b.X+b.W*f, b.Y+b.H*f)
					tt.Frame()
				}
				tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
				for _, key := range []ui.Key{ui.KeyLeft, ui.KeyHome, ui.KeyLeft, ui.KeyEnd, ui.KeyRight, ui.KeyEscape} {
					tt.Key(0, key)
					tt.Frame()
				}
			}
			if native != tc.native {
				t.Errorf("drawn natively: %v, want %v", native, tc.native)
			}
			for _, text := range tt.Texts() {
				if n := len([]rune(text)); tc.native && n > 60 {
					t.Errorf("a label of %d characters is shown whole", n)
				}
			}
		})
	}
}
