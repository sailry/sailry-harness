package acceptance

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func (run *caseRun) commandFixture() error {
	output, err := run.command(run.ctx, "inspect_host", nil)
	if err != nil {
		return err
	}
	host, err := outputAs[protocol.HostInfo](output, "host_info")
	if err != nil {
		return err
	}
	if host.Os == nil || !strings.Contains(strings.ToLower(*host.Os), "darwin") && !strings.Contains(strings.ToLower(*host.Os), "macos") && !strings.Contains(strings.ToLower(*host.Os), "linux") {
		return &caseIssue{"skipped", "This command fixture requires a POSIX execution Node"}
	}
	return run.standard()
}

func caseCommand(run *caseRun) error {
	return commandExit(run, "printf 'SAILRY-COMMAND-OK\\n'", "SAILRY-COMMAND-OK", 0)
}

func caseCommandFailure(run *caseRun) error {
	return commandExit(run, "printf 'SAILRY-EXIT7\\n'; exit 7", "SAILRY-EXIT7", 7)
}

func caseCommandMissing(run *caseRun) error {
	return commandExit(run, "printf 'SAILRY-EXIT127\\n'; exit 127", "SAILRY-EXIT127", 127)
}

func commandExit(run *caseRun, command, marker string, expected int32) error {
	if err := run.commandFixture(); err != nil {
		return err
	}
	view, err := run.ask(fmt.Sprintf("Call run_command exactly once with command=%q, access=\"workspace\", background=false, timeout_ms=5000. It intentionally exits %d. Do not retry, repair, install or run another command. Report its actual exit code.", command, expected))
	if err != nil {
		return err
	}
	call, detail, err := run.requireTool(view, "run_command")
	if err != nil {
		return err
	}
	count := 0
	for _, value := range view.tools() {
		if value.name == "run_command" {
			count++
		}
	}
	if count != 1 {
		return fail("The exact command fixture was retried instead of run once")
	}
	return run.assertCommandCompletion(call, detail, command, marker, expected)
}

func (run *caseRun) assertCommandCompletion(call toolCall, detail protocol.ToolDetail, command, marker string, expected int32) error {
	if argumentString(detail, "command") != command || argumentString(detail, "access") == "unrestricted" {
		return fail("Command tool changed the exact restricted fixture command")
	}
	if call.summary.Failure != nil && call.summary.Failure.Origin == protocol.ToolFailureOriginRuntime {
		return blocked("The execution Node could not run the restricted command fixture")
	}
	if expected == 0 && call.summary.Failure != nil || expected != 0 && (call.summary.Failure == nil || call.summary.Failure.Origin != protocol.ToolFailureOriginTool) {
		return fail("Command completion has the wrong tool failure classification")
	}
	if detail.Result == nil {
		return fail("Command has no canonical raw result")
	}
	var raw struct {
		Kind string                     `json:"kind"`
		Data protocol.ProcessCompletion `json:"data"`
	}
	if json.Unmarshal(*detail.Result, &raw) != nil || raw.Kind != "command_result" {
		return fail("Command raw result did not retain its process completion")
	}
	code, err := decode[int32](raw.Data.Outcome.Data)
	if err != nil || raw.Data.Outcome.Kind != "exited" || code != expected || !strings.Contains(raw.Data.Stdout.Text, marker) {
		return fail("Canonical command result has the wrong exit code or stdout marker")
	}
	// Resolved.Command is optional presentation metadata. Error results retain
	// their original payload without a plugin content projection, so their raw
	// completion above is authoritative even when that display field is absent.
	if detail.Resolved != nil && detail.Resolved.Command != nil {
		outcome := detail.Resolved.Command
		resolvedCode, err := decode[int32](outcome.Data)
		if err != nil || outcome.Kind != "exited" || resolvedCode != expected {
			return fail("Projected command outcome differs from the canonical completion")
		}
	}
	run.evidence("command", fmt.Sprintf("Canonical raw result retains exit=%d and the fixture stdout marker", expected))
	return nil
}

func (run *caseRun) processes() ([]protocol.ProcessInfo, error) {
	output, err := run.command(run.ctx, "list_commands", protocol.CommandListCommandsPayload{Session: run.session.ID})
	if err != nil {
		return nil, err
	}
	return outputAs[[]protocol.ProcessInfo](output, "commands")
}

func (run *caseRun) processOutput(id protocol.RequestID, until *string) (protocol.ProcessSnapshot, error) {
	output, err := run.command(run.ctx, "read_command", protocol.CommandReadCommandPayload{Session: run.session.ID, ID: id, WaitMs: 3000, Until: until})
	if err != nil {
		return protocol.ProcessSnapshot{}, err
	}
	return outputAs[protocol.ProcessSnapshot](output, "command_output")
}

