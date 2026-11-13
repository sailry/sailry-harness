package acceptance

import (
	"fmt"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

const idleSession = "sailry-go/theme"

func sendMessage(w *window) error {
	if err := w.open(nil); err != nil {
		return err
	}
	if err := w.selectConversation(idleSession); err != nil {
		return err
	}
	send, err := w.control(ui.RoleButton, label("send"))
	if err != nil {
		return err
	}
	if !send.Disabled {
		return fmt.Errorf("Send is enabled with an empty draft")
	}
	const text = "Acceptance: send this fixture message"
	if err := w.typeMessage(text); err != nil {
		return err
	}
	cv := w.app.Data.Find(idleSession)
	before := len(cv.Turns)
	if err := w.button(label("send")); err != nil {
		return err
	}
	if len(cv.Turns) != before+1 || cv.Turns[before].User != text {
		return fmt.Errorf("Send produced %d turns, want %d with message %q", len(cv.Turns), before+1, text)
	}
	if draft := w.app.Composer(cv.ID).Draft; draft != "" {
		return fmt.Errorf("Send retained draft %q", draft)
	}
	if _, err := w.control(ui.RoleButton, label("chat_stop")); err != nil {
		return err
	}
	w.result.Turns = []string{cv.Turns[before].ID}
	w.evidence("assertion", fmt.Sprintf("Send disabled for empty input; sent %q; turns %d -> %d; draft cleared; Stop shown", text, before, len(cv.Turns)))
	return nil
}

func streamReply(w *window) error {
	if err := w.open(nil); err != nil {
		return err
	}
	if err := w.selectConversation(idleSession); err != nil {
		return err
	}
	const text = "Acceptance: stream a fixture reply"
	if err := w.typeMessage(text); err != nil {
		return err
	}
	cv := w.app.Data.Find(idleSession)
	n := len(cv.Turns)
	w.tester.Key(0, ui.KeyEnter)
	w.tester.Frame()
	if len(cv.Turns) != n+1 || cv.Turns[n].User != text || !cv.Running() {
		return fmt.Errorf("Enter did not start the expected reply: turns %d, running %v", len(cv.Turns), cv.Running())
	}
	if err := w.advance(2 * time.Second); err != nil {
		return err
	}
	partial := answerText(cv.Turns[n].Reply)
	if partial == "" || !cv.Running() || !w.tester.HasText(partial) {
		return fmt.Errorf("the intermediate reply is not visible and running: %q, running %v", partial, cv.Running())
	}
	if err := w.advance(time.Second); err != nil {
		return err
	}
	grown := answerText(cv.Turns[n].Reply)
	if len(grown) <= len(partial) || !strings.HasPrefix(grown, partial) {
		return fmt.Errorf("stream did not append text: %q -> %q", partial, grown)
	}
	for step := 0; step < 200 && cv.Running(); step++ {
		if err := w.advance(preview.Tick); err != nil {
			return err
		}
	}
	last := cv.Turns[n]
	if last.Reply.Status != conversation.Completed || cv.Playing() || !w.tester.HasText(label("turn_completed")) {
		return fmt.Errorf("reply did not finish visibly: status %d, playing %v", last.Reply.Status, cv.Playing())
	}
	if !w.tester.HasText("The preview played this reply without a model.") {
		return fmt.Errorf("the completed fixture answer is not rendered")
	}
	w.result.Turns = []string{last.ID}
	w.evidence("assertion", fmt.Sprintf("Enter sent message; visible streamed answer grew %d -> %d -> %d bytes; reply completed", len(partial), len(grown), len(answerText(last.Reply))))
	return nil
}

func stopReply(w *window) error {
	if err := w.open(nil); err != nil {
		return err
	}
	if err := w.selectConversation(idleSession); err != nil {
		return err
	}
	if err := w.typeMessage("Acceptance: stop the fixture reply"); err != nil {
		return err
	}
	if err := w.button(label("send")); err != nil {
		return err
	}
	if err := w.advance(2 * time.Second); err != nil {
		return err
	}
	cv := w.app.Data.Find(idleSession)
	n := len(cv.Turns) - 1
	before := answerText(cv.Turns[n].Reply)
	if before == "" || !cv.Running() {
		return fmt.Errorf("Stop fixture has no active partial reply")
	}
	if err := w.button(label("chat_stop")); err != nil {
		return err
	}
	if cv.Turns[n].Reply.Status != conversation.Cancelled || cv.Running() {
		return fmt.Errorf("Stop left reply status %d, running %v", cv.Turns[n].Reply.Status, cv.Running())
	}
	if err := w.advance(8 * time.Second); err != nil {
		return err
	}
	if got := answerText(cv.Turns[n].Reply); got != before || cv.Playing() {
		return fmt.Errorf("stopped reply changed from %q to %q, playing %v", before, got, cv.Playing())
	}
	if _, err := w.control(ui.RoleButton, label("send")); err != nil {
		return err
	}
	w.result.Turns = []string{cv.Turns[n].ID}
	w.evidence("assertion", fmt.Sprintf("Stop canceled the reply; %d bytes remained unchanged after eight simulated seconds; Send returned", len(before)))
	return nil
}

func toolOutput(w *window) error {
	const raw = "{\n  \"data\": {\"elapsed_ms\": 84, \"outcome\": {\"type\": \"exited\", \"data\": 127}},\n  \"stderr\": \"/bin/sh: python: command not found\\n\"\n}"
	output := &conversation.Output{}
	loads := 0
	if err := w.open(func(data *preview.Data) {
		cv := data.Find(idleSession)
		cv.Turns = []conversation.Turn{{ID: "acceptance/tool", User: "Inspect a failed fixture command", Reply: conversation.Reply{
			Status: conversation.Running,
			Blocks: []conversation.Block{{Kind: conversation.ToolsBlock, Calls: []conversation.Call{{
				ID: "acceptance/command", Name: "command", Label: "Command", Warning: true, Error: label("tool_failed"), Output: output,
				Load: func() { loads++; output.Text = raw },
			}}}},
		}}}
		cv.Status = preview.Running
	}); err != nil {
		return err
	}
	if err := w.selectConversation(idleSession); err != nil {
		return err
	}
	if loads != 0 || w.tester.HasText(raw) {
		return fmt.Errorf("tool details loaded before expansion: loads %d", loads)
	}
	if err := w.click("Command"); err != nil {
		return err
	}
	w.tester.Frame()
	if loads != 1 || !w.tester.HasText(raw) {
		return fmt.Errorf("expansion did not load the original result: loads %d, visible %v", loads, w.tester.HasText(raw))
	}
	failures := 0
	for _, text := range w.tester.Texts() {
		if text == label("tool_failed") {
			failures++
		}
	}
	if failures != 1 {
		return fmt.Errorf("failure label rendered %d times, want only the tool header", failures)
	}
	if err := w.copyOutput(raw); err != nil {
		return err
	}
	if err := w.click("Command"); err != nil {
		return err
	}
	if w.tester.HasText(raw) || loads != 1 {
		return fmt.Errorf("closing the tool retained its body or reloaded it: visible %v, loads %d", w.tester.HasText(raw), loads)
	}
	w.result.Turns = []string{"acceptance/tool"}
	w.evidence("assertion", "Expanded lazy details once; original JSON rendered and copied unchanged; failure label appeared only in the header; closing hid the result")
	w.evidence("tool_result", raw)
	return nil
}

func approveTool(w *window) error {
	if err := w.open(nil); err != nil {
		return err
	}
	cv := w.app.Data.Find(preview.Big)
	w.result.Session = cv.ID
	if len(cv.Waiting.Approvals) != 2 {
		return fmt.Errorf("fixture has %d approvals, want 2", len(cv.Waiting.Approvals))
	}
	approved, remaining := cv.Waiting.Approvals[0].ID, cv.Waiting.Approvals[1].ID
	if err := w.button(label("approval_live_count", "count", 2)); err != nil {
		return err
	}
	if !w.tester.HasText("1/2") || !w.tester.HasText("internal/kit/loader.go") {
		return fmt.Errorf("approval popover did not show its first request")
	}
	if err := w.button(label("knowledge_next")); err != nil {
		return err
	}
	if !w.tester.HasText("2/2") || !w.tester.HasText("go vet ./...") {
		return fmt.Errorf("Next did not show the command approval")
	}
	if err := w.button(label("knowledge_previous")); err != nil {
		return err
	}
	if err := w.button(label("approval_allow_once")); err != nil {
		return err
	}
	if len(cv.Waiting.Approvals) != 1 || cv.Waiting.Approvals[0].ID != remaining {
		return fmt.Errorf("Allow once did not resolve only approval %q", approved)
	}
	if _, err := w.control(ui.RoleButton, label("approval_live_count", "count", 1)); err != nil {
		return err
	}
	w.evidence("assertion", fmt.Sprintf("Paged both requests; Allow once resolved %q; %q and the one-request capsule remain", approved, remaining))
	return nil
}

func answerQuestion(w *window) error {
	if err := w.open(nil); err != nil {
		return err
	}
	cv := w.app.Data.Find(preview.Big)
	w.result.Session = cv.ID
	if cv.Waiting.Question == nil {
		return fmt.Errorf("fixture has no pending question")
	}
	question := cv.Waiting.Question.ID
	if err := w.button(label("question_reply")); err != nil {
		return err
	}
	if err := w.advance(time.Second); err != nil {
		return err
	}
	if !w.tester.HasText("Keep the 2x3 loader or use Kit's spinner?") {
		return fmt.Errorf("question popover did not show its prompt")
	}
	if err := w.click("Use the spinner"); err != nil {
		return err
	}
	chosen, err := w.control(ui.RoleRadio, "Use the spinner")
	if err != nil || !chosen.Checked {
		return fmt.Errorf("the selected question answer is not checked")
	}
	if err := w.button(label("interaction_submit")); err != nil {
		return err
	}
	if cv.Waiting.Question != nil {
		return fmt.Errorf("Submit did not resolve question %q", question)
	}
	if _, open := w.tester.AccessByRole(ui.RoleButton, label("interaction_submit")); open {
		return fmt.Errorf("question submit remained after answering")
	}
	w.evidence("assertion", fmt.Sprintf("Opened %q, chose Use the spinner, and Submit removed the pending question and its popover", question))
	return nil
}

func answerText(reply conversation.Reply) string {
	var answer strings.Builder
	for _, block := range reply.Blocks {
		if block.Kind == conversation.TextBlock {
			answer.WriteString(block.Text)
		}
	}
	return answer.String()
}

func (w *window) copyOutput(text string) error {
	area, ok := w.tester.Find(text)
	if !ok || area.W <= 0 || area.H <= 0 {
		return liveFail("original tool result has no visible text bounds")
	}
	w.tester.Move(area.X+8, area.Y+8)
	w.tester.Frame()
	var copy ui.AccessInfo
	for _, info := range w.tester.AccessAll() {
		// The user bubble also has a Copy button. The output's is the
		// compact button within the result, above its text's trailing edge.
		if info.Role == ui.RoleButton && info.Name == label("tool_copy") && info.Bounds.W == 20 && info.Bounds.H == 20 &&
			info.Bounds.Y >= area.Y-8 && info.Bounds.Y < area.Y+area.H {
			copy = info
			break
		}
	}
	if copy.Name == "" {
		return liveFail("expanded tool output has no visible Copy button")
	}
	w.tester.ClickAt(copy.Bounds.X+copy.Bounds.W/2, copy.Bounds.Y+copy.Bounds.H/2)
	w.tester.Frame()
	if w.tester.Clipboard() != text {
		return liveFail("Copy changed the original tool result")
	}
	return nil
}

func fileReferences(w *window) error {
	if err := w.open(func(data *preview.Data) {
		cv := data.Find(idleSession)
		cv.Status = preview.Idle
		cv.Turns = []conversation.Turn{{ID: "acceptance/file-references", Reply: conversation.Reply{Status: conversation.Completed, Blocks: []conversation.Block{
			{Kind: conversation.TextBlock, Text: "Review [Inline source](export.py#L7).\n\n- [List source](stock.py)\n\n> [Quoted source](test_export.py)\n\n[Top-level source](export.py)"},
			{Kind: conversation.FilesBlock, Files: []conversation.File{{ID: "acceptance/publication", Path: "output/report.pdf", Mime: "application/pdf"}}},
		}}}}
	}); err != nil {
		return err
	}
	offered := 0
	w.app.Messages.Embed = func(_, target, _ string, _ bool) (conversation.Embed, bool) {
		offered++
		return conversation.Embed{File: &conversation.File{ID: "incorrect/reference", Path: target}}, true
	}
	var clicked string
	var external bool
	w.app.Messages.OpenLink = func(c *ui.Context, session, target string) bool {
		if session == idleSession {
			clicked = target
			external = conversation.External(c)
		}
		return true
	}
	if err := w.selectConversation(idleSession); err != nil {
		return err
	}
	for _, entry := range []struct{ name, target string }{
		{"Inline source", "export.py#L7"}, {"List source", "stock.py"},
		{"Quoted source", "test_export.py"}, {"Top-level source", "export.py"},
	} {
		for _, modifier := range []ui.Modifiers{0, ui.Cmd} {
			link, err := w.control(ui.RoleLink, entry.name)
			if err != nil {
				return err
			}
			// A selectable paragraph can have the same name as its only link.
			// Click the link's words, not the paragraph's surrounding whitespace.
			clicked, external = "", false
			w.tester.ClickAtWith(modifier, link.Bounds.X+link.Bounds.W/2, link.Bounds.Y+link.Bounds.H/2)
			w.tester.Frame()
			if clicked != entry.target || external != (modifier&ui.Cmd != 0) {
				return fmt.Errorf("file reference %q lost its navigation target or modifier", entry.name)
			}
		}
	}
	if offered != 0 {
		return fmt.Errorf("ordinary Markdown references were offered as publications %d times", offered)
	}
	cards := 0
	for _, info := range w.tester.AccessAll() {
		if info.Role == ui.RoleButton && info.Name == label("artifact_open") {
			cards++
		}
	}
	if cards != 1 || !w.tester.HasText("report.pdf") {
		return fmt.Errorf("explicit publication has %d cards, want exactly one", cards)
	}
	w.result.Turns = []string{"acceptance/file-references"}
	w.evidence("assertion", "Inline, list, quoted and top-level source references stayed clickable with their paths and line; only the explicitly published report retained a card")
	return nil
}
