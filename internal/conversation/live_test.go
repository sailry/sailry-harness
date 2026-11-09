package conversation_test

import (
	"image/png"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/conversation"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/preview"
	"github.com/sailry/sailry-go/internal/theme"
)

func TestCite(t *testing.T) {
	got := conversation.Cite("Go adds iterators.", []conversation.Citation{{URI: "https://go.dev/a", End: 2}, {URI: "https://x/<b>"}})
	want := "Go [1](<https://go.dev/a>) adds iterators. [2](<https://x/%3Cb%3E>)"
	if got != want {
		t.Errorf("Cite = %q, want %q", got, want)
	}
	if conversation.Cite("plain", nil) != "plain" {
		t.Error("text without citations changed")
	}
}

func TestMarkSkipsCode(t *testing.T) {
	src := "Find the list.\n\n```go\nlist := 1\n```\nUse `list` and List."
	got := conversation.Mark(src, "list")
	want := "Find the <mark>list</mark>.\n\n```go\nlist := 1\n```\nUse `list` and <mark>List</mark>."
	if got != want {
		t.Errorf("Mark =\n%q\nwant\n%q", got, want)
	}
}

func TestFind(t *testing.T) {
	turns := []conversation.Turn{
		{User: "Port the theme", Reply: conversation.Reply{Blocks: []conversation.Block{{Kind: conversation.TextBlock, Text: "Theme ported; the theme loads."}}}},
		{User: "nothing here"},
	}
	m := conversation.Find(turns, "THEME")
	if len(m) != 3 || !m[0].User || m[1].User || m[2].N != 1 {
		t.Errorf("Find = %+v", m)
	}
	if conversation.Find(turns, "  ") != nil {
		t.Error("an empty query matched")
	}
}

func TestViewer(t *testing.T) {
	v := conversation.Viewer{Images: make([]conversation.Image, 3), Zoom: 1}
	for range 20 {
		v.ZoomBy(1.25)
	}
	if v.Zoom != conversation.MaxZoom {
		t.Errorf("zoom %v past the limit", v.Zoom)
	}
	v.Turns, v.FlipH = 1, true
	v.Step(true)
	if v.Index != 1 || v.Zoom != 1 || v.Turns != 0 || v.FlipH {
		t.Errorf("step kept the view: %+v", v)
	}
	v.Step(true)
	v.Step(true)
	if v.Index != 2 {
		t.Errorf("stepped past the end: %d", v.Index)
	}
}

func TestQuoteAndCount(t *testing.T) {
	if q := conversation.Quote("a\nb\n"); q != "> a\n> b\n\n" {
		t.Errorf("Quote = %q", q)
	}
	for n, want := range map[int]string{950: "950", 1200: "1.2k", 3000: "3k", 3_400_000: "3.4M"} {
		if got := conversation.Count(n); got != want {
			t.Errorf("Count(%d) = %q, want %q", n, got, want)
		}
	}
}

// recorder keeps the transcript's actions.
type recorder struct {
	noActions
	opened []conversation.File
	older  int
}

func (r *recorder) OpenFile(f conversation.File) { r.opened = append(r.opened, f) }
func (r *recorder) LoadOlder()                   { r.older++ }

// scenesTester shows one turn of every preview scene, tall enough to
// build them all, in the given theme mode and language.
func scenesTester(t *testing.T, dark bool, lang string) (*conversation.State, []conversation.Turn, *recorder, *ui.Tester) {
	turns := preview.Turns("scenes", 16, 0)
	s := conversation.New()
	r := &recorder{}
	mode := theme.Light
	if dark {
		mode = theme.Dark
	}
	sess := &conversation.Session{Older: 3}
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", mode)
		ui.Column(c).Fill().Children(func() { conversation.View(c, s, "scenes", turns, sess, r) })
	}, 1024, 9000)
	tt.SetLocale(lang)
	tt.Frame()
	tt.Frame()
	return s, turns, r, tt
}

// Every new block shows on the preview's scenes, in both languages and
// modes; snapshots go to $SAILRY_SNAPSHOTS when set.
func TestLiveBlocks(t *testing.T) {
	for _, lang := range []string{"en", "zh-CN"} {
		for _, dark := range []bool{false, true} {
			_, _, r, tt := scenesTester(t, dark, lang)
			for _, want := range []string{
				i18n.In(lang, "turn_changes_count", "count", 5),

				i18n.In(lang, "background_outcome_stalled"),
				"scroll-report.md", "invoice-2026-11.pdf", "Frames per day",
			} {
				if !tt.HasText(want) {
					t.Errorf("%s dark=%v: %q not shown", lang, dark, want)
				}
			}
			if r.older == 0 {
				t.Errorf("the start in view asked for no older page")
			}
			if dir := os.Getenv("SAILRY_SNAPSHOTS"); dir != "" {
				name := "live-" + lang
				if dark {
					name += "-dark"
				}
				f, err := os.Create(filepath.Join(dir, name+".png"))
				if err != nil {
					t.Fatal(err)
				}
				png.Encode(f, tt.Image())
				f.Close()
			}
		}
	}
}

// Open on a file card asks for its preview.
func TestFileCardOpens(t *testing.T) {
	_, _, r, tt := scenesTester(t, false, "en")
	if !tt.HasText("frame.txt") {
		t.Fatal("no file card")
	}
	// The first Open is the attached log's card.
	if err := tt.Click(i18n.In("en", "artifact_open")); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if len(r.opened) != 1 || !strings.HasSuffix(r.opened[0].Path, "frame.txt") {
		t.Errorf("opened %+v", r.opened)
	}
}

// ⌘F's search counts the matches and steps through them.
func TestSearchBar(t *testing.T) {
	s, turns, _, tt := scenesTester(t, false, "en")
	s.OpenSearch("scenes")
	tt.Frame()
	tt.Type("theme")
	tt.Frame()
	n := len(conversation.Find(turns, "theme"))
	if n == 0 {
		t.Fatal("the scenes say no theme")
	}
	if !tt.HasText("1 of " + strconv.Itoa(n)) {
		t.Fatalf("count not shown: %q", tt.Texts())
	}
	tt.Key(0, ui.KeyEnter)
	tt.Frame()
	if !tt.HasText("2 of " + strconv.Itoa(n)) {
		t.Errorf("Enter did not step: %q", tt.Texts())
	}
	tt.Key(0, ui.KeyEscape)
	tt.Frame()
	if s.SearchOpen() {
		t.Error("Escape left the bar open")
	}
}
