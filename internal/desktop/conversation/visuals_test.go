package conversation_test

import (
	"bytes"
	"image/png"
	"log/slog"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
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
// reply: a diagram that is not a data chart is drawn as a picture, a theme
// switch and a narrower window keep it, and a source that cannot be read
// or a block with no end stays code.
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
// a picture without a card; nothing moves at rest; and data the painters
// cannot show falls back without a panic.
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
			// The other is the name the plot has for assistive technology.
			if st.count(title) != 2 || tt.HasText("Weekly active teams") {
				t.Errorf("titles: %q", tt.Texts())
			}
			caption, _ := tt.Find(title)
			if intro, _ := tt.Find(chartIntro); caption.X != intro.X {
				t.Errorf("the caption starts at %v, the text at %v", caption.X, intro.X)
			}
			for _, want := range []string{"W40", "W45", "teams", "2000"} {
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
			if st.count("Week-4 retention") != 2 || !tt.HasText("Pro") || !tt.HasText("Free") || !tt.HasText("%") {
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
			if st.count("流量来源") != 2 || !tt.HasText("直接访问") || !tt.HasText("42%") || tt.HasText("4200 · 42%") {
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
				t.Fatalf("no diagram: %v %+v", ok, d)
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
		st.tt.SetSize(chartStageWidth, 760)
		st.tt.Frame()
		st.tt.Frame()
		if _, ok := st.tt.AccessByRole(ui.RoleImage, "flowchart"); !ok {
			t.Fatal("no diagram")
		}
		above, _ := st.tt.Find(chartIntro)
		below, ok := st.tt.Find(chartOutro)
		if room := below.Y - above.Y - above.H; !ok || room < 300 || room > 420+40 {
			t.Errorf("a tall diagram takes %v between %v and %v", room, above, below)
		}
		st.shot(t, "tall", false)
	})
	t.Run("mixed", func(t *testing.T) {
		// Bars and a line share one value axis; a horizontal chart runs
		// its categories down.
		const sales = "xychart-beta\n    title \"Sales revenue\"\n    x-axis [jan, feb, mar, apr, may, jun, jul, aug, sep, oct, nov, dec]\n    y-axis \"Revenue (in $)\" 4000 --> 11000\n    bar [5000, 6000, 7500, 8200, 9500, 10500, 11000, 10200, 9200, 8500, 7000, 6000]\n    line [5000, 6000, 7500, 8200, 9500, 10500, 11000, 10200, 9200, 8500, 7000, 6000]\n"
		const plans = "xychart-beta horizontal\n    x-axis [Free, Starter, Pro, Enterprise]\n    y-axis \"teams\"\n    bar \"W44\" [820, 540, 360, 120]\n    bar \"W45\" [870, 610, 395, 140]\n"
		st := stageCharts(false, conversation.Block{Kind: conversation.TextBlock, Text: fence(sales) + "\n\n" + fence(plans)})
		st.tt.SetSize(chartStageWidth, 760)
		st.tt.Frame()
		st.tt.Frame()
		mixed, ok := st.tt.AccessByRole(ui.RoleImage, "Sales revenue")
		if !ok || !mixed.Focusable || mixed.Bounds.H > 200 {
			t.Fatalf("no native chart of bars and a line: %v %+v", ok, mixed)
		}
		rows, ok := st.tt.AccessByRole(ui.RoleImage, "xychart")
		if !ok || !rows.Focusable || rows.Bounds.H > 260 || !st.tt.HasText("W44") || !st.tt.HasText("Enterprise") {
			t.Fatalf("no native horizontal chart named by its kind: %v %+v %q", ok, rows, st.tt.Texts())
		}
		b := rows.Bounds
		st.tt.Move(b.X+b.W*0.3, b.Y+b.H*0.12)
		st.tt.Frame()
		if hot, _ := st.tt.AccessByRole(ui.RoleImage, "xychart"); hot.Value != "Free: W44 820, W45 870" {
			t.Errorf("the first row reads %q", hot.Value)
		}
		st.shot(t, "mixed", false)
	})
	t.Run("hostile", checkHostileCharts)
}

// checkHostileCharts draws data a model may write that a chart cannot
// show, or can only with care: none panics, each is a native chart or
// falls back to the diagram's picture or its code, and what is drawn keeps
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
		{"horizontal", "xychart-beta horizontal\n    x-axis [" + list(40, func(i int) string { return "row" + strconv.Itoa(i) }) + "]\n    bar [" + list(40, strconv.Itoa) + "]\n", true},
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

// The diagrams of the proof frames, as a model writes them.
const (
	seqSource   = "sequenceDiagram\n    participant C as 客户端\n    participant R as 哈希环\n    participant N as 节点 B\n    C->>R: hash(\"user:42\")\n    R-->>C: 节点 B\n    C->>N: GET user:42\n    N-->>C: 值\n"
	classSource = "classDiagram\n    class Ring {\n        +points []uint32\n        +Add(node string)\n        +Node(key string) string\n    }\n    class Node {\n        +name string\n        +Get(key string) Value\n    }\n    class Value\n    Ring \"1\" o-- \"*\" Node : places\n    Node --> Value : returns\n"
	stateSource = "stateDiagram-v2\n    [*] --> Queued\n    Queued --> Running: start\n    Running --> Completed: done\n    Running --> Failed: error\n    Failed --> Queued: retry\n    Completed --> [*]\n"
	erSource    = "erDiagram\n    TEAM ||--o{ MEMBER : has\n    TEAM ||--o{ PROJECT : owns\n    PROJECT ||--o{ SESSION : contains\n    TEAM {\n        string id PK\n        string name\n    }\n    SESSION {\n        string id PK\n        datetime started\n    }\n"
	ganttSource = "gantt\n    title Release plan\n    dateFormat YYYY-MM-DD\n    section Build\n    Parser :a1, 2026-11-02, 5d\n    Renderer :a2, after a1, 7d\n    section Ship\n    Review :b1, after a2, 3d\n    Release :milestone, m1, after b1, 0d\n"
)

// diagramKinds is a source of every kind of diagram that is not a data
// chart, with the name its kind gives it.
var diagramKinds = []struct{ kind, src string }{
	{"flowchart", flowSource},
	{"sequence", seqSource},
	{"class", classSource},
	{"state", stateSource},
	{"er", erSource},
	{"gantt", ganttSource},
	{"gitGraph", "gitGraph\n    commit id: \"Alpha\"\n    commit id: \"Beta\"\n    branch develop\n    commit id: \"Gamma\"\n    checkout main\n    merge develop\n"},
	{"journey", "journey\n    title Release day\n    section Morning\n      Tag the build: 5: Ada\n      Run the gate: 3: Ada\n      Fix a flake: 1: Ada, Lin\n    section Afternoon\n      Publish: 5: Ada\n      Write notes: 4: Lin\n"},
	{"mindmap", "mindmap\n    Sailry\n        Host[Host]\n        Desktop(Desktop)\n        Mobile\n"},
	{"timeline", "timeline\n    title Release history\n    2024 : Prototype\n    2025 : Desktop : Host\n    2026 : Mobile\n"},
	{"quadrantChart", "quadrantChart\n  x-axis Low effort --> High effort\n  y-axis Low value --> High value\n  quadrant-1 Plan\n  quadrant-2 Do\n  quadrant-3 Drop\n  quadrant-4 Delegate\n"},
	{"sankey", "sankey\n\nSignups,Activated,280\n\nSignups,Dropped,120\n\nActivated,Retained,190\n"},
	{"requirement", "requirementDiagram\n\nrequirement \"retry_delivery\" {\n    id: 1\n    text: \"A failed webhook is sent again\"\n    risk: high\n    verifymethod: test\n}\n"},
	{"c4", "C4Context\n    title System context\n    Person(user, \"User\", \"Asks questions\")\n    System(app, \"App\", \"Answers them\")\n    Rel(user, app, \"Asks\")\n"},
	{"block", "block\n  columns 3\n  parse layout paint cache\n"},
	{"architecture", "architecture-beta\n    service web(server)[Web]\n    service api(server)[API]\n    service db(database)[Store]\n    web:B --> T:db\n    api:B --> T:db\n    align row web api\n"},
	{"kanban", "kanban\n  todo[To do]\n    task1[Write the adapter]\n"},
	{"packet", "packet\ntitle Frame header\n+16: \"Kind\"\n+16: \"Flags\"\n32-47: \"Length\"\n48-63: \"Stream\"\n64-95: \"Payload\"\n"},
	{"radar", "radar-beta\n  axis s[\"Speed\"], c[\"Cost\"], q[\"Quality\"]\n  axis r[\"Reach\"], e[\"Ease\"], t[\"Trust\"]\n  curve a[\"Plan A\"]{85, 90, 80, 70, 75, 90}\n  curve b[\"Plan B\"]{70, 75, 85, 80, 90, 85}\n\n  max 100\n  min 0\n"},
	{"treemap", "treemap-beta\n\"Desktop\"\n    \"Kit\": 10\n    \"Conversation\": 20\n\"Host\"\n    \"Node\": 15\n    \"Link\": 25\n"},
	{"venn", "venn-beta\n  set A[\"Desktop\"]\n  set B[\"Mobile\"]\n  union A,B[\"Client\"]\n"},
	{"usecase", "usecase-beta\nactor Reader\nSearch\nReader -- \"with a query\" --> Search\n"},
	{"swimlane", "swimlane-beta\n  subgraph Review\n    read[Read the change]\n    reply[Leave a reply]\n  end\n"},
	{"wardley", "wardley-beta\ntitle Hosting\n\ncomponent Own servers [0.45, 0.40] (inertia)\ncomponent Managed platform [0.65, 0.45]\n\nOwn servers -> Managed platform\n"},
	{"treeView", "treeView-beta\n    \"internal\"\n        \"desktop\"\n            \"kit\"\n        \"node\"\n"},
	{"railroad", "railroad-ebnf-beta\ndigit = \"0\" | \"1\" | \"2\" ;\nnumber = digit ;\n"},
	{"ishikawa", "ishikawa-beta\n    Slow build\n    Cache\n        Cold cache\n        Wrong key\n    Tests\n        Run in series\n    Machine\n        Few cores\n"},
	{"eventmodeling", "eventmodeling\n\ntf 01 ui InviteForm\ntf 02 cmd SendInvite\ntf 03 evt InviteSent\n"},
	{"cynefin", "cynefin-beta\n  title Incident kinds\n\n  complex\n  complicated\n  clear\n  chaotic\n"},
	{"agentflow", "agentflow-beta TB\n  flow team[\"Release Team\"]\n    flow builder[\"Builder\"]\n      build[\"Build the package\"]@{ shape: task }\n    end\n\n    flow checker[\"Checker\"]\n      verify[\"Verify the package\"]@{ shape: task }\n    end\n\n    builder --> checker\n  end\n"},
	{"info", "info showInfo\n"},
}

// diagramFailed reports an error box in the window, under either title
// it may have, or the title the library gives one by itself.
func diagramFailed(tt *ui.Tester) bool {
	for _, lang := range []string{"en", "zh-CN"} {
		if tt.HasText(i18n.In(lang, "artifact_preview_failed")) {
			return true
		}
	}
	return tt.HasText("Diagram failed")
}

// coded counts the code blocks of mermaid source in the window, by their
// headers.
func coded(tt *ui.Tester) int {
	n := 0
	for _, text := range tt.Texts() {
		if text == "mermaid" {
			n++
		}
	}
	return n
}

// debugLog collects the debug log, and returns how often it says s.
func debugLog(t *testing.T) func(s string) int {
	t.Helper()
	var log bytes.Buffer
	previous := slog.Default()
	slog.SetDefault(slog.New(slog.NewTextHandler(&log, &slog.HandlerOptions{Level: slog.LevelDebug})))
	t.Cleanup(func() { slog.SetDefault(previous) })
	return func(s string) int { return strings.Count(log.String(), s) }
}

// checkDiagrams is the proof of the diagrams in a reply that are not data
// charts: each kind is one picture named for assistive technology, within
// the caps, with no card, no error box and nothing moving; data charts
// stay native; a source that cannot be drawn is its code, with no notice;
// a block still being written is its code, read by nobody; and the demo's
// diagrams draw.
func checkDiagrams(t *testing.T) {
	t.Run("flowchart and sequence", func(t *testing.T) {
		for _, mode := range []theme.Mode{theme.Light, theme.Dark} {
			for kind, src := range map[string]string{"flowchart": flowSource, "sequence": seqSource} {
				tt := replyTester("Here:\n\n"+fence(src)+"\n\nDone.", &mode)
				tt.Advance(time.Second)
				d, ok := tt.AccessByRole(ui.RoleImage, kind)
				if !ok || d.Focusable || d.Bounds.W <= 0 || d.Bounds.H <= 0 || d.Bounds.H > 420 {
					t.Fatalf("no %s picture: %v %+v", kind, ok, d)
				}
				if pictures(tt) != 1 || coded(tt) != 0 || diagramFailed(tt) || !tt.HasText("Done.") {
					t.Errorf("%s: %d pictures, %d code blocks, failed %v: %q", kind, pictures(tt), coded(tt), diagramFailed(tt), tt.Texts())
				}
			}
		}
	})
	t.Run("kinds", func(t *testing.T) {
		for _, tc := range diagramKinds {
			t.Run(tc.kind, func(t *testing.T) {
				start := time.Now()
				st := stageCharts(false, conversation.Block{Kind: conversation.TextBlock, Text: fence(tc.src)})
				first := time.Since(start)
				st.tt.SetSize(chartStageWidth, 760)
				st.tt.Frame()
				st.tt.Frame()
				d, ok := st.tt.AccessByRole(ui.RoleImage, tc.kind)
				if !ok || d.Focusable || d.Bounds.W <= 0 || d.Bounds.H <= 0 {
					t.Fatalf("no picture named %s: %v %+v %q", tc.kind, ok, d, st.tt.Texts())
				}
				t.Logf("%s is drawn at %v, first shown in %v", tc.kind, d.Bounds, first)
				// The room the reply gives the picture is what is capped:
				// one still past the cap at the smallest scale, as a
				// Wardley map is by a fraction, scrolls in a box of it.
				if b := d.Bounds; b.X+b.W > chartStageWidth {
					t.Errorf("the picture takes %v", b)
				}
				if intro, _ := st.tt.Find(chartIntro); d.Bounds.X != intro.X {
					t.Errorf("the picture starts at %v, the text at %v", d.Bounds.X, intro.X)
				}
				above, _ := st.tt.Find(chartIntro)
				below, ok := st.tt.Find(chartOutro)
				if room := below.Y - above.Y - above.H; !ok || room <= 0 || room > 420+40 {
					t.Errorf("the picture takes %v between %v and %v", room, above, below)
				}
				if pictures(st.tt) != 1 || coded(st.tt) != 0 || diagramFailed(st.tt) {
					t.Errorf("%d pictures, %d code blocks, failed %v: %q", pictures(st.tt), coded(st.tt), diagramFailed(st.tt), st.tt.Texts())
				}
				if st.moves(2 * time.Second) {
					t.Error("the picture moves at rest")
				}
			})
		}
	})
	t.Run("native", func(t *testing.T) {
		// A data chart is still the painters': it takes the focus and
		// reads its values, which a picture does not.
		for title, src := range map[string]string{"Weekly active teams": barSource, "流量来源": pieSource} {
			mode := theme.Light
			tt := replyTester(fence(src), &mode)
			if plot, ok := tt.AccessByRole(ui.RoleImage, title); !ok || !plot.Focusable || pictures(tt) != 1 || coded(tt) != 0 {
				t.Errorf("%q is not a native chart: %v %+v", title, ok, plot)
			}
		}
	})
	t.Run("titles", func(t *testing.T) {
		// The block's title is a caption and the name.
		const title = "读取路径"
		st := stageCharts(false, conversation.Block{Kind: conversation.VisualBlock, Visual: &conversation.Visual{Title: title, Mermaid: seqSource}})
		if _, ok := st.tt.AccessByRole(ui.RoleImage, title); !ok || st.count(title) != 2 || pictures(st.tt) != 1 {
			t.Errorf("a titled sequence: named %v, %d titles: %q", ok, st.count(title), st.tt.Texts())
		}
		caption, _ := st.tt.Find(title)
		if intro, _ := st.tt.Find(chartIntro); caption.X != intro.X {
			t.Errorf("the caption starts at %v, the text at %v", caption.X, intro.X)
		}
		// A picture that draws its title has no caption above it.
		st = stageCharts(false, conversation.Block{Kind: conversation.VisualBlock, Visual: &conversation.Visual{Title: title, Mermaid: ganttSource}})
		if _, ok := st.tt.AccessByRole(ui.RoleImage, title); !ok || st.count(title) != 1 {
			t.Errorf("a gantt with its own title: named %v, %d titles: %q", ok, st.count(title), st.tt.Texts())
		}
		// The source's title for assistive technology names an untitled block.
		st = stageCharts(false, conversation.Block{Kind: conversation.TextBlock, Text: fence("flowchart LR\n    accTitle: Signup funnel\n    A[Visit] --> B[Sign up]\n")})
		if _, ok := st.tt.AccessByRole(ui.RoleImage, "Signup funnel"); !ok || st.count("Signup funnel") != 1 {
			t.Errorf("an accTitle: named %v: %q", ok, st.tt.Texts())
		}
		// A source that cannot be drawn under a title is that title and its code.
		st = stageCharts(false, conversation.Block{Kind: conversation.VisualBlock, Visual: &conversation.Visual{Title: title, Mermaid: "flowchart LR\n  Cart -->\n"}})
		if pictures(st.tt) != 0 || st.count(title) != 1 || diagramFailed(st.tt) {
			t.Errorf("an undrawable visual: %d pictures, %d titles, failed %v: %q", pictures(st.tt), st.count(title), diagramFailed(st.tt), st.tt.Texts())
		}
	})
	t.Run("fallback", func(t *testing.T) {
		// A finished block that cannot be drawn is its code, with no error
		// box and no notice.
		for _, tc := range []struct {
			name, src string
			long      bool
		}{
			{"syntax error", "flowchart LR\n  Cart -->\n", false},
			{"unknown kind", "no such diagram\n", false},
			{"reserved property", "flowchart LR\n    A --> B\n    style A fill:var(--pele-bg)\n", false},
			{"reserved property, spaced", "flowchart LR\n    A --> B\n    style A stroke:VAR(  --_line )\n", false},
			{"past the library's limit", "flowchart TD\n" + strings.Repeat("    A --> B\n", 5000), true},
		} {
			t.Run(tc.name, func(t *testing.T) {
				says := debugLog(t)
				mode := theme.Light
				tt := replyTester("Here:\n\n"+fence(tc.src)+"\n\nDone.", &mode)
				tt.Advance(time.Second)
				if pictures(tt) != 0 || diagramFailed(tt) {
					t.Errorf("%d pictures, failed %v: %q", pictures(tt), diagramFailed(tt), tt.Texts())
				}
				// A long block's header is above the window.
				if !tc.long && (coded(tt) != 1 || !tt.HasText("Done.")) {
					t.Errorf("%d code blocks: %q", coded(tt), tt.Texts())
				}
				if says("crychic: Mermaid") != 0 {
					t.Error("the library was shown a source it cannot draw")
				}
			})
		}
	})
	t.Run("streaming", func(t *testing.T) {
		// A reply written a character at a time: the block is code until
		// its fence closes, nothing reads it before, and it is read once.
		says := debugLog(t)
		const full = "Here:\n\n```mermaid\nflowchart LR\n    Cart --> Pay\n    Pay -->|ok| Shipped\n```\n\nDone."
		text := ""
		s := conversation.New()
		tt := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			turns := []conversation.Turn{{ID: "stream", User: "Draw it", Reply: conversation.Reply{Status: conversation.Running,
				Blocks: []conversation.Block{{Kind: conversation.TextBlock, Text: text}}}}}
			ui.Column(c).Fill().Children(func() { conversation.View(c, s, "stream", turns, nil, noActions{}) })
		}, 900, 900)
		tt.SetLocale("en")
		closed := false
		for i, r := range full {
			text = full[:i+len(string(r))]
			tt.Frame()
			if diagramFailed(tt) {
				t.Fatalf("an error shows after %q: %q", text, tt.Texts())
			}
			if closed = strings.Count(text, "```") == 2; closed {
				continue
			}
			if pictures(tt) != 0 || says("crychic: Mermaid") != 0 {
				t.Fatalf("an unfinished block is drawn or read after %q: %d pictures, %d log lines", text, pictures(tt), says("crychic: Mermaid"))
			}
			if open := strings.Contains(text, "```mermaid\n"); open && coded(tt) != 1 {
				t.Fatalf("an unfinished block is not its code after %q: %q", text, tt.Texts())
			}
		}
		tt.Advance(time.Second)
		for range 10 {
			tt.Frame()
		}
		if _, ok := tt.AccessByRole(ui.RoleImage, "flowchart"); !closed || !ok || coded(tt) != 0 || !tt.HasText("Done.") {
			t.Errorf("the finished block is not drawn: %q", tt.Texts())
		}
		if says("Mermaid drew") != 1 || says("Mermaid failed") != 0 {
			t.Errorf("the library drew %d times and failed %d", says("Mermaid drew"), says("Mermaid failed"))
		}
		if says("Cart") != 0 || says("Shipped") != 0 {
			t.Error("the log holds the source")
		}
	})
	t.Run("demo", func(t *testing.T) {
		// Every diagram of the demo draws, and there is a flowchart and a
		// sequence diagram among them in both languages.
		fences := regexp.MustCompile("(?s)```mermaid\n(.*?\n)```")
		for _, lang := range []string{"en", "zh-CN"} {
			d := preview.Demo(lang, "local")
			all := slices.Clone(d.Unassigned)
			for _, p := range d.Projects {
				all = append(all, p.Conversations...)
			}
			kinds := map[string]int{}
			for _, cv := range all {
				for _, turn := range cv.Turns {
					for _, b := range turn.Reply.Blocks {
						var blocks []conversation.Block
						switch {
						case b.Kind == conversation.VisualBlock && b.Visual != nil:
							blocks = append(blocks, b)
						case b.Kind == conversation.TextBlock:
							for _, m := range fences.FindAllStringSubmatch(b.Text, -1) {
								blocks = append(blocks, conversation.Block{Kind: conversation.TextBlock, Text: fence(m[1])})
							}
						}
						for _, block := range blocks {
							st := stageCharts(false, block)
							if pictures(st.tt) != 1 || coded(st.tt) != 0 || diagramFailed(st.tt) {
								t.Errorf("%s: a visual of %s in %s is not drawn: %q", lang, turn.ID, cv.ID, st.tt.Texts())
								continue
							}
							for _, a := range st.tt.AccessAll() {
								if a.Role != ui.RoleImage {
									continue
								}
								if a.Focusable {
									kinds["chart"]++
								} else {
									kinds[a.Name]++
									st.tt.SetScale(2)
									st.shot(t, "diagram-demo-"+a.Name+"-"+lang, false)
								}
							}
						}
					}
				}
			}
			if kinds["flowchart"] == 0 || kinds["sequence"] == 0 {
				t.Errorf("%s: the demo draws %v, want a flowchart and a sequence diagram", lang, kinds)
			}
		}
	})
	t.Run("snapshots", func(t *testing.T) {
		// The common kinds, two to a frame, light and dark, as pictures to
		// look at: $SAILRY_SNAPSHOTS/conversation-chart-diagram-<kinds>-zh-CN-<mode>.png.
		// The gantt marks today, so its frame changes with the day.
		for _, dark := range []bool{false, true} {
			for _, pair := range []struct{ name, a, b string }{
				{"flow-sequence", flowSource, seqSource},
				{"class-state", classSource, stateSource},
				{"er-gantt", erSource, ganttSource},
			} {
				st := stageCharts(dark, conversation.Block{Kind: conversation.TextBlock, Text: fence(pair.a) + "\n\n" + fence(pair.b)})
				st.tt.SetSize(chartStageWidth, 1120)
				st.tt.SetScale(2)
				st.tt.Frame()
				st.tt.Advance(time.Second)
				if pictures(st.tt) != 2 || coded(st.tt) != 0 || diagramFailed(st.tt) {
					t.Errorf("%s: %d pictures: %q", pair.name, pictures(st.tt), st.tt.Texts())
				}
				st.shot(t, "diagram-"+pair.name, dark)
			}
		}
	})
}
