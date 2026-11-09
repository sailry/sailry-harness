package preview

import (
	"fmt"
	"strings"
	"time"

	cv "github.com/sailry/sailry-go/internal/conversation"
)

// The preview's turns: a set of scenes that between them show every kind of
// block, tool output and state the transcript draws. They are made-up
// projections; no tool ever ran.

// scene builds a finished turn; id prefixes its call IDs, which must be
// unique across the dataset.
type scene func(id string) cv.Turn

var scenes = []scene{explore, edit, failed, refused, sessions, questions, delegated, planned, answered}

// Turns makes n finished turns; seed varies them between conversations.
func Turns(conv string, n, seed int) []cv.Turn {
	out := make([]cv.Turn, n)
	for i := range out {
		id := fmt.Sprintf("%s/%d", conv, i)
		t := scenes[(i+seed)%len(scenes)](id)
		t.ID = id
		out[i] = t
	}
	return out
}

func call(id, name, label, icon, detail string) cv.Call {
	return cv.Call{ID: id + "/" + name + "/" + detail, Name: name, Label: label, Icon: icon, Detail: detail, Code: true}
}

func tools(calls ...cv.Call) cv.Block     { return cv.Block{Kind: cv.ToolsBlock, Calls: calls} }
func text(s string) cv.Block              { return cv.Block{Kind: cv.TextBlock, Text: s} }
func reasoning(s string) cv.Block         { return cv.Block{Kind: cv.ReasoningBlock, Text: s} }
func secs(n int) time.Duration            { return time.Duration(n) * time.Second }
func output(s string) *cv.Output          { return &cv.Output{Text: s} }
func paths(p ...string) *cv.Output        { return &cv.Output{Paths: p} }
func done(s cv.RunStatus, d int) cv.Reply { return cv.Reply{Status: s, Elapsed: secs(d)} }

// explore: reasoning, then a sequence of reads and searches (one group
// listing paths) before the answer.
func explore(id string) cv.Turn {
	read := call(id, "read", "Read file", "book-open", "internal/conversation/list.go")
	read.Output = output("func (l *list) estimate(i int) float32 {\n\tif h, ok := l.measured[i]; ok {\n\t\treturn h\n\t}\n\treturn l.average\n}")
	grep1 := call(id, "search", "Search", "search", `"estimate" in internal/`)
	grep1.Output = paths("internal/conversation/list.go", "internal/conversation/bench_test.go", "internal/sidebar/sidebar.go")
	grep2 := call(id, "search", "Search", "search", `"measured" in internal/`)
	grep2.Output = paths("internal/conversation/list.go")
	r := done(cv.Completed, 14)
	r.Blocks = []cv.Block{
		reasoning("The user asks how rows are measured. I should read the list first, then look for where estimates are used."),
		tools(read, grep1, grep2),
		text("The list measures each row the first time it shows and **estimates** the rest from the average, so the scroll bar settles as you move.\n\n- Rows keep their place by key, not by offset.\n- Heights are cached per width."),
	}
	return cv.Turn{User: "Can you read `internal/conversation/list.go` and explain how rows are measured?", Reply: r}
}

// edit: a diff and a command whose output is longer than the 100 lines
// kept.
func edit(id string) cv.Turn {
	e := call(id, "edit", "Edit file", "writing", "internal/conversation/view_test.go")
	e.Output = &cv.Output{Diff: &cv.Diff{Path: "internal/conversation/view_test.go", Added: 6, Removed: 1, Lines: []cv.DiffLine{
		{Op: ' ', Text: "func TestScrollMiddle(t *testing.T) {"},
		{Op: '-', Text: "\tt.Skip(\"todo\")"},
		{Op: '+', Text: "\ts, cv, tt := bigTester(t)"},
		{Op: '+', Text: "\tl := s.List(cv.ID)"},
		{Op: '+', Text: "\tl.ScrollTo(1000, ui.Center)"},
		{Op: '+', Text: "\ttt.Frame()"},
		{Op: '+', Text: "\tif f, l := l.Visible(); f > 1000 || l < 1000 {"},
		{Op: '+', Text: "\t\tt.Fatal(f, l)"},
		{Op: ' ', Text: "}"},
	}}}
	var lines []string
	for i := range 140 {
		lines = append(lines, fmt.Sprintf("=== RUN   TestCase%03d\n--- PASS: TestCase%03d (0.00s)", i, i))
	}
	lines = append(lines, "PASS", "ok  \tgithub.com/sailry/sailry-go/internal/conversation\t0.412s")
	run := call(id, "command", "Command", "command", "go test ./internal/conversation")
	run.Output = &cv.Output{Text: strings.Join(lines, "\n"), Command: true}
	r := done(cv.Completed, 38)
	r.Blocks = []cv.Block{
		tools(e, run),
		text("Done. The new test scrolls by `ScrollTo(1000, ui.Center)` and checks that message **#1000** is on screen.\n\n| Step | Result |\n|---|---|\n| Build | ok |\n| `go test ./...` | ok |"),
	}
	return cv.Turn{User: "Add a test that scrolls to the middle of the transcript.", Reply: r}
}

