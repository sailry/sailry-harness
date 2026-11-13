package acceptance

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"runtime/debug"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown/parser"
	core "github.com/sailry/sailry-harness/internal/acceptance"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/live"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// RunReplay reads acceptance recordings without opening a Client or a Node.
// Each recorded snapshot goes through the production projection and shell in
// its original order; the Tester's clock advances without wall-clock sleeps.
func RunReplay(ctx context.Context, directory string, emit func(core.CaseResult)) core.Report {
	report := core.Report{Version: 1, Suite: "replay", Started: time.Now().UTC()}
	add := func(result core.CaseResult) {
		report.Cases = append(report.Cases, result)
		if emit != nil {
			emit(result)
		}
	}
	entries, err := os.ReadDir(filepath.Join(directory, "recordings"))
	if err != nil {
		add(core.CaseResult{ID: "replay.recordings", Group: "replay", Title: "Read acceptance recordings", Status: "blocked", Detail: "The acceptance recordings directory could not be read"})
	} else {
		for _, entry := range entries {
			if entry.IsDir() || !strings.HasSuffix(entry.Name(), ".json") {
				continue
			}
			id, parseErr := protocol.ParseSessionID(strings.TrimSuffix(entry.Name(), ".json"))
			if parseErr != nil {
				add(core.CaseResult{ID: "replay.invalid-name", Group: "replay", Title: "Read acceptance recording", Status: "failed", Detail: "A recording filename does not identify a session"})
				continue
			}
			result := core.CaseResult{ID: "replay/" + id.String(), Group: "replay", Session: id.String(), Title: "Replay recorded conversation", Status: "failed"}
			if ctx.Err() != nil {
				result.Status, result.Detail = "canceled", "Replay canceled before this recording started"
			} else if recording, readErr := core.ReadRecording(directory, id); readErr != nil {
				result.Detail = "The acceptance recording is invalid or unreadable"
			} else {
				result = replayRecording(ctx, recording, result)
			}
			add(result)
		}
		if len(report.Cases) == 0 {
			add(core.CaseResult{ID: "replay.recordings", Group: "replay", Title: "Read acceptance recordings", Status: "blocked", Detail: "No recorded acceptance conversations were found"})
		}
	}
	report.Finished = time.Now().UTC()
	return report
}

