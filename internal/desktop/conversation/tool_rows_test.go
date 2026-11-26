package conversation

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestToolRowsKeepExecutionInput(t *testing.T) {
	const command = "go test ./internal/node/agent/..."
	const raw = `{"error":"process exited with code 2","stdout":"original output"}`
	for _, test := range []struct {
		name string
		call Call
	}{
		{"failed", Call{Failed: true}},
		{"denied", Call{State: Denied, Warning: true}},
		{"interrupted", Call{State: StateInterrupted, Warning: true}},
		{"not_executed", Call{State: NotExecuted, Warning: true}},
		{"tool_failure", Call{Warning: true}},
		{"background", Call{Background: &Background{Outcome: OutcomeFailed, Waiting: true}}},
	} {
		t.Run(test.name, func(t *testing.T) {
			call := test.call
			call.ID, call.Name, call.Label, call.Icon = "command", "command", "Command", "square-terminal"
			call.Detail, call.Code = command, true
			call.Error = i18n.In("zh-CN", "tool_failed")
			call.Output = &Output{Text: raw}
			state := New()
			var status ui.Color
			var danger, warning ui.Color
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				palette := theme.Of(c)
				status, danger, warning = rowColor(palette, call), palette.Danger, palette.Warning
				state.row(c, call)
			}, 900, 300)
			tester.SetLocale("zh-CN")
			tester.Frame()
			if !tester.HasText("Command") || !tester.HasText(command) || tester.HasText(raw) {
				t.Fatalf("row lost its execution input or has the wrong default disclosure: %q", tester.Texts())
			}
			for _, key := range []string{"tool_failed", "approval_rejected", "tool_interrupted", "tool_not_executed", "background_result_waiting"} {
				if tester.HasText(i18n.In("zh-CN", key)) {
					t.Fatalf("tool row contains result status %s: %q", key, tester.Texts())
				}
			}
			if call.Failed && status != danger || (call.Warning || call.State != StateNone || call.Background != nil) && status != warning {
				t.Fatal("hiding status text changed the failure or warning color")
			}
			if err := tester.Click("Command"); err != nil {
				t.Fatal(err)
			}
			tester.Frame()
			if !tester.HasText(raw) || !tester.HasText(command) || tester.HasText(call.Error) {
				t.Fatalf("expanded output changed the original result or replaced the execution input: %q", tester.Texts())
			}
		})
	}
}

func TestToolFailureDetailsRemainAvailable(t *testing.T) {
	const failure = "process exited with code 2"
	call := Call{ID: "failure", Name: "command", Label: "Command", Detail: "go test ./...", Failed: true, Error: failure, Output: &Output{Failure: failure}}
	state := New()
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		state.row(c, call)
	}, 900, 300)
	tester.Frame()
	if tester.HasText(failure) || !tester.HasText(call.Detail) {
		t.Fatalf("failed row did not show just its input: %q", tester.Texts())
	}
	if err := tester.Click("Command"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText(failure) || !tester.HasText(call.Detail) {
		t.Fatalf("opening the failed row lost its input or message: %q", tester.Texts())
	}
	if err := tester.Click("Command"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if tester.HasText(failure) || !tester.HasText(call.Detail) {
		t.Fatalf("collapsing the failure changed its command: %q", tester.Texts())
	}
	if err := tester.Click("Command"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText(failure) {
		t.Fatalf("original failure cannot be opened: %q", tester.Texts())
	}
}

func TestFailureToneKeepsRoutineToolErrorsMuted(t *testing.T) {
	var toolColor, runtimeColor, muted, danger ui.Color
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		p := theme.Of(c)
		muted, danger = p.MutedForeground, p.Danger
		toolColor = failureTone(c, &Output{Failure: "old text was not found", ToolFailure: true})
		runtimeColor = failureTone(c, &Output{Failure: "runtime crash"})
	}, 400, 120)
	tester.Frame()
	if toolColor != muted {
		t.Fatalf("routine tool failure tone = %v, want muted %v", toolColor, muted)
	}
	if runtimeColor != danger {
		t.Fatalf("runtime failure tone = %v, want danger %v", runtimeColor, danger)
	}
}

