package preview

import (
	"image"
	"image/color"
	"math"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
	cv "github.com/sailry/sailry-harness/internal/desktop/conversation"
)

// Preview content for phase 4a: attachments, output files of every kind
// and preview state, pictures, a drawn chart, grounding with citations,
// model retries, background notices, compaction turns and a changes card;
// and the session states (older pages, a connection recovering with a
// diagnostic, the first page loading). Pictures are drawn here; nothing
// is read from disk.

var pictures = sync.OnceValue(func() []cv.Image {
	draw := func(w, h int, a, b color.RGBA) *ui.Bitmap {
		img := image.NewRGBA(image.Rect(0, 0, w, h))
		for y := range h {
			for x := range w {
				t := float64(x+y) / float64(w+h)
				wave := 0.5 + 0.5*math.Sin(float64(x)/18+float64(y)/27)
				mix := func(p, q uint8) uint8 { return uint8(float64(p)*(1-t) + float64(q)*t*0.85 + 24*wave) }
				img.SetRGBA(x, y, color.RGBA{mix(a.R, b.R), mix(a.G, b.G), mix(a.B, b.B), 255})
			}
		}
		return ui.NewBitmap(img)
	}
	return []cv.Image{
		{ID: "pic-dunes", Name: "dunes.png", Width: 480, Height: 300, Source: draw(480, 300, color.RGBA{230, 160, 90, 255}, color.RGBA{90, 60, 150, 255})},
		{ID: "pic-tide", Name: "tide.png", Width: 300, Height: 420, Source: draw(300, 420, color.RGBA{40, 120, 190, 255}, color.RGBA{20, 200, 170, 255})},
		{ID: "pic-moss", Name: "moss.png", Width: 360, Height: 360, Source: draw(360, 360, color.RGBA{60, 140, 70, 255}, color.RGBA{200, 210, 90, 255})},
	}
})

// chart is the frames per day the artifacts scene draws.
const chart = "xychart-beta\n    title \"Frames per day\"\n    x-axis [Mon, Tue, Wed, Thu, Fri]\n    y-axis \"frames\" 0 --> 180\n    bar [80, 130, 104, 160, 176]\n"

const reportMD = "# Scroll report\n\nFrames stayed under **1.2 ms** while scrolling 500 turns.\n\n| Case | ms |\n|---|---|\n| Steady | 0.18 |\n| Hover | 0.19 |\n| Scroll | 0.95 |\n\n- Rows build only in view\n- Markdown parses once\n"

const benchGo = "package conversation_test\n\nfunc BenchmarkSteady(b *testing.B) {\n\ts, cv, tt := bigTester(b)\n\ts.List(cv.ID).ScrollTo(500, ui.Center)\n\tfor b.Loop() {\n\t\ttt.Move(500, 300)\n\t}\n}\n"

const logText = "ok  \tgithub.com/sailry/sailry-harness/internal/desktop/conversation\t0.353s\nBenchmarkScroll-14    200    567118 ns/op\nBenchmarkSteady-14    200    175308 ns/op\n"

// outputs are the files of the artifacts scene: one of each kind and of
// each preview state.
func outputs(id string) []cv.File {
	pics := pictures()
	return []cv.File{
		{ID: id + "/report", Path: "docs/scroll-report.md", Mime: "text/markdown", Kind: cv.MarkdownFile, Text: reportMD, Image: &pics[0], Editable: true},
		{ID: id + "/bench", Path: "internal/conversation/bench_test.go", Mime: "text/x-go", Kind: cv.CodeFile, Text: benchGo, Editable: true},
		{ID: id + "/log", Path: "bench.txt", Mime: "text/plain", Kind: cv.TextFile, Text: logText},
		{ID: id + "/shot", Path: "screens/transcript.png", Mime: "image/png", Kind: cv.ImageFile, Image: &pics[2]},
		{ID: id + "/pdf", Path: "exports/invoice-2026-11.pdf", Mime: "application/pdf", Kind: cv.OtherFile},
		{ID: id + "/deck", Path: "exports/roadmap.pptx", Mime: "application/vnd.openxmlformats-officedocument.presentationml.presentation", Kind: cv.OtherFile},
		{ID: id + "/large", Path: "data/trace.json", Mime: "application/json", Kind: cv.TextFile, State: cv.PreviewLarge},
		{ID: id + "/changed", Path: "internal/kit/menu.go", Mime: "text/x-go", Kind: cv.CodeFile, State: cv.PreviewChanged},
	}
}