func replayRecording(ctx context.Context, recording core.RecordingSession, result core.CaseResult) (out core.CaseResult) {
	started := time.Now()
	out = result
	w := &window{ctx: ctx, result: &out}
	defer func() {
		if recovered := recover(); recovered != nil {
			out.Status, out.Detail = "failed", "Recorded UI replay panicked"
			w.evidence("stack", string(debug.Stack()))
		}
		out.Duration = time.Since(started)
	}()
	fail := func(err error) core.CaseResult {
		out.Detail = err.Error()
		var issue *liveIssue
		if errors.As(err, &issue) {
			out.Status = issue.status
		}
		if errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
			out.Status = "canceled"
		}
		return out
	}
	if err := validateRecording(recording); err != nil {
		return fail(err)
	}
	before, err := json.Marshal(recording)
	if err != nil {
		return fail(errors.New("The recording could not be decoded without losing data"))
	}
	w.evidence("scope", "Read-only recorded data; production live projection and headless shell; no Client, Node, model or tool execution")
	scope := "recorded-snapshots"
	if len(recording.Frames) == 0 {
		scope = "terminal-only"
	} else if recording.Truncated || recording.DroppedFrames > 0 {
		scope = "truncated-snapshots"
	}
	w.evidence("replay_scope", scope)
	w.evidence("recording", fmt.Sprintf("%d recorded snapshots; %d dropped snapshots; capture truncated=%t", len(recording.Frames), recording.DroppedFrames, recording.Truncated))
	cv := &viewmodel.Conversation{ID: recording.Session.String(), Node: recording.Node.String(), Title: "Recorded conversation"}
	w.app = shell.NewLive(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{cv}, Workspace: &viewmodel.Workspace{}})
	w.app.Effects, w.app.NavOpen = false, false
	w.app.Sidebar.Selected = cv.ID
	// The default live shell is read-only without action adapters. Links are
	// also retained locally if a recorded result happens to contain one.
	w.app.Messages.OpenLink = func(*ui.Context, string, string) bool { return true }
	w.tester = ui.NewTesterWith(w.app.View, 1280, 1000, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
	w.tester.SetLocale("en")
	var at time.Duration
	probe := &recordedReplyProbe{}
	rendered := 0
	for index, frame := range recording.Frames {
		if err := ctx.Err(); err != nil {
			return fail(err)
		}
		cv.Turns, cv.Session, cv.Waiting, cv.Queue = live.Conversation(client.ConversationState{Snapshot: frame.Snapshot, Connected: true}, "en")
		if err := verifyRecordedText(frame.Snapshot, cv.Turns); err != nil {
			return fail(fmt.Errorf("Snapshot %d: %w", index+1, err))
		}
		w.app.Messages.List(cv.ID).ScrollToEnd()
		w.tester.Advance(frame.At - at)
		w.tester.Frame()
		if checked, err := probe.check(w, cv.Turns, frame.At-at); err != nil {
			return fail(fmt.Errorf("Snapshot %d: %w", index+1, err))
		} else if checked {
			rendered++
		}
		at = frame.At
	}
	// Terminal history is independently recorded, rather than invented to
	// fill a missing frame. Focused checks below inspect each of its turns.
	terminal := protocol.DefaultConversationSnapshot()
	terminal.Node, terminal.Sequence, terminal.Page, terminal.Missing = recording.Node, recording.History.Sequence, recording.History.Page, recording.History.Missing
	turns, session, _, _ := live.Conversation(client.ConversationState{Snapshot: terminal, Connected: true}, "en")
	if err := verifyRecordedText(terminal, turns); err != nil {
		return fail(err)
	}
	if len(turns) == 0 {
		out.Status, out.Detail = "skipped", "No conversation turns were recorded; there is no output to replay"
		return out
	}
	loads := map[string]bool{}
	if err := bindRecordedDetails(turns, terminal, recording.Tools, loads); err != nil {
		return fail(err)
	}
	for index := range turns {
		if err := ctx.Err(); err != nil {
			return fail(err)
		}
		turn := &turns[index]
		out.Turns = append(out.Turns, turn.ID)
		// A focused turn retains the complete recorded turn and production
		// controls while making every result inspectable in a long history.
		cv.Turns, cv.Session, cv.Queue = []conversation.Turn{*turn}, session, nil
		w.app.Messages.List(cv.ID).ScrollToEnd()
		w.tester.Frame()
		if checked, err := probe.check(w, cv.Turns, 0); err != nil {
			return fail(err)
		} else if checked {
			rendered++
		}
		if !turn.Reply.Status.Active() {
			if text := conversation.AnswerText(conversation.Compact(turn.Reply.Blocks)); text != "" {
				if err := w.button(label("turn_copy")); err != nil {
					return fail(errors.New("The recorded answer has no reachable Copy action"))
				}
				if w.tester.Clipboard() != text {
					return fail(errors.New("Copy changed the recorded answer"))
				}
			}
		}
		if err := inspectRecordedTools(w, &cv.Turns[0], loads); err != nil {
			return fail(err)
		}
	}
	after, err := json.Marshal(recording)
	if err != nil || !bytes.Equal(before, after) {
		return fail(errors.New("Replay modified the source recording"))
	}
	w.evidence("assertion", fmt.Sprintf("Replayed %d snapshots in order and inspected %d recorded turns; answer and tool Copy actions preserve their text", len(recording.Frames), len(turns)))
	w.evidence("rendered_text", fmt.Sprintf("Verified text by turn and role; checked %d plain reply tails in a focused production transcript", rendered))
	w.evidence("thinking", "Recorded thinking stays in the source data; the production UI intentionally has no thinking row")
	out.Status = "passed"
	switch scope {
	case "terminal-only":
		out.Detail = "Terminal history replay passed; no streaming snapshots were recorded"
	case "truncated-snapshots":
		out.Detail = "Available snapshots replayed; the truncated capture does not cover the complete stream"
	default:
		out.Detail = "Recorded snapshots and terminal history replay passed"
	}
	return out
}

func validateRecording(recording core.RecordingSession) error {
	if recording.Version != 1 || recording.Node == (protocol.NodeID{}) || recording.Session == (protocol.SessionID{}) || recording.History.Page.Session != recording.Session {
		return errors.New("The recording version or conversation identity is invalid")
	}
	var at time.Duration
	var sequence uint64
	for index, frame := range recording.Frames {
		if frame.At < 0 || index > 0 && (frame.At < at || frame.Snapshot.Sequence < sequence) {
			return errors.New("Recorded snapshots are out of order")
		}
		if frame.Snapshot.Node != recording.Node || frame.Snapshot.Page.Session != recording.Session {
			return errors.New("A recorded snapshot belongs to another Node or conversation")
		}
		if err := validateRecordedPage(frame.Snapshot.Page); err != nil {
			return err
		}
		at, sequence = frame.At, frame.Snapshot.Sequence
	}
	if err := validateRecordedPage(recording.History.Page); err != nil {
		return err
	}
	for _, tool := range recording.Tools {
		if tool.Reference.Session != recording.Session {
			return errors.New("A recorded tool belongs to another conversation")
		}
	}
	return nil
}

