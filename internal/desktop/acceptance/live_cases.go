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
	// Paragraphs are separate rendered elements. Keep the trailing sample
	// inside the final paragraph rather than searching across their boundary.
	paragraphs := strings.Split(strings.TrimSpace(text), "\n\n")
	runes := []rune(strings.TrimSpace(paragraphs[len(paragraphs)-1]))
	if len(runes) == 0 {
		return false
	}
	if len(runes) > 64 {
		runes = runes[len(runes)-64:]
		return w.tester.HasText(string(runes))
	}
	// A short marker may also occur inside the user's instructions. Require
	// its complete rendered element instead of a matching prompt substring.
	_, shown := w.tester.Find(string(runes))
	return shown
}

type liveStreamSamples struct {
	turn        protocol.TurnID
	first, last int
}

func (w *liveWindow) observeStreamFrame(c *ui.Context) {
	if w.stream == nil || w.input == "" || w.app.Sidebar.Selected != w.session.ID.String() {
		return
	}
	cv := w.app.Data.Find(w.session.ID.String())
	if cv == nil || len(cv.Turns) != 1 {
		return
	}
	turn := cv.Turns[0]
	id, err := protocol.ParseTurnID(turn.ID)
	if err != nil || id == (protocol.TurnID{}) || turn.User != w.input || !turn.Reply.Status.Active() ||
		w.turn != (protocol.TurnID{}) && id != w.turn {
		return
	}
	text := answerText(turn.Reply)
	paragraphs := strings.Split(strings.TrimSpace(text), "\n\n")
	sample := strings.TrimSpace(paragraphs[len(paragraphs)-1])
	if sample == "" || strings.Contains(w.input, sample) {
		return
	}
	// Shell's AfterFrame runs before layout. Tester.Frame may settle several
	// frames, so observe each final build pass after its children paint instead.
	// Capture this pass's projection, never a newer Controller draft; discarded
	// build passes have no paint callback. Atlas repaints must not count twice.
	stream, painted := w.stream, false
	c.Root().DrawOver(func(*ui.Painter, ui.Rect) {
		if painted || w.tester == nil {
			return
		}
		painted = true
		if !w.visibleStreamParagraph(sample) || stream.turn != (protocol.TurnID{}) && stream.turn != id {
			return
		}
		stream.turn = id
		if stream.first == 0 {
			stream.first = len(text)
		}
		stream.last = max(stream.last, len(text))
	})
}

func (w *liveWindow) visibleStreamParagraph(sample string) bool {
	log, ok := w.tester.AccessByRole(ui.RoleLog, "")
	if !ok || log.Offscreen || log.Bounds.W <= 0 || log.Bounds.H <= 0 {
		return false
	}
	viewport := log.Bounds
	if composer, present := w.tester.AccessByRole(ui.RoleTextField, label("composer")); present && composer.Bounds.Y > viewport.Y {
		viewport.H = min(viewport.H, composer.Bounds.Y-viewport.Y)
	}
	clipped, shown := w.tester.Find(sample)
	if !shown || clipped.W <= 0 || clipped.H <= 0 {
		return false
	}
	for _, info := range w.tester.AccessAll() {
		box := info.Bounds
		// Exact paragraph text excludes matching prompt substrings. Requiring
		// its whole box inside the transcript and its committed clip also keeps
		// growth below the viewport from counting as visible streamed output.
		if info.Role == ui.RoleText && info.Name == sample && !info.Offscreen && box.W > 0 && box.H > 0 &&
			box.X >= viewport.X && box.Y >= viewport.Y && box.X+box.W <= viewport.X+viewport.W && box.Y+box.H <= viewport.Y+viewport.H &&
			clipped.X <= box.X && clipped.Y <= box.Y && clipped.X+clipped.W >= box.X+box.W && clipped.Y+clipped.H >= box.Y+box.H {
			return true
		}
	}
	return false
}

func liveStream(w *liveWindow) error {
	const marker = "SAILRY-E2E-STREAM"
	samples := &liveStreamSamples{}
	w.stream = samples
	defer func() { w.stream = nil }()
	if err := w.send("Do not call tools. Write a single plain-text paragraph of about 220 words describing a fictional red kite. Begin with SAILRY-E2E-STREAM and end with KITE-FINISHED. Do not use Markdown, lists or headings."); err != nil {
		return err
	}
	if err := w.wait(func() (bool, error) {
		turn := w.uiTurn()
		if turn == nil {
			return false, nil
		}
		text := answerText(turn.Reply)
		canonical, known := w.canonicalRun()
		if !known || activeLive(canonical.Status) || turn.Reply.Status.Active() {
			return false, nil
		}
		if canonical.Status != protocol.ConversationStatusCompleted || turn.Reply.Status != conversation.Completed {
			return false, liveFail("The UI-sent streaming reply did not complete successfully")
		}
		if samples.turn != w.turn || samples.first == 0 || samples.last <= samples.first {
			return false, unavailable("The provider completed before two growing visible stream samples could be observed")
		}
		if !strings.Contains(text, marker) || !strings.Contains(text, "KITE-FINISHED") || !w.visibleAnswer(text) {
			return false, liveFail("The completed fixture reply is missing its markers or visible final text")
		}
		w.evidence("assertion", fmt.Sprintf("Typed and clicked Send; draft cleared; visible active text grew from %d to %d bytes; UI and Node both completed", samples.first, samples.last))
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
	if err := w.sendWithEnter("Do not call tools. Write one long plain-text paragraph of at least 1600 words describing an imaginary blue boat. Start immediately with BOAT-STREAM and keep writing the paragraph without headings or lists."); err != nil {
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
	w.evidence("assertion", fmt.Sprintf("Enter sent the fixture; clicked Stop on visible streamed text; Node and UI canceled; %d bytes remained stable across real event frames; Send returned", len(stopped)))
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
	if history.Page.NextBefore != nil || len(history.Missing) != 0 || len(history.Page.Queue.Items) != 0 {
		return liveFail("The fixture history is incomplete or still queued; a single admission cannot be confirmed")
	}
	if _, admitted, err := w.admittedTurn(history.Page); err != nil {
		return err
	} else if !admitted {
		return liveFail("The fixture history does not contain exactly one admitted task and user message")
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
		w.evidence("canonical", "Exactly one task and user message retain the fixture input, session and turn; durable status and assistant text match the final UI reply")
		return nil
	}
	return liveFail("The UI-sent turn was absent from durable history")
}