// attachments: a message with pictures and a file, answered.
func attachments(id string) cv.Turn {
	pics := pictures()
	r := done(cv.Completed, 9)
	r.Blocks = []cv.Block{text("Both screenshots show the header **4 DIPs** too tall; the log confirms the extra padding.")}
	return cv.Turn{User: "Compare these two screenshots against the log.", Images: pics[:2],
		Files: []cv.File{{ID: id + "/attached", Path: "logs/frame.txt", Mime: "text/plain", Kind: cv.TextFile, Text: logText}}, Reply: r}
}

// artifacts: output files as cards, a picture the model made and a chart.
func artifacts(id string) cv.Turn {
	pics := pictures()
	r := done(cv.Completed, 41)
	r.Blocks = []cv.Block{
		tools(call(id, "write", "Write file", "writing", "docs/scroll-report.md")),
		text("The report, the benchmark and the exports are ready."),
		{Kind: cv.FilesBlock, Files: outputs(id)},
		{Kind: cv.ImagesBlock, Images: pics[2:]},
		{Kind: cv.VisualBlock, Visual: &cv.Visual{Title: "Frames per day", Mermaid: chart}},
	}
	return cv.Turn{User: "Write the scroll report and export the invoice.", Reply: r}
}

// grounded: a search the answer is grounded on, cited in place.
func grounded(id string) cv.Turn {
	r := done(cv.Completed, 12)
	answer := "Go 1.27 keeps the loop variable per iteration and adds iterator helpers to the standard library."
	r.Blocks = []cv.Block{
		{Kind: cv.SearchBlock, Sources: []cv.Citation{
			{URI: "https://go.dev/doc/go1.27", Title: "Go 1.27 Release Notes"},
			{URI: "https://go.dev/blog/range-functions", Title: "Range Over Function Types"},
		}},
		text(answer),
	}
	r.Citations = []cv.Citation{{URI: "https://go.dev/doc/go1.27", Title: "Go 1.27 Release Notes", End: 45}, {URI: "https://go.dev/blog/range-functions"}}
	return cv.Turn{User: "What changed in Go 1.27?", Reply: r}
}

// retried: model requests retried, one with the provider's message, and
// one that gave up.
func retried(id string) cv.Turn {
	r := done(cv.Failed, 75)
	r.Blocks = []cv.Block{
		text("Fetching the pull requests…"),
		{Kind: cv.RetryBlock, Retry: &cv.Retry{Attempt: 2, Limit: 5, Reason: "chat_model_rate_limited"}},
		{Kind: cv.RetryBlock, Retry: &cv.Retry{Attempt: 5, Limit: 5, Reason: "chat_model_server_overloaded", Exhausted: true,
			Detail: `529 {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}`}},
		{Kind: cv.ErrorBlock, Label: "Model service overloaded", Text: "The provider stayed overloaded after 5 attempts."},
	}
	return cv.Turn{User: "Summarize the open pull requests.", Reply: r}
}

// changed: edits with the changes card, one file already undone.
func changed(id string) cv.Turn {
	r := done(cv.Completed, 58)
	r.Blocks = []cv.Block{
		tools(call(id, "edit", "Edit file", "pencil", "internal/kit/menu.go"), call(id, "edit", "Edit file", "pencil", "internal/shell/rail.go")),
		text("The menus now share one floating look."),
	}
	r.Changes = &cv.Changes{Files: []cv.ChangedFile{
		{Path: "internal/kit/menu.go", Added: 262},
		{Path: "internal/shell/rail.go", Added: 18, Removed: 14},
		{Path: "internal/panel/panel.go", Added: 22, Removed: 20},
		{Path: "internal/composer/interactions.go", Added: 9, Removed: 5},
		{Path: "docs/theme.md", Added: 17, Removed: 11},
	}, Undone: map[string]bool{"docs/theme.md": true}}
	return cv.Turn{User: "Give every menu the floating look.", Reply: r}
}