func validateRecordedPage(page protocol.ConversationPage) error {
	runs := map[protocol.TurnID]bool{}
	for _, run := range page.Runs {
		if run.Session != page.Session || run.Turn == (protocol.TurnID{}) || runs[run.Turn] {
			return errors.New("Recorded runs have invalid conversation or turn identities")
		}
		runs[run.Turn] = true
	}
	seen := map[string]bool{}
	var sequence uint64
	for index, entry := range page.Entries {
		if entry.ID == "" || seen[entry.ID] || index > 0 && entry.Sequence <= sequence {
			return errors.New("Recorded entries have invalid identities or ordering")
		}
		seen[entry.ID], sequence = true, entry.Sequence
	}
	return nil
}

func verifyRecordedText(snapshot protocol.ConversationSnapshot, turns []conversation.Turn) error {
	type text struct {
		input     string
		users     []string
		canonical bool
		assistant strings.Builder
	}
	runs := map[protocol.TurnID]protocol.ConversationRun{}
	for _, run := range snapshot.Page.Runs {
		runs[run.Turn] = run
	}
	owner := func(turn protocol.TurnID) protocol.TurnID {
		if run := runs[turn]; run.Joined != nil {
			if _, listed := runs[*run.Joined]; listed {
				return *run.Joined
			}
		}
		return turn
	}
	expected := map[protocol.TurnID]*text{}
	for _, run := range snapshot.Page.Runs {
		if owner(run.Turn) != run.Turn || run.Status == protocol.ConversationStatusQueued && slices.ContainsFunc(snapshot.Page.Queue.Items, func(item protocol.ConversationPending) bool { return item.Turn == run.Turn }) {
			continue
		}
		if run.Status == protocol.ConversationStatusContinued && run.Continuation != nil && !slices.ContainsFunc(snapshot.Page.Entries, func(entry protocol.ConversationEntry) bool { return entry.Turn == run.Turn && entry.Author != "user" }) {
			continue
		}
		value := &text{}
		if run.Continuation == nil && run.Input != nil {
			value.input = run.Input.Message.Text
		}
		expected[run.Turn] = value
	}
	read := func(parts []protocol.ConversationPart) (string, error) {
		var value strings.Builder
		for _, part := range parts {
			if part.Kind != "text" {
				continue
			}
			partText, err := copyLiveValue[string](part.Data)
			if err != nil {
				return "", errors.New("The recording has invalid text")
			}
			value.WriteString(partText)
		}
		return value.String(), nil
	}
	for _, entry := range snapshot.Page.Entries {
		if _, known := runs[entry.Turn]; !known {
			return errors.New("Recorded text belongs to an unknown turn")
		}
		id := owner(entry.Turn)
		value := expected[id]
		if value == nil {
			continue // Queued and empty continued runs have no transcript row.
		}
		partText, err := read(entry.Parts)
		if err != nil {
			return err
		}
		if entry.Author == "user" {
			if runs[id].Continuation != nil && entry.Turn == id {
				continue // A package continuation's own prompt is not UI copy.
			}
			value.canonical = true
			if partText != "" {
				value.users = append(value.users, partText)
			}
		} else {
			value.assistant.WriteString(partText)
		}
	}
	for _, draft := range snapshot.Drafts {
		if _, known := runs[draft.Turn]; !known {
			return errors.New("Recorded draft text belongs to an unknown turn")
		}
		partText, err := read(draft.Parts)
		if err != nil {
			return err
		}
		if value := expected[owner(draft.Turn)]; value != nil {
			value.assistant.WriteString(partText)
		}
	}
	for _, turn := range turns {
		id, err := protocol.ParseTurnID(turn.ID)
		value := expected[id]
		if err != nil || value == nil {
			return errors.New("The production projection added or repeated a recorded turn")
		}
		user := value.input
		if value.canonical {
			user = strings.Join(value.users, "\n\n")
		}
		if turn.User != user {
			return errors.New("The production projection changed recorded user text or its turn")
		}
		if answerText(turn.Reply) != value.assistant.String() {
			return errors.New("The production projection changed recorded assistant text or its turn")
		}
		delete(expected, id)
	}
	if len(expected) != 0 {
		return errors.New("The production projection lost a recorded turn")
	}
	return nil
}

