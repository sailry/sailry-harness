package acceptance

import (
	"bytes"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	core "github.com/sailry/sailry-harness/internal/acceptance"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func liveFail(detail string) error { return &liveIssue{"failed", detail} }

func (w *liveWindow) visibleAnswer(text string) bool {
	// A plain-text reply can span several rendered paragraphs. Checking its
	// trailing text avoids confusing a marker in the user's prompt with output.
	runes := []rune(strings.TrimSpace(text))
	if len(runes) > 64 {
		runes = runes[len(runes)-64:]
	}
	return len(runes) > 0 && w.tester.HasText(string(runes))
}

func liveStream(w *liveWindow) error {
	const marker = "SAILRY-E2E-STREAM"
	if err := w.send("Do not call tools. Write a single plain-text paragraph of about 220 words describing a fictional red kite. Begin with SAILRY-E2E-STREAM and end with KITE-FINISHED. Do not use Markdown, lists or headings."); err != nil {
		return err
	}
	partial, grew := 0, false
	if err := w.wait(func() (bool, error) {
		turn := w.uiTurn()
		if turn == nil {
			return false, nil
		}
		text := answerText(turn.Reply)
		if turn.Reply.Status.Active() && w.visibleAnswer(text) {
			if partial == 0 {
				partial = len(text)
			} else if len(text) > partial {
				grew = true
			}
		}
		canonical, known := w.canonicalRun()
		if !known || activeLive(canonical.Status) || turn.Reply.Status.Active() {
			return false, nil
		}
		if canonical.Status != protocol.ConversationStatusCompleted || turn.Reply.Status != conversation.Completed {
			return false, liveFail("The UI-sent streaming reply did not complete successfully")
		}
		if !grew {
			return false, unavailable("The provider completed before two growing visible stream samples could be observed")
		}
		if !strings.Contains(text, marker) || !strings.Contains(text, "KITE-FINISHED") || !w.visibleAnswer(text) {
			return false, liveFail("The completed fixture reply is missing its markers or visible final text")
		}
		w.evidence("assertion", fmt.Sprintf("Typed and clicked Send; draft cleared; visible active text grew from %d to %d bytes; UI and Node both completed", partial, len(text)))
		return true, nil
	}); err != nil {
		return err
	}
	if err := w.capture(); err != nil {
		return err
	}
	return w.assertDurable(protocol.ConversationStatusCompleted)
}

const liveCommandFixture = "printf 'SAILRY-EXIT7\\n'; exit 7"

func liveTool(w *liveWindow) error {
	output, err := w.command(w.ctx, "inspect_host", nil)
	if err != nil {
		return err
	}
	host, err := liveOutput[protocol.HostInfo](output, "host_info")
	if err != nil {
		return err
	}
	if host.Os == nil || !strings.Contains(strings.ToLower(*host.Os), "darwin") && !strings.Contains(strings.ToLower(*host.Os), "macos") && !strings.Contains(strings.ToLower(*host.Os), "linux") {
		return &liveIssue{"skipped", "This command fixture requires a POSIX execution Node"}
	}
	if err := w.send(fmt.Sprintf("Call run_command exactly once with command=%q, access=\"workspace\", background=false, timeout_ms=5000. Use only this conversation's dedicated workspace and the default working directory. This exact command only prints a fixture marker and intentionally exits 7. Do not retry, repair, read files, install, access the network or run another tool. Report its actual exit code briefly.", liveCommandFixture)); err != nil {
		return err
	}
	if err := w.waitCompleted(); err != nil {
		return err
	}
	state, _ := w.controller.Conversation(w.session.ID)
	if state.Snapshot.Page.Session != w.session.ID {
		return liveFail("The tool fixture projection belongs to another session")
	}
	toolName, err := w.commandToolName()
	if err != nil {
		return err
	}
	reference, summary, err := liveToolReference(state.Snapshot.Page, w.turn, toolName)
	if err != nil {
		return err
	}
	if summary.Failure != nil && summary.Failure.Origin == protocol.ToolFailureOriginRuntime {
		return unavailable("The execution Node could not run the restricted command fixture")
	}
	if !summary.HasDetails {
		return liveFail("The completed command has no expandable result")
	}
	// Completion folds the work region. Open it with its real disclosure before
	// opening the tool, so the test covers both user interactions.
	if err := w.button(label("turn_completed")); err != nil {
		return err
	}
	var name string
	for _, block := range w.uiTurn().Reply.Blocks {
		for _, call := range block.Calls {
			if call.Name == toolName {
				name = call.Label
				if call.LabelKey != "" && name == "" {
					name = label(call.LabelKey)
				}
			}
		}
	}
	if name == "" {
		return liveFail("The canonical command call has no rendered tool row")
	}
	if _, loaded := w.controller.ConversationDetail(reference); loaded {
		return liveFail("Tool details were loaded before its disclosure was opened")
	}
	if err := w.click(name); err != nil {
		return err
	}
	var detail protocol.ToolDetail
	if err := w.wait(func() (bool, error) {
		var loaded bool
		detail, loaded = w.controller.ConversationDetail(reference)
		return loaded, nil
	}); err != nil {
		return err
	}
	expected, err := liveCommandResult(detail)
	if err != nil {
		return err
	}
	if err := w.wait(func() (bool, error) {
		for _, block := range w.uiTurn().Reply.Blocks {
			for _, call := range block.Calls {
				if call.Name == toolName && call.Output != nil && call.Output.Text == expected {
					if call.Output.Failure != "" {
						return false, liveFail("Expanded output added a synthetic failure message")
					}
					return w.tester.HasText(expected), nil
				}
			}
		}
		return false, nil
	}); err != nil {
		return err
	}
	if err := w.copyOutput(expected); err != nil {
		return err
	}
	w.evidence("assertion", "Clicked completed work and tool disclosures; lazy detail loaded; visible output and Copy equal the canonical result projection")
	w.evidence("tool", "Exactly one workspace command retained exit=7 and the fixture stdout marker")
	if err := w.capture(); err != nil {
		return err
	}
	return w.assertDurable(protocol.ConversationStatusCompleted)
}

func (w *liveWindow) commandToolName() (string, error) {
	output, err := w.command(w.ctx, "read_turn_capture", protocol.CommandReadTurnCapturePayload{Session: w.session.ID, Turn: w.turn})
	if err != nil {
		return "", err
	}
	capture, err := liveOutput[protocol.QueuedTurn](output, "queued_turn")
	if err != nil || capture.ID != w.turn || capture.Session != w.session.ID {
		return "", liveFail("The command tool capture belongs to another acceptance turn")
	}
	for _, reference := range capture.Plugins {
		if reference.Name != "commands" {
			continue
		}
		output, err := w.command(w.ctx, "read_plugin_version", protocol.CommandReadPluginVersionPayload{Package: reference})
		if err != nil {
			return "", err
		}
		info, err := liveOutput[protocol.PluginInfo](output, "plugin")
		if err != nil || info.Summary.Name != reference.Name || info.Summary.Digest != reference.Digest || info.Extension == nil {
			return "", liveFail("The command declaration differs from its captured package version")
		}
		for _, tool := range info.Extension.Tools {
			if tool.Name == "run_command" && tool.Server == nil && tool.Presentation == protocol.ToolPresentationContent {
				return core.PackageToolAlias(reference.Name, tool.Name), nil
			}
		}
	}
	return "", liveFail("The UI-sent turn did not capture the commands package's content tool")
}

func liveToolReference(page protocol.ConversationPage, turn protocol.TurnID, name string) (protocol.ToolDetailReference, *protocol.ToolDetailSummary, error) {
	var call protocol.ConversationPartToolCallPayload
	var result protocol.ConversationPartToolResultPayload
	var reference protocol.ToolDetailReference
	var callBranch, resultBranch string
	calls, results := 0, 0
	for _, entry := range page.Entries {
		if entry.Turn != turn {
			continue
		}
		for index, part := range entry.Parts {
			var err error
			switch part.Kind {
			case "tool_call":
				calls++
				call, err = copyLiveValue[protocol.ConversationPartToolCallPayload](part.Data)
				callBranch = entry.Branch
			case "tool_result":
				results++
				result, err = copyLiveValue[protocol.ConversationPartToolResultPayload](part.Data)
				resultBranch = entry.Branch
				reference = protocol.ToolDetailReference{Session: page.Session, Turn: turn, Entry: entry.ID, Index: uint64(index), Call: result.ID}
			}
			if err != nil {
				return reference, nil, liveFail("The command fixture has an invalid canonical tool part")
			}
		}
	}
	if calls != 1 || results != 1 || call.ID == nil || result.ID == nil || *call.ID != *result.ID || call.Name != name || result.Name != name || callBranch != resultBranch || result.Summary == nil || result.Summary.Revision == "" {
		return reference, nil, liveFail("Expected one declared command call paired with its own canonical result")
	}
	reference.Revision = result.Summary.Revision
	return reference, result.Summary, nil
}

func liveCommandResult(detail protocol.ToolDetail) (string, error) {
	var arguments struct {
		Command     string            `json:"command"`
		Access      string            `json:"access"`
		Background  bool              `json:"background"`
		Pty         bool              `json:"pty"`
		Timeout     uint64            `json:"timeout_ms"`
		Cwd         *string           `json:"cwd"`
		Attachments []json.RawMessage `json:"attachments"`
	}
	if json.Unmarshal(detail.Arguments, &arguments) != nil || arguments.Command != liveCommandFixture || arguments.Access != "workspace" || arguments.Background || arguments.Pty || arguments.Timeout != 5000 || arguments.Cwd != nil && *arguments.Cwd != "." || len(arguments.Attachments) != 0 {
		return "", liveFail("The model changed the exact restricted command fixture")
	}
	if detail.Result == nil || detail.Content != nil {
		return "", liveFail("The intentionally failed command did not retain its original result")
	}
	var raw struct {
		Kind string                     `json:"kind"`
		Data protocol.ProcessCompletion `json:"data"`
	}
	if json.Unmarshal(*detail.Result, &raw) != nil || raw.Kind != "command_result" {
		return "", liveFail("The canonical command result has no process completion")
	}
	code, err := copyLiveValue[int32](raw.Data.Outcome.Data)
	if err != nil || raw.Data.Outcome.Kind != "exited" || code != 7 || raw.Data.Stdout.Text != "SAILRY-EXIT7\n" || raw.Data.Stderr.Text != "" || raw.Data.Stdout.Truncated || raw.Data.Stdout.InvalidUtf8 {
		return "", liveFail("The canonical command result lost its exact exit code or stdout marker")
	}
	// Returned command errors preserve their original result; the optional
	// display formatter and its Resolved.Command are not the process receipt.
	var formatted bytes.Buffer
	if err := json.Indent(&formatted, *detail.Result, "", "  "); err != nil {
		return "", liveFail("The canonical command result is not valid JSON")
	}
	return formatted.String(), nil
}

func liveStop(w *liveWindow) error {
	if err := w.send("Do not call tools. Write one long plain-text paragraph of at least 1600 words describing an imaginary blue boat. Start immediately with BOAT-STREAM and keep writing the paragraph without headings or lists."); err != nil {
		return err
	}
	if err := w.wait(func() (bool, error) {
		turn := w.uiTurn()
		if turn == nil {
			return false, nil
		}
		if !turn.Reply.Status.Active() {
			return false, unavailable("The provider finished before a visible live reply could be stopped")
		}
		return w.visibleAnswer(answerText(turn.Reply)), nil
	}); err != nil {
		return err
	}
	if err := w.button(label("chat_stop")); err != nil {
		return err
	}
	if err := w.wait(func() (bool, error) {
		run, known := w.canonicalRun()
		turn := w.uiTurn()
		if !known || turn == nil || activeLive(run.Status) || turn.Reply.Status.Active() {
			return false, nil
		}
		if run.Status != protocol.ConversationStatusCancelled || turn.Reply.Status != conversation.Cancelled {
			return false, liveFail("Clicking Stop did not cancel both the Node turn and UI reply")
		}
		return true, nil
	}); err != nil {
		return err
	}
	stopped := answerText(w.uiTurn().Reply)
	until := time.Now().Add(300 * time.Millisecond)
	if err := w.wait(func() (bool, error) {
		if turn := w.uiTurn(); turn == nil || turn.Reply.Status.Active() || answerText(turn.Reply) != stopped {
			return false, liveFail("The canceled live reply continued to change")
		}
		return !time.Now().Before(until), nil
	}); err != nil {
		return err
	}
	if _, err := w.control(ui.RoleButton, label("send")); err != nil {
		return err
	}
	w.evidence("assertion", fmt.Sprintf("Clicked Stop on visible streamed text; Node and UI canceled; %d bytes remained stable across real event frames; Send returned", len(stopped)))
	if err := w.capture(); err != nil {
		return err
	}
	return w.assertDurable(protocol.ConversationStatusCancelled)
}

func (w *liveWindow) waitCompleted() error {
	return w.wait(func() (bool, error) {
		run, known := w.canonicalRun()
		turn := w.uiTurn()
		if !known || turn == nil || activeLive(run.Status) || turn.Reply.Status.Active() {
			return false, nil
		}
		if run.Status != protocol.ConversationStatusCompleted || turn.Reply.Status != conversation.Completed {
			return false, liveFail("The tool fixture did not complete successfully in both Node and UI")
		}
		return true, nil
	})
}

func (w *liveWindow) assertDurable(expected protocol.ConversationStatus) error {
	output, err := w.command(w.ctx, "read_conversation", protocol.CommandReadConversationPayload{Session: w.session.ID, Limit: 100})
	if err != nil {
		return err
	}
	history, err := liveOutput[protocol.ConversationHistory](output, "conversation")
	if err != nil {
		return err
	}
	for _, run := range history.Page.Runs {
		if run.Turn != w.turn {
			continue
		}
		if run.Status != expected {
			return liveFail("The durable turn status differs from the observed UI terminal state")
		}
		var canonical strings.Builder
		for _, entry := range history.Page.Entries {
			if entry.Turn != w.turn || entry.Author == "user" {
				continue
			}
			for _, part := range entry.Parts {
				if part.Kind == "text" {
					text, err := copyLiveValue[string](part.Data)
					if err != nil {
						return liveFail("The durable fixture answer has invalid text")
					}
					canonical.WriteString(text)
				}
			}
		}
		if turn := w.uiTurn(); turn == nil || answerText(turn.Reply) != canonical.String() {
			return liveFail("The final UI answer differs from the durable canonical answer")
		}
		w.evidence("canonical", "Durable turn status and assistant text match the final UI reply")
		return nil
	}
	return liveFail("The UI-sent turn was absent from durable history")
}