// noticed: background results delivered with a turn.
func noticed(id string) cv.Turn {
	r := done(cv.Completed, 3)
	r.Notice = true
	return cv.Turn{Notices: []cv.Notice{
		{Title: "go test ./...", Outcome: cv.OutcomeCompleted},
		{Title: "Audit the icon set", Agent: true, Outcome: cv.OutcomeFailed},
		{Title: "npm run watch", Outcome: cv.OutcomeStalled},
	}, Reply: r}
}

// compacted: a compaction turn, finished with its summary, failed, or
// running, by the turn.
func compacted(id string) cv.Turn {
	k := &cv.Compaction{State: cv.Compacted, Summary: "The user ports Sailry's desktop UI to MyGo. Done: theme tokens, shell, sidebar, right panel. Next: the transcript's live features."}
	switch len(id) % 3 {
	case 1:
		k = &cv.Compaction{State: cv.CompactionFailed, Cause: "context_window_exceeded: the summary did not fit"}
	case 2:
		k = &cv.Compaction{State: cv.CompactionUnneeded}
	}
	return cv.Turn{Compaction: k, Reply: done(cv.Completed, 0)}
}

// stamp gives the turns their send and finish times, a few minutes apart
// up to the dataset's clock.
func stamp(turns []cv.Turn) {
	for i := range turns {
		sent := Now.Add(-time.Duration(len(turns)-i) * 7 * time.Minute)
		turns[i].Sent = sent
		if !turns[i].Reply.Status.Active() {
			turns[i].Reply.Finished = sent.Add(turns[i].Reply.Elapsed + 2*time.Second)
		}
	}
}

// usage is a conversation's preview metrics, from its turns.
func usage(turns int) *cv.Usage {
	return &cv.Usage{Tokens: 1840 * turns, Speed: 62.5, Cost: 0.0042 * float64(turns), Cached: 1210 * turns, Turns: turns,
		Context: math.Min(0.92, 0.04*float64(turns))}
}

// pageSize is how many turns a page of history holds: the live view reads its
// history 20 turns at a time, so its message navigation ticks the turns of
// the pages loaded, not the whole session's.
const pageSize = 20

// paged shows c's last page of turns and keeps the rest as older pages.
func paged(c *Conversation) {
	if n := len(c.Turns) - pageSize; n > 0 {
		c.older, c.Turns = c.Turns[:n], c.Turns[n:]
		c.Session.Older = len(c.older)
	}
}

// LoadAll loads every older page at once, for a benchmark of the whole
// history.
func LoadAll(c *Conversation) {
	c.Turns = append(c.older, c.Turns...)
	c.older, c.Session.Older, c.Session.LoadingOlder = nil, 0, false
}

// pageAfter is how long the preview takes to load a page.
const pageAfter = 600 * time.Millisecond

// LoadOlder asks for the page before the first turn shown; Advance moves
// it in once pageAfter passed.
func LoadOlder(c *Conversation) {
	if c.Session.Older == 0 || c.Session.LoadingOlder {
		return
	}
	c.Session.LoadingOlder, c.Session.OlderFailed = true, false
	c.paging = time.Time{}
}

// page runs a requested page on the frame clock, reporting whether one
// still loads.
func page(c *Conversation, now time.Time) bool {
	if !c.Session.LoadingOlder {
		return false
	}
	if c.paging.IsZero() {
		c.paging = now
	}
	if now.Sub(c.paging) < pageAfter {
		return true
	}
	n := min(pageSize, len(c.older))
	c.Turns = append(append([]cv.Turn(nil), c.older[len(c.older)-n:]...), c.Turns...)
	c.older = c.older[:len(c.older)-n]
	c.Session.Older, c.Session.LoadingOlder = len(c.older), false
	return false
}

// childTurns are a subagent's session for its tab: a short preview run.
func ChildTurns(id, title string) []cv.Turn {
	r := done(cv.Completed, 18)
	r.Blocks = []cv.Block{
		tools(call(id, "read", "Read file", "book-open", "internal/icons/icons.go"), call(id, "grep", "Search", "search", `"load("`)),
		text("All 89 icons resolve; none is unused."),
	}
	t := []cv.Turn{{ID: id + "/0", User: title, Reply: r}}
	stamp(t)
	return t
}