// A focused transcript checks rendered reply text without asking historical or
// folded content to be onscreen. Its source block is unchanged, but other turns
// and the user's prompt cannot satisfy the assistant's visibility assertion.
type recordedReplyProbe struct {
	w  *window
	cv *viewmodel.Conversation
}

func (probe *recordedReplyProbe) check(parent *window, turns []conversation.Turn, elapsed time.Duration) (bool, error) {
	if len(turns) == 0 {
		return false, nil
	}
	turn := turns[len(turns)-1]
	blocks := turn.Reply.Blocks
	if turn.Compaction != nil || len(blocks) == 0 || blocks[len(blocks)-1].Kind != conversation.TextBlock {
		return false, nil
	}
	block := blocks[len(blocks)-1]
	document := parser.Parse(block.Text)
	last := 0
	for index := range document.Children(0) {
		last = index
	}
	if last == 0 || document.Block(last).Kind != parser.Paragraph {
		return false, nil
	}
	inlines := document.Inlines(last)
	for _, inline := range inlines {
		if inline.Kind != parser.Text && inline.Kind != parser.SoftBreak {
			return false, nil // Links, math and structured blocks have separate UI checks.
		}
	}
	sample := parser.PlainText(inlines)
	if strings.TrimSpace(sample) == "" {
		return false, nil
	}
	if probe.w == nil {
		probe.cv = &viewmodel.Conversation{ID: parent.result.Session, Title: "Recorded reply"}
		probe.w = &window{ctx: parent.ctx, result: parent.result, app: shell.NewLive(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{probe.cv}, Workspace: &viewmodel.Workspace{}})}
		probe.w.app.Effects, probe.w.app.NavOpen = false, false
		probe.w.app.Sidebar.Selected = probe.cv.ID
		probe.w.tester = ui.NewTesterWith(probe.w.app.View, 1280, 1000, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
		probe.w.tester.SetLocale("en")
	}
	probe.cv.Turns = []conversation.Turn{{ID: turn.ID, Reply: conversation.Reply{Status: turn.Reply.Status, Blocks: []conversation.Block{block}}}}
	probe.w.app.Messages.List(probe.cv.ID).ScrollToEnd()
	probe.w.tester.Advance(elapsed)
	probe.w.tester.Frame()
	if !recordedTextVisible(probe.w.tester, sample) {
		return false, errors.New("The recorded assistant reply has no visible rendered text")
	}
	return true, nil
}

func recordedTextVisible(tester *ui.Tester, sample string) bool {
	log, ok := tester.AccessByRole(ui.RoleLog, "")
	if !ok {
		return false
	}
	for _, info := range tester.AccessAll() {
		box, viewport := info.Bounds, log.Bounds
		if info.Name == sample && !info.Offscreen && box.W > 0 && box.H > 0 &&
			box.X < viewport.X+viewport.W && box.X+box.W > viewport.X && box.Y < viewport.Y+viewport.H && box.Y+box.H > viewport.Y {
			return true
		}
	}
	return false
}

type recordedCall struct {
	key     string
	name    string
	summary *protocol.ToolDetailSummary
	ref     protocol.ToolDetailReference
}