// An open command row shows the command's own output and nothing the
// application wrote about its state, whatever that state is.
func TestToolCommandShowsOnlyOutput(t *testing.T) {
	for _, test := range []struct {
		name       string
		status     Background
		historical bool
	}{
		{"completed", Background{Outcome: OutcomeCompleted}, false},
		{"nonzero exit", Background{Outcome: OutcomeFailed, Exit: new(1), Waiting: true}, false},
		{"historical completion", Background{Outcome: OutcomeCompleted, Exit: new(0)}, true},
		{"historical nonzero exit", Background{Outcome: OutcomeFailed, Exit: new(1)}, true},
		{"running", Background{Running: true}, false},
		{"failed, result waiting", Background{Outcome: OutcomeFailed, Waiting: true}, false},
		{"timed out", Background{Outcome: OutcomeTimedOut}, false},
		{"stopped", Background{Outcome: OutcomeStopped}, false},
		{"unknown", Background{Outcome: OutcomeUnknown}, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			call := Call{ID: "command", Label: "Command"}
			const text = "command result"
			if test.historical {
				call.BackgroundDetail = func() BackgroundDetail {
					return BackgroundDetail{Output: &Output{Text: text, Command: true}}
				}
			} else {
				test.status.Preview = text
				call.Background = &test.status
			}
			state := New()
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				state.row(c, call)
			}, 900, 300)
			tester.SetLocale("zh-CN")
			tester.Frame()
			if tester.HasText(text) {
				t.Fatalf("a command that exited opened on its own: %q", tester.Texts())
			}
			if err := tester.Click("Command"); err != nil {
				t.Fatal(err)
			}
			tester.Frame()
			if got := tester.Texts(); !slices.Equal(got, []string{"Command", text, i18n.In("zh-CN", "tool_copy")}) {
				t.Fatalf("expanded command should show only its output: %q", got)
			}
		})
	}
}

func TestOpenToolDetailLifetime(t *testing.T) {
	var reads []func(*Output)
	var cancels, attachments int
	call := Call{ID: "example", Label: "Tool", DetailRevision: "one", Output: &Output{}, Load: func(apply func(*Output)) func() {
		reads = append(reads, apply)
		return func() { cancels++ }
	}}
	state := New()
	mounted := true
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		if mounted {
			state.row(c, call)
		}
	}, 640, 400)
	frame := func() { tester.Frame() }
	toggle := func() {
		t.Helper()
		if err := tester.Click("Tool"); err != nil {
			t.Fatal(err)
		}
		frame()
	}
	frame()
	if len(reads) != 0 {
		t.Fatal("folded result fetched")
	}
	toggle()
	if len(reads) != 1 {
		t.Fatalf("open reads = %d", len(reads))
	}
	attach := func() { attachments++ }
	reads[0](&Output{Text: "first result", AttachCards: attach})
	frame()
	if !tester.HasText("first result") || len(reads) != 1 || attachments != 1 {
		t.Fatal("open detail not retained")
	}
	reads[0](&Output{Text: "replaced result", AttachCards: attach})
	call.DetailRevision = "two"
	frame()
	if len(reads) != 2 || cancels != 1 {
		t.Fatalf("revision reads %d cancels %d", len(reads), cancels)
	}
	reads[0](&Output{Text: "stale result", AttachCards: attach})
	frame()
	if tester.HasText("first result") || tester.HasText("stale result") || tester.HasText("replaced result") || attachments != 1 {
		t.Fatal("stale revision retained")
	}
	toggle()
	if cancels != 2 {
		t.Fatalf("close cancels = %d", cancels)
	}
	toggle()
	if len(reads) != 3 {
		t.Fatalf("reopen reads = %d", len(reads))
	}
	mounted = false
	reads[2](&Output{Text: "unmounted result", AttachCards: attach})
	frame()
	if cancels != 3 || attachments != 1 || tester.HasText("unmounted result") {
		t.Fatalf("unmount cancels = %d, card attachments = %d", cancels, attachments)
	}
}

func TestFoldedToolImageIsResolvedOnlyWhenShown(t *testing.T) {
	call := Call{ID: "image-tool", Name: "read_file", Label: "Read file", Output: &Output{Images: []Image{{ID: "attachment", Name: "result.png"}}}}
	state := New()
	var reads int
	state.resolveImage = func(session string, image Image) Image { reads++; return image }
	tester := ui.NewTester(func(c *ui.Context) { theme.Apply(c, "", theme.Light); state.row(c, call) }, 640, 400)
	tester.Frame()
	if reads != 0 {
		t.Fatal("folded tool requested its bitmap")
	}
	if err := tester.Click("Read file"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if reads == 0 {
		t.Fatal("open tool did not request its bitmap")
	}
	if err := tester.Click("Read file"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	before := reads
	tester.Frame()
	if reads != before {
		t.Fatal("closed tool kept requesting its bitmap")
	}
}