// failed: a runtime failure (danger), a tool's own failure (warning), and
// the turn's error.
func failed(id string) cv.Turn {
	f := call(id, "command", "Command", "command", "gofmt -l .")
	f.Failed, f.State, f.Error = true, cv.StateFailed, "exit status 2"
	f.Output = &cv.Output{Failure: "internal/conversation/list.go:12:1: expected declaration, found '}'"}
	w := call(id, "fetch", "Fetch page", "globe", "https://go.dev/doc/effective_go")
	w.Warning, w.Error = true, "The page did not answer in 30 s"
	w.Output = &cv.Output{Failure: "Request timed out after 30 s", ToolFailure: true}
	r := done(cv.Failed, 9)
	r.Blocks = []cv.Block{
		tools(f),
		tools(w),
		{Kind: cv.ErrorBlock, Label: "Model request failed", Text: "The provider closed the stream before the response ended (HTTP 502)."},
	}
	return cv.Turn{User: "Why does the composer lose focus after sending?", Reply: r}
}

// refused: denied, interrupted and unexecuted calls of a stopped run.
func refused(id string) cv.Turn {
	d := call(id, "command", "Command", "command", "rm -rf build/")
	d.Warning, d.State, d.Error = true, cv.Denied, "Denied by the user"
	i := call(id, "write", "Write file", "writing", "internal/conversation/list.go")
	i.Warning, i.State = true, cv.StateInterrupted
	n := call(id, "write2", "Write file", "writing", "internal/conversation/rows.go")
	n.Name, n.Warning, n.State = "write", true, cv.NotExecuted
	r := done(cv.Cancelled, 21)
	r.Blocks = []cv.Block{tools(d, i, n), text("Renamed in 4 files before stopping. No public API changed.")}
	return cv.Turn{User: "Rename `rowHeight` to `estimate` everywhere.", Reply: r}
}

// sessions: built-in tools, a table, notices and a partial result.
func sessions(id string) cv.Turn {
	l := cv.Call{ID: id + "/list", Name: "list_sessions", LabelKey: "tool_list_sessions", Icon: "tasks", Detail: "sailry-go", Code: true}
	l.Output = &cv.Output{Table: &cv.Table{Columns: []string{"Session", "Updated", "Turns"}, Rows: [][]string{
		{"Port the neutral theme tokens", "25 min ago", "7"},
		{"Copy locale keys for the shell", "3 h ago", "4"},
		{"Resizable sidebar split", "1 day ago", "3"},
		{"Tester coverage for the composer", "2 days ago", "5"},
		{"Settings providers list", "3 days ago", "2"},
		{"Icon tiles for plugins", "5 days ago", "2"},
	}}, Partial: true}
	s := cv.Call{ID: id + "/search", Name: "search_session", LabelKey: "tool_search_session", Icon: "search", Detail: `"benchmark"`, Code: true}
	s.Output = &cv.Output{Text: "Turn 12: BenchmarkScroll-1   2000   412 µs/op\nTurn 31: BenchmarkJump-1   800   1.79 ms/op", Notices: []string{"2 of 9 matches shown"}}
	k := cv.Call{ID: id + "/skill", Name: "load_skill", LabelKey: "tool_load_skill", Icon: "book-open", Detail: "go-benchmarks"}
	o := cv.Call{ID: id + "/out", Name: "read_output", LabelKey: "tool_read_output", Icon: "file-text", Detail: "bench.txt", Code: true}
	o.Output = output("BenchmarkScroll-1   2000   412 µs/op   0 B/op")
	r := done(cv.Completed, 7)
	r.Blocks = []cv.Block{tools(l, s, k, o), text("Results on this machine:\n\n```text\nBenchmarkScroll-1   2000   412 µs/op   0 B/op\n```\n\nThat is well under a 16.7 ms frame.")}
	return cv.Turn{User: "Run the benchmarks again with GOMAXPROCS=1.", Reply: r}
}