func bindRecordedDetails(turns []conversation.Turn, snapshot protocol.ConversationSnapshot, tools []core.RecordingTool, loaded map[string]bool) error {
	sources := map[string]recordedCall{}
	pending := map[string][]string{}
	for _, entry := range snapshot.Page.Entries {
		for index, part := range entry.Parts {
			key := entry.ID + "/" + strconv.Itoa(index)
			var id *string
			var name string
			var summary *protocol.ToolDetailSummary
			switch part.Kind {
			case "tool_call":
				value, err := copyLiveValue[protocol.ConversationPartToolCallPayload](part.Data)
				if err != nil {
					return err
				}
				id, name, summary = value.ID, value.Name, value.Summary
				if id != nil {
					identity := entry.Turn.String() + "\x00" + entry.Branch + "\x00" + *id
					pending[identity] = append(pending[identity], key)
				}
			case "tool_result":
				value, err := copyLiveValue[protocol.ConversationPartToolResultPayload](part.Data)
				if err != nil {
					return err
				}
				id, name, summary = value.ID, value.Name, value.Summary
				if id != nil {
					identity := entry.Turn.String() + "\x00" + entry.Branch + "\x00" + *id
					if keys := pending[identity]; len(keys) == 1 && sources[keys[0]].name == name {
						key = keys[0]
					}
					delete(pending, identity)
				}
			default:
				continue
			}
			if summary != nil {
				sources[key] = recordedCall{key: key, name: name, summary: summary, ref: protocol.ToolDetailReference{
					Session: snapshot.Page.Session, Turn: entry.Turn, Entry: entry.ID, Index: uint64(index), Call: id, Revision: summary.Revision,
				}}
			} else if part.Kind == "tool_call" {
				sources[key] = recordedCall{key: key, name: name}
			}
		}
	}
	for ti := range turns {
		for bi := range turns[ti].Reply.Blocks {
			for ci := range turns[ti].Reply.Blocks[bi].Calls {
				call := &turns[ti].Reply.Blocks[bi].Calls[ci]
				source, ok := sources[call.ID]
				if !ok || source.summary == nil || !source.summary.HasDetails {
					continue
				}
				index := slices.IndexFunc(tools, func(tool core.RecordingTool) bool { return reflect.DeepEqual(tool.Reference, source.ref) })
				if index < 0 || tools[index].Detail == nil {
					return unavailable("A recorded expandable tool has no retained detail; its UI check is incomplete")
				}
				full := *call
				live.ProjectToolDetail(&full, source.summary, *tools[index].Detail, "en")
				call.Output = &conversation.Output{}
				call.Load = func() {
					loaded[source.key] = true
					*call = full
				}
			}
		}
	}
	return nil
}

func inspectRecordedTools(w *window, turn *conversation.Turn, loaded map[string]bool) error {
	labels := []string{}
	var expected []conversation.Call
	for _, block := range turn.Reply.Blocks {
		for _, call := range block.Calls {
			name := call.Label
			if name == "" && call.LabelKey != "" {
				name = label(call.LabelKey)
			}
			if name != "" {
				labels = append(labels, name)
			}
			if call.Load != nil {
				expected = append(expected, call)
			}
		}
	}
	if len(expected) == 0 {
		return nil
	}
	// Open only recorded work and tool disclosures. Every clicked control
	// is inside this focused turn; replay supplies no execution adapter.
	w.app.Messages.List(w.result.Session).ScrollTo(0, ui.Start)
	w.tester.Frame()
	clicked := map[string]bool{}
	for {
		if err := w.ctx.Err(); err != nil {
			return err
		}
		opened := false
		for _, control := range w.tester.AccessAll() {
			if control.Role != ui.RoleButton || control.Expanded || control.Offscreen || control.Bounds.X < 100 {
				continue
			}
			work := slices.Contains([]string{label("turn_completed"), label("turn_failed"), label("turn_cancelled"), label("chat_interrupted"), label("chat_running")}, control.Name)
			tool := slices.ContainsFunc(labels, func(name string) bool { return control.Name == name || strings.HasPrefix(control.Name, name+" ") })
			identity := fmt.Sprintf("%s/%v", control.Name, control.Bounds)
			if (work || tool) && !clicked[identity] {
				clicked[identity] = true
				w.tester.ClickAt(control.Bounds.X+4, control.Bounds.Y+control.Bounds.H/2)
				w.tester.Frame()
				opened = true
				break
			}
		}
		if !opened {
			if w.app.Messages.List(w.result.Session).AtEnd() {
				break
			}
			w.tester.Scroll(1260, 500, 0, 500)
			w.tester.Frame()
		}
	}
	for _, call := range expected {
		if !loaded[call.ID] {
			return errors.New("A recorded tool result could not be expanded in the transcript")
		}
	}
	for _, block := range turn.Reply.Blocks {
		for _, call := range block.Calls {
			if !loaded[call.ID] {
				continue
			}
			if err := inspectRecordedCall(w, turn, call); err != nil {
				return err
			}
		}
	}
	return nil
}