func caseBackground(run *caseRun) error {
	if err := run.commandFixture(); err != nil {
		return err
	}
	const command = "printf 'SAILRY-BACKGROUND\\n'; sleep 1; printf 'SAILRY-DONE\\n'"
	view, err := run.ask(fmt.Sprintf("Call run_command with command=%q, access=\"workspace\", background=true, follow_up=false, timeout_ms=5000. Copy its returned UUID to read_command with wait_ms=3000 and until=\"SAILRY-DONE\". Do not start another command.", command))
	if err != nil {
		return err
	}
	if _, _, err := run.requireTool(view, "read_command"); err != nil {
		return err
	}
	_, detail, err := run.requireTool(view, "run_command")
	if err != nil || argumentString(detail, "command") != command {
		return fail("Background tool did not run the exact fixture command")
	}
	processes, err := run.processes()
	if err != nil {
		return err
	}
	for _, process := range processes {
		if process.Command != command {
			continue
		}
		marker := "SAILRY-DONE"
		output, err := run.processOutput(process.ID, &marker)
		if err != nil {
			return err
		}
		if !strings.Contains(output.Stdout.Text, "SAILRY-BACKGROUND") || !strings.Contains(output.Stdout.Text, marker) || output.Info.Status.Kind != "finished" {
			return fail("Retained background process did not finish with both output markers")
		}
		run.evidence("process", process.ID.String()+" finished; retained stdout inspected")
		return nil
	}
	return fail("Background command was not retained in this session")
}

func caseCommandInput(run *caseRun) error {
	if err := run.commandFixture(); err != nil {
		return err
	}
	// Background commands have no process timeout. The explicit stop below and
	// case cleanup own this fixture's lifetime, including approval/model waits.
	turn, err := run.submit("Start run_command with command=\"cat\", access=\"workspace\", background=true, follow_up=false, timeout_ms=10000. Use write_command to send SAILRY-INPUT followed by one newline to its returned UUID. Use read_command with wait_ms=2000 and until=\"SAILRY-INPUT\", then stop_command for the same UUID. Wait for the requested input and stop approvals. You may use tool_search to discover write_command or stop_command if needed. Do not start another process or use another tool.")
	if err != nil {
		return err
	}
	view, err := run.observe(turn.ID, func(view turnView) (bool, error) {
		if len(view.questions()) != 0 {
			return false, blocked("Interactive command fixture asked an unexpected question; no answer was sent")
		}
		for _, approval := range view.approvals() {
			if run.resolved[approval.ID] {
				continue
			}
			process, err := run.inputProcess(view)
			if err != nil {
				return false, err
			}
			matched := false
			for _, call := range view.tools() {
				if call.reference.Entry != approval.Entry || call.reference.Index != approval.Index {
					continue
				}
				detail, err := run.detail(call)
				if err != nil {
					return false, err
				}
				matched = commandInputApprovalMatches(call.name, detail, process.ID)
			}
			if !matched || approval.Session != run.session.ID || approval.Turn != turn.ID {
				return false, blocked("Approval differs from the exact owned command input or stop; no answer was sent")
			}
			if _, err := run.command(run.ctx, "resolve_approval", protocol.CommandResolveApprovalPayload{Session: run.session.ID, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: "approve"}}); err != nil {
				return false, err
			}
			if run.resolved == nil {
				run.resolved = map[protocol.ApprovalID]bool{}
			}
			run.resolved[approval.ID] = true
			run.evidence("approval", "Approved only the fixture input or stop for owned process "+process.ID.String())
		}
		return false, nil
	}, true)
	if err != nil {
		return err
	}
	if view.run.Status != protocol.ConversationStatusCompleted {
		return fail("Interactive command turn did not complete")
	}
	if err := run.assertCapture(turn); err != nil {
		return err
	}
	process, err := run.inputProcess(view)
	if err != nil {
		return err
	}
	calls := []toolCall{}
	for _, call := range view.tools() {
		if call.name != "tool_search" {
			calls = append(calls, call)
		}
	}
	if len(calls) != 4 || calls[0].name != "run_command" || calls[1].name != "write_command" || calls[2].name != "read_command" || calls[3].name != "stop_command" {
		return fail("Interactive fixture did not execute its exact start, input, read and stop sequence")
	}
	for _, name := range []string{"write_command", "read_command", "stop_command"} {
		call, detail, err := run.requireTool(view, name)
		if err != nil {
			return err
		}
		if call.summary.Failure != nil || detail.Result == nil || argumentString(detail, "id") != process.ID.String() {
			return fail("Interactive tool did not successfully target its owned process")
		}
		if name == "write_command" && argumentString(detail, "input") != "SAILRY-INPUT\n" {
			return fail("Interactive tool did not send the exact fixture input")
		}
	}
	output, err := run.processOutput(process.ID, nil)
	if err != nil {
		return err
	}
	if output.Info.ID != process.ID || output.Info.Session != run.session.ID || output.Info.Turn != turn.ID || !strings.Contains(output.Stdout.Text, "SAILRY-INPUT") || output.Info.Status.Kind == "running" || output.Info.Status.Kind == "stopping" {
		return fail("Input was not echoed or the owned process was left running")
	}
	run.evidence("process", "Exact input echo retained and owned process stopped")
	return nil
}

