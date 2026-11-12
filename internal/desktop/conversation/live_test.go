package conversation_test

import (
	"image"
	"image/color"
	"image/draw"
	"image/png"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The pure text rules of the transcript: citations, search marks, search
// matches, quoting and counts.
func TestTextRules(t *testing.T) {
	t.Run("Cite", func(t *testing.T) {
		got := conversation.Cite("Go adds iterators.", []conversation.Citation{{URI: "https://go.dev/a", End: 2}, {URI: "https://x/<b>"}})
		want := "Go [1](<https://go.dev/a>) adds iterators. [2](<https://x/%3Cb%3E>)"
		if got != want {
			t.Errorf("Cite = %q, want %q", got, want)
		}
		if conversation.Cite("plain", nil) != "plain" {
			t.Error("text without citations changed")
		}
	})
	t.Run("MarkSkipsCode", func(t *testing.T) {
		src := "Find the list.\n\n```go\nlist := 1\n```\nUse `list` and List."
		got := conversation.Mark(src, "list")
		want := "Find the <mark>list</mark>.\n\n```go\nlist := 1\n```\nUse `list` and <mark>List</mark>."
		if got != want {
			t.Errorf("Mark =\n%q\nwant\n%q", got, want)
		}
		// A link keeps its destination, a citation's as Cite writes it too:
		// its text is marked, and the text around it.
		src = "The Go [go.dev (go)](https://go.dev/doc(go)) site [1](<https://go.dev/a>) is go](."
		got = conversation.Mark(conversation.Cite(src, nil), "go")
		want = "The <mark>Go</mark> [<mark>go</mark>.dev (<mark>go</mark>)](https://go.dev/doc(go)) site [1](<https://go.dev/a>) is <mark>go</mark>](."
		if got != want {
			t.Errorf("Mark =\n%q\nwant\n%q", got, want)
		}
	})
	t.Run("Find", func(t *testing.T) {
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
	})
	t.Run("QuoteAndCount", func(t *testing.T) {
		if q := conversation.Quote("a\nb\n"); q != "> a\n> b\n\n" {
			t.Errorf("Quote = %q", q)
		}
		for n, want := range map[int]string{950: "950", 1200: "1.2k", 3000: "3k", 3_400_000: "3.4M"} {
			if got := conversation.Count(n); got != want {
				t.Errorf("Count(%d) = %q, want %q", n, got, want)
			}
		}
	})
}

// recorder keeps the transcript's actions.
type recorder struct {
	noActions
	opened []conversation.File
	older  int
	// The message being edited, and what the transcript asked for.
	editing        string
	edits, cancels int
	saved, started []string
	forks, rewinds []string
	fileActions    []conversation.FileAction
	turns          []conversation.Turn
}

func (r *recorder) Edit(turn string) {
	r.edits++
	for i := range r.turns {
		if r.turns[i].ID == turn {
			r.turns[i].Editing = &conversation.Editing{Text: r.turns[i].User}
		}
	}
}
func (r *recorder) CancelEdit(turn string) {
	r.cancels++
	for i := range r.turns {
		if r.turns[i].ID == turn {
			r.turns[i].Editing = nil
		}
	}
}
func (r *recorder) SaveEdit(turn, text string) {
	r.saved = append(r.saved, turn+"\x00"+text)
	for i := range r.turns {
		if r.turns[i].ID == turn {
			r.turns[i].Editing.Pending = true
		}
	}
}
func (r *recorder) Start(text string)  { r.started = append(r.started, text) }
func (r *recorder) Fork(turn string)   { r.forks = append(r.forks, turn) }
func (r *recorder) Rewind(turn string) { r.rewinds = append(r.rewinds, turn) }
func (r *recorder) FileAction(_ conversation.File, a conversation.FileAction) {
	r.fileActions = append(r.fileActions, a)
}

// transcriptShot saves the frame under the worktree's .runtime/ui-shots.
func transcriptShot(t *testing.T, tt *ui.Tester, name string) {
	t.Helper()
	dir := filepath.Join("..", "..", "..", ".runtime", "ui-shots")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	f, err := os.Create(filepath.Join(dir, name+".png"))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	if err := png.Encode(f, tt.Image()); err != nil {
		t.Fatal(err)
	}
}

// The message actions and the live content of a reply, in Chinese and dark:
// the inline editor, the enablement of Edit and the fork menu, cards from
// Markdown, a tool result's picture, a stopped retry, an interrupted
// compaction, a document's exports, the search result and the selection pill.
func checkTranscript(t *testing.T) {
	const lang = "zh-CN"
	T := func(key string, args ...any) string { return i18n.In(lang, key, args...) }
	pic := func() *ui.Bitmap {
		img := image.NewRGBA(image.Rect(0, 0, 320, 200))
		draw.Draw(img, img.Bounds(), image.NewUniform(color.RGBA{R: 40, G: 90, B: 160, A: 255}), image.Point{}, draw.Src)
		draw.Draw(img, image.Rect(40, 40, 200, 140), image.NewUniform(color.RGBA{R: 230, G: 180, B: 60, A: 255}), image.Point{}, draw.Src)
		return ui.NewBitmap(img)
	}
	shot := conversation.Image{ID: "shot", Name: "screen.png", Width: 320, Height: 200, Source: pic(), Reference: "call/0"}
	r := &recorder{}
	r.turns = []conversation.Turn{
		{ID: "edit", User: "把主题迁移到新令牌", Reply: conversation.Reply{Status: conversation.Completed, Blocks: []conversation.Block{
			{Kind: conversation.TextBlock, Text: "已完成，报告与截图如下。\n\n[迁移报告](docs/report.md)\n\n![截图](sailry-image:call/0)\n\n![缺失](sailry-image:call/9)\n\n其余内容见 [设计说明](docs/design.md) 一文。"},
		}}},
		{ID: "tools", User: "截一张图", Reply: conversation.Reply{Status: conversation.Failed, Blocks: []conversation.Block{
			{Kind: conversation.ToolsBlock, Calls: []conversation.Call{{ID: "shot", Name: "load_skill", LabelKey: "tool_load_skill", Icon: "book-open", Detail: "screenshot", Output: &conversation.Output{Text: "captured", Images: []conversation.Image{shot}}}}},
			{Kind: conversation.ToolsBlock, Calls: []conversation.Call{{ID: "denied", Name: "read_session", LabelKey: "tool_read_session", Icon: "file-text", Failed: true, Error: T("tool_denied")}}},
			{Kind: conversation.RetryBlock, Retry: &conversation.Retry{Attempt: 2, Limit: 5, Stopped: true, Reason: "chat_model_rate_limited"}},
			{Kind: conversation.SearchBlock, Text: "Go 1.23 加入了迭代器。", Sources: []conversation.Citation{{URI: "https://go.dev/blog/range-functions", Title: "Range over function types"}}, Suggestions: []conversation.Citation{{URI: "https://example.com/search?q=go", Title: "go iterators"}}},
			{Kind: conversation.FilesBlock, Files: []conversation.File{{ID: "doc", Path: "方案.md", Kind: conversation.MarkdownFile, Document: true, Exportable: true}}},
		}}},
		{ID: "compact", Compaction: &conversation.Compaction{State: conversation.CompactionInterrupted, Elapsed: 83 * 1e9}},
		{ID: "child", User: "只读会话的消息", Controls: conversation.Controls{ReadOnly: true, NoEdit: true, NoFork: true, NoRewind: true, NoWorktree: true},
			Reply: conversation.Reply{Status: conversation.Completed, Blocks: []conversation.Block{{Kind: conversation.TextBlock, Text: "只读回复"}}}},
	}
	s := conversation.New()
	var embedded []string
	s.Embed = func(_, dest, label string, image bool) (conversation.Embed, bool) {
		embedded = append(embedded, dest)
		switch {
		case dest == "docs/report.md" && !image:
			return conversation.Embed{File: &conversation.File{ID: "link/docs/report.md", Path: "docs/report.md", Kind: conversation.MarkdownFile}}, true
		case dest == "sailry-image:call/0" && image:
			return conversation.Embed{Image: &shot}, true
		case image:
			return conversation.Embed{Missing: true}, true
		}
		return conversation.Embed{}, false
	}
	sess := &conversation.Session{}
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		ui.Column(c).Fill().Children(func() { conversation.View(c, s, "t", r.turns, sess, r) })
	}, 1024, 1500)
	tt.SetLocale(lang)
	tt.Frame()
	tt.Frame()
	click := func(label string) {
		t.Helper()
		if err := tt.Click(label); err != nil {
			t.Fatal(err, tt.Texts())
		}
		tt.Frame()
	}
	// A read-only session has one Edit fewer and one fork menu fewer than its turns.
	count := func(label string) int {
		n := 0
		for _, text := range tt.Texts() {
			if text == label {
				n++
			}
		}
		return n
	}
	if edits, forks := count(T("message_edit")), count(T("chat_fork")); edits != 2 || forks != 2 {
		t.Fatalf("Edit shows %d times and the fork menu %d: %q", edits, forks, tt.Texts())
	}
	// Cards from Markdown: the standalone link is a file card, the conversation's picture a thumbnail, the unknown one a line; a link inside a sentence stays a link.
	if !tt.HasText("report.md") || !tt.HasText(T("chat_image_missing")) || !tt.HasText("设计说明") {
		t.Fatalf("Markdown cards: %q", tt.Texts())
	}
	if _, ok := tt.AccessByRole(ui.RoleImage, "screen.png"); !ok {
		if _, found := tt.Find("screen.png"); !found {
			t.Fatalf("no image card: %q", tt.Texts())
		}
	}
	for _, dest := range embedded {
		if dest == "docs/design.md" {
			t.Fatal("a link inside a sentence was offered as a card")
		}
	}
	click(T("artifact_open"))
	if len(r.opened) != 1 || r.opened[0].Path != "docs/report.md" {
		t.Fatalf("the file card opened %+v", r.opened)
	}
	// Edit turns the bubble into its editor; Save sends the text once and shows it pending.
	click(T("message_edit"))
	if r.edits != 1 || !tt.HasText(T("queue_save")) || !tt.HasText(T("settings_cancel")) {
		t.Fatalf("no inline editor: %q", tt.Texts())
	}
	if edit, _ := tt.AccessByRole(ui.RoleButton, T("message_edit")); !edit.Disabled {
		t.Error("Edit stays enabled while its editor is open")
	}
	tt.Type("，并保留圆角")
	transcriptShot(t, tt, "transcript-inline-edit-zh-dark")
	tt.Key(0, ui.KeyEscape)
	tt.Frame()
	if r.cancels != 1 || tt.HasText(T("queue_save")) || !tt.HasText("把主题迁移到新令牌") {
		t.Fatalf("Escape did not restore the bubble: %q", tt.Texts())
	}
	click(T("message_edit"))
	click(T("settings_cancel"))
	if r.cancels != 2 || tt.HasText(T("queue_save")) {
		t.Fatal("Cancel did not restore the bubble")
	}
	click(T("message_edit"))
	tt.ClickAt(8, 1490)
	tt.Frame()
	if r.cancels != 3 || tt.HasText(T("queue_save")) {
		t.Fatal("leaving the editor did not restore the bubble")
	}
	click(T("message_edit"))
	tt.Type(" 更新")
	click(T("queue_save"))
	if len(r.saved) != 1 || r.saved[0] != "edit\x00把主题迁移到新令牌 更新" || r.cancels != 3 {
		t.Fatalf("Save sent %q", r.saved)
	}
	// While the replacement is in flight nothing saves again or cancels.
	tt.Key(0, ui.KeyEscape)
	tt.Key(0, ui.KeyEnter)
	tt.Frame()
	if save, _ := tt.AccessByRole(ui.RoleButton, T("queue_save")); !save.Disabled || len(r.saved) != 1 || r.cancels != 3 {
		t.Fatal("a pending editor saved again or cancelled", len(r.saved), r.cancels)
	}
	r.turns[0].Editing = nil
	// A turn whose actions do not apply keeps them disabled and silent.
	r.turns[0].Controls = conversation.Controls{NoEdit: true, NoFork: true, NoRewind: true, NoWorktree: true}
	tt.Frame()
	if edit, _ := tt.AccessByRole(ui.RoleButton, T("message_edit")); !edit.Disabled {
		t.Error("Edit is enabled where it cannot apply")
	}
	tt.Click(T("message_edit"))
	tt.Frame()
	if r.edits != 4 {
		t.Fatal("a disabled Edit acted")
	}
	r.turns[0].Controls = conversation.Controls{}
	click(T("turn_failed"))
	click(T("tool_read_session")) // the sequence opens on its calls
	// The tool's label is its built-in key in Chinese; its picture shows under the output and opens the viewer.
	if !tt.HasText(T("tool_load_skill")) || !tt.HasText(T("tool_read_session")) || !tt.HasText(T("tool_denied")) || tt.HasText("load_skill") {
		t.Fatalf("core tool labels: %q", tt.Texts())
	}
	click(T("tool_load_skill"))
	tt.Frame()
	if !tt.HasText("captured") {
		t.Fatalf("the tool output did not open: %q", tt.Texts())
	}
	// The stopped retry names its reason; the interrupted compaction its state and time.
	if !tt.HasText(T("chat_model_rate_limited")) || !tt.HasText(T("chat_model_retried", "attempt", 2, "limit", 5)) {
		t.Fatalf("retry notice: %q", tt.Texts())
	}
	if want := T("chat_compact_interrupted") + " " + conversation.Elapsed(83*1e9); !tt.HasText(want) {
		t.Fatalf("no %q in %q", want, tt.Texts())
	}
	transcriptShot(t, tt, "transcript-cards-tool-image-retry-zh-dark")
	// The document card's menu exports it as HTML and as an image.
	r.turns = r.turns[1:] // the first turn's menus come before the document's otherwise
	tt.Frame()
	click(T("more"))
	click(T("conversation_export_html"))
	click(T("more"))
	click(T("conversation_export_image"))
	if len(r.fileActions) != 2 || r.fileActions[0] != conversation.ExportDocumentHTML || r.fileActions[1] != conversation.ExportDocumentImage {
		t.Fatalf("document exports = %v", r.fileActions)
	}
	// The search row opens the result: the answer, its source and the provider's suggestion.
	click(T("chat_search_result"))
	tt.Advance(1e9)
	if !tt.HasText("Range over function types") || !tt.HasText("go iterators") {
		t.Fatalf("search result: %q", tt.Texts())
	}
	transcriptShot(t, tt, "transcript-search-result-zh-dark")
	click("go iterators")
	if opened := tt.OpenedURLs(); len(opened) != 1 || opened[0] != "https://example.com/search?q=go" {
		t.Fatalf("a suggestion opened %q", opened)
	}
	tt.Key(0, ui.KeyEscape)
	tt.Advance(1e9)
	if tt.HasText("go iterators") {
		t.Fatal("Escape did not close the search result")
	}
	// Selecting text of a reply offers Quote, Copy and New conversation.
	sess.ReadOnly = true
	at, ok := tt.Find("只读回复")
	if !ok {
		t.Fatal("no reply to select")
	}
	tt.Press(at.X+1, at.Y+at.H/2)
	tt.Move(at.X+at.W-1, at.Y+at.H/2)
	tt.Release(at.X+at.W-1, at.Y+at.H/2)
	tt.Frame()
	if quote, ok := tt.AccessByRole(ui.RoleButton, T("message_quote")); !ok || !quote.Disabled {
		t.Fatalf("a read-only session's Quote: %+v %q", quote, tt.Texts())
	}
	transcriptShot(t, tt, "transcript-selection-pill-zh-dark")
	click(T("chat_new"))
	if len(r.started) != 1 || !strings.HasPrefix(r.started[0], "> ") || !strings.Contains(r.started[0], "回复") {
		t.Fatalf("New conversation started %q", r.started)
	}
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
	t.Run("mermaid", checkMermaidBlocks)
	t.Run("charts", checkCharts)
	t.Run("content sizes", checkContentSizes)
	t.Run("diagrams", checkDiagrams)
	t.Run("links", checkLinks)
	t.Run("markdown", checkMarkdown)
	t.Run("math", checkMath)
	t.Run("transcript", checkTranscript)
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
			// The chart is drawn natively, named by its title and small.
			d, ok := tt.AccessByRole(ui.RoleImage, "Frames per day")
			if !ok || !d.Focusable || d.Bounds.W < 300 || d.Bounds.W > 560 || d.Bounds.H > 200 {
				t.Errorf("%s dark=%v: no chart: %+v", lang, dark, d)
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