// inspectRecordedCall follows the full-turn disclosure check with an isolated
// instance of the same production call. A long or identically named result in
// another turn must not supply this call's text bounds or Copy button.
func inspectRecordedCall(parent *window, turn *conversation.Turn, call conversation.Call) error {
	if call.Output == nil {
		return errors.New("The expanded recorded tool has no output projection")
	}
	w := &window{ctx: parent.ctx, result: parent.result}
	cv := &viewmodel.Conversation{ID: parent.result.Session, Title: "Recorded tool", Turns: []conversation.Turn{{
		ID: turn.ID, Reply: conversation.Reply{Status: turn.Reply.Status, Blocks: []conversation.Block{{Kind: conversation.ToolsBlock, Calls: []conversation.Call{call}}}},
	}}}
	w.app = shell.NewLive(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{cv}, Workspace: &viewmodel.Workspace{}})
	w.app.Effects, w.app.NavOpen = false, false
	w.app.Sidebar.Selected = cv.ID
	w.app.Messages.OpenLink = func(*ui.Context, string, string) bool { return true }
	w.tester = ui.NewTesterWith(w.app.View, 1280, 1000, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
	w.tester.SetLocale("en")
	// Work and the single call (or its path group) are the only disclosure
	// buttons in this transcript. Both retain the production labels and state.
	name := call.Label
	if name == "" && call.LabelKey != "" {
		name = label(call.LabelKey)
	}
	for {
		opened := false
		for _, control := range w.tester.AccessAll() {
			if control.Role != ui.RoleButton || control.Expanded || control.Offscreen || control.Bounds.X < 100 {
				continue
			}
			work := slices.Contains([]string{label("turn_completed"), label("turn_failed"), label("turn_cancelled"), label("chat_interrupted"), label("chat_running")}, control.Name)
			tool := name != "" && (control.Name == name || strings.HasPrefix(control.Name, name+" "))
			if !work && !tool {
				continue
			}
			w.tester.ClickAt(control.Bounds.X+4, control.Bounds.Y+control.Bounds.H/2)
			w.tester.Frame()
			opened = true
			break
		}
		if !opened {
			break
		}
	}
	output := call.Output
	// The production path-group renderer replaces the aggregate preview with
	// its individual paths, and offers no raw-text Copy action for that view.
	// Verify the actual path rows rather than expecting hidden preview text.
	if len(output.Paths) > 1 || len(output.Paths) == 1 && output.Paths[0] != call.Detail {
		for _, path := range output.Paths {
			area, ok := w.tester.Find(path)
			if !ok || area.W <= 0 || area.H <= 0 || area.Y < shell.HeaderHeight || area.Y+area.H > 1000 {
				return errors.New("A recorded tool path is not visible in its expanded group")
			}
		}
		if _, shown := w.tester.AccessByRole(ui.RoleButton, label("tool_copy")); shown {
			return errors.New("The recorded path group unexpectedly offers a raw-result Copy action")
		}
		w.evidence("tool_ui", fmt.Sprintf("Expanded recorded path group; verified %d original paths", len(output.Paths)))
		return nil
	}
	if output.Text == "" {
		return nil
	}
	display := output.Text
	if output.Command {
		lines := strings.Split(strings.TrimRight(display, "\n"), "\n")
		display = strings.Join(lines[max(0, len(lines)-100):], "\n")
	}
	if _, ok := w.tester.Find(display); !ok {
		return errors.New("The expanded recorded tool result has no rendered text")
	}
	return copyRecordedOutput(w, output)
}

func copyRecordedOutput(w *window, output *conversation.Output) error {
	var buttons []ui.AccessInfo
	for _, button := range w.tester.AccessAll() {
		if button.Role == ui.RoleButton && button.Name == label("tool_copy") && !button.Offscreen {
			buttons = append(buttons, button)
		}
	}
	if len(buttons) != 1 {
		return errors.New("The recorded tool result has no unique reachable Copy action")
	}
	button := buttons[0]
	// Hover the button itself: text bounds can extend beyond the inner output
	// scroll viewport, while the output's own Copy action stays at its top.
	w.tester.Move(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
	w.tester.Frame()
	if err := w.tester.Click(label("tool_copy")); err != nil {
		return errors.New("The recorded tool result Copy action is covered or unreachable")
	}
	w.tester.Frame()
	var parts []string
	for _, text := range []string{output.Failure, output.Text} {
		if text != "" {
			parts = append(parts, text)
		}
	}
	if output.Diff != nil {
		for _, line := range output.Diff.Lines {
			parts = append(parts, string(output.Diff.Op(line))+line.Text)
		}
	}
	if w.tester.Clipboard() != strings.Join(parts, "\n") {
		return errors.New("Copy changed the recorded tool result")
	}
	w.evidence("tool_ui", fmt.Sprintf("Expanded recorded tool output; copied %d original bytes", len(w.tester.Clipboard())))
	return nil
}