func commandInputApprovalMatches(name string, detail protocol.ToolDetail, process protocol.RequestID) bool {
	if process == (protocol.RequestID{}) || argumentString(detail, "id") != process.String() {
		return false
	}
	return name == "stop_command" || name == "write_command" && argumentString(detail, "input") == "SAILRY-INPUT\n"
}

func (run *caseRun) inputProcess(view turnView) (protocol.ProcessInfo, error) {
	starts := 0
	for _, call := range view.tools() {
		if call.name == "run_command" {
			starts++
		}
	}
	if starts != 1 {
		return protocol.ProcessInfo{}, blocked("Interactive fixture did not start exactly one process")
	}
	call, detail, err := run.requireTool(view, "run_command")
	if err != nil {
		return protocol.ProcessInfo{}, err
	}
	if call.summary.Failure != nil || detail.Result == nil || argumentString(detail, "command") != "cat" || argumentString(detail, "access") != "workspace" || string(arguments(detail)["background"]) != "true" {
		return protocol.ProcessInfo{}, blocked("Interactive fixture did not start its exact restricted background command")
	}
	var raw struct {
		Kind string               `json:"kind"`
		Data protocol.ProcessInfo `json:"data"`
	}
	if json.Unmarshal(*detail.Result, &raw) != nil || raw.Kind != "command_started" || raw.Data.ID == (protocol.RequestID{}) || raw.Data.Session != run.session.ID || raw.Data.Turn != view.run.Turn || raw.Data.Command != "cat" || raw.Data.Access != protocol.ProcessAccessWorkspace {
		return protocol.ProcessInfo{}, blocked("Interactive fixture returned no exact owned background process")
	}
	return raw.Data, nil
}

func caseLargeCommand(run *caseRun) error {
	if err := run.commandFixture(); err != nil {
		return err
	}
	view, err := run.ask("Call run_command exactly once with command=\"seq 1 20000\", access=\"workspace\", background=false, timeout_ms=5000. Do not quote its output in the answer. Do not start another process.")
	if err != nil {
		return err
	}
	if calls := view.tools(); len(calls) != 1 || calls[0].name != "run_command" {
		return fail("Large output fixture did not execute exactly one command")
	}
	call, detail, err := run.requireTool(view, "run_command")
	if err != nil {
		return err
	}
	if err := run.assertCommandCompletion(call, detail, "seq 1 20000", "1\n2\n", 0); err != nil {
		return err
	}
	id, err := retainedCommandID(detail)
	if err != nil {
		return err
	}
	// Foreground completions retain output by the ID in their original result;
	// list_commands intentionally lists only background processes.
	output, err := run.processOutput(id, nil)
	if err != nil {
		return err
	}
	if output.Info.ID != id || output.Info.Session != run.session.ID || output.Info.Turn != view.run.Turn || output.Info.Command != "seq 1 20000" || output.Info.Status.Kind != "finished" || !output.Stdout.Truncated || len(output.Stdout.Text) > 64*1024 {
		return fail("Large command preview did not retain its owned completion and bounded truncation")
	}
	var expected strings.Builder
	for line := 1; line <= 20000; line++ {
		fmt.Fprintf(&expected, "%d\n", line)
	}
	text := expected.String()
	for offset := uint64(0); ; {
		pageOutput, err := run.command(run.ctx, "read_command_page", protocol.CommandReadCommandPagePayload{Session: run.session.ID, ID: id, Stream: protocol.ProcessOutputOutputStreamStdout, Offset: offset, Limit: 32768})
		if err != nil {
			return err
		}
		page, err := outputAs[protocol.ProcessOutputOutputPage](pageOutput, "command_output_page")
		if err != nil {
			return err
		}
		end := offset + uint64(len(page.Text))
		if page.Info.ID != id || page.Info.Session != run.session.ID || page.Offset != offset || page.TotalBytes != uint64(len(text)) || len(page.Text) == 0 || len(page.Text) > 32768 || end > uint64(len(text)) || page.Text != text[offset:end] {
			return fail("Retained output page differs from the exact bounded command fixture")
		}
		if page.NextOffset == nil {
			if end != uint64(len(text)) {
				return fail("Retained output pagination ended before the fixture completed")
			}
			break
		}
		if *page.NextOffset != end || end >= uint64(len(text)) {
			return fail("Retained output pagination did not advance within the fixture")
		}
		offset = end
	}
	run.evidence("output", "Truncated 64 KiB preview and every bounded retained-output page match all 20000 fixture lines")
	return nil
}

func retainedCommandID(detail protocol.ToolDetail) (protocol.RequestID, error) {
	var raw struct {
		Kind    string `json:"kind"`
		Command struct {
			ID protocol.RequestID `json:"id"`
		} `json:"sailry_command"`
	}
	if detail.Result == nil || json.Unmarshal(*detail.Result, &raw) != nil || raw.Kind != "command_result" || raw.Command.ID == (protocol.RequestID{}) {
		return protocol.RequestID{}, fail("Foreground command result has no retained command identity")
	}
	return raw.Command.ID, nil
}