// questions: the history rows of questions in each state.
func questions(id string) cv.Turn {
	q := func(n string, st cv.QuestionState, answers ...string) cv.Block {
		return cv.Block{Kind: cv.QuestionBlock, Question: &cv.Question{CallID: id + "/q" + n, Prompt: "Which theme should the gallery open in?", Options: []string{"Light", "Dark", "System"}, State: st, Answers: answers}}
	}
	r := done(cv.Completed, 64)
	r.Blocks = []cv.Block{
		q("1", cv.QuestionAnswered, "Dark"),
		q("2", cv.QuestionDeclined),
		q("3", cv.QuestionCancelled),
		q("4", cv.QuestionUnconsumed, "System"),
		q("5", cv.QuestionInterrupted),
		q("6", cv.QuestionTimedOut),
		text("The gallery opens in **dark** and follows the system after."),
	}
	return cv.Turn{User: "Ask me before choosing the gallery's theme.", Reply: r}
}

// delegated: one subagent, then three.
func delegated(id string) cv.Turn {
	r := done(cv.Completed, 3725)
	r.Blocks = []cv.Block{
		{Kind: cv.ChildrenBlock, Children: []cv.Child{{ID: id + "/c1", Title: "Audit the icon set", Model: "Claude Haiku 5.5", Effort: "low", Status: cv.ChildCompleted}}},
		text("The icon audit found two unused SVGs."),
		{Kind: cv.ChildrenBlock, Children: []cv.Child{
			{ID: id + "/c2", Title: "Port light tokens", Model: "Claude Sonnet 5.5", Effort: "medium", Status: cv.ChildCompleted},
			{ID: id + "/c3", Title: "Port dark tokens", Model: "Claude Sonnet 5.5", Effort: "medium", Status: cv.ChildFailed},
			{ID: id + "/c4", Title: "", Model: "Claude Haiku 5.5", Status: cv.ChildCancelled},
		}},
		text("Light tokens are ported; the dark pass failed and is ready to run again."),
	}
	return cv.Turn{User: "Split the theme port across subagents.", Reply: r}
}

// planned: a task's progress with every step state; an interrupted run.
func planned(id string) cv.Turn {
	r := done(cv.Interrupted, 95)
	r.Blocks = []cv.Block{
		{Kind: cv.ProgressBlock, Steps: []cv.Step{
			{Text: "Read the reference layout", State: cv.StepCompleted},
			{Text: "Skip the mobile layout", State: cv.StepSkipped},
			{Text: "Port the settings frame", State: cv.StepFailed},
			{Text: "Wait for the theme review", State: cv.StepBlocked},
			{Text: "Gallery screenshots", State: cv.StepPaused},
			{Text: "Write the docs", State: cv.StepPending},
		}},
		text("Stopped while porting the settings frame."),
	}
	return cv.Turn{User: "Plan the settings port and start on it.", Reply: r}
}

// answered: a plain answer, without work.
func answered(id string) cv.Turn {
	r := done(cv.Completed, 2)
	r.Blocks = []cv.Block{text("Focus moves to the send button because it is clicked with the pointer. Keeping the focus in the input is one line:\n\n```go\napp.input.Focus()\n```")}
	return cv.Turn{User: "Why does Send take the focus?", Reply: r}
}

// live is the reply still running in a conversation: a group of calls with
// one running and one awaiting approval, subagents at work and a task in
// progress.
func live(id string) cv.Turn {
	read := call(id, "read", "Read file", "book-open", "internal/theme/theme.go")
	read.Output = output("package theme")
	run := call(id, "command", "Command", "command", "go test -bench . -run ^$")
	run.Running = true
	ask := call(id, "write", "Write file", "writing", "internal/theme/controls.go")
	ask.Pending, ask.State = true, cv.Pending
	vet := call(id, "command", "Command", "command", "go vet ./...")
	vet.Pending, vet.State = true, cv.Pending
	r := cv.Reply{Status: cv.Running, Elapsed: secs(65), Phase: cv.Generating}
	r.Blocks = []cv.Block{
		reasoning("Port the controls next; the benchmark runs meanwhile."),
		tools(read, run),
		{Kind: cv.ChildrenBlock, Children: []cv.Child{
			{ID: id + "/c1", Title: "Measure scroll frames", Model: "Claude Haiku 5.5", Effort: "low", Status: cv.ChildRunning},
			{ID: id + "/c2", Title: "Check contrast", Model: "Claude Haiku 5.5", Effort: "low", Status: cv.ChildCompleted},
		}},
		{Kind: cv.ProgressBlock, Steps: []cv.Step{
			{Text: "Port the palette", State: cv.StepCompleted},
			{Text: "Port the controls", State: cv.StepInProgress},
			{Text: "Run the benchmarks", State: cv.StepPending},
		}},
		tools(ask, vet),
		{Kind: cv.QuestionBlock, Question: &cv.Question{CallID: id + "/ask", Prompt: "Keep the 2x3 loader or use Kit's spinner?", State: cv.QuestionPending}},
	}
	return cv.Turn{ID: id, User: "Port the controls and benchmark the transcript.", Reply: r}
}
