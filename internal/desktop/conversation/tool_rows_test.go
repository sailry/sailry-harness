package conversation

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestToolRowsKeepExecutionInputWithoutResultText(t *testing.T) {
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
				t.Fatalf("collapsed row lost its execution input or displayed the result: %q", tester.Texts())
			}
			for _, key := range []string{"tool_failed", "approval_rejected", "tool_interrupted", "tool_not_executed", "background_result_waiting"} {
				if tester.HasText(i18n.In("zh-CN", key)) {
					t.Fatalf("tool row contains result status %s: %q", key, tester.Texts())
				}
			}
			if call.Failed && status != danger || (call.State != StateNone || call.Background != nil) && status != warning {
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
		t.Fatalf("collapsed row shows a failure in place of the command: %q", tester.Texts())
	}
	if err := tester.Click("Command"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText(failure) {
		t.Fatalf("original failure cannot be opened: %q", tester.Texts())
	}
}
