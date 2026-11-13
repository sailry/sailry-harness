package acceptance

import (
	"fmt"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

func queueMessage(w *window) error {
	if err := w.open(nil); err != nil {
		return err
	}
	cv := w.app.Data.Find(preview.Big)
	w.result.Session = cv.ID
	before := len(cv.Queue)
	const original, replacement = "Acceptance: queued fixture message", "Acceptance: edited queued message"
	if !cv.Running() {
		return fmt.Errorf("queue fixture has no active reply")
	}
	if err := w.typeMessage(original); err != nil {
		return err
	}
	w.tester.Key(0, ui.KeyTab)
	w.tester.Frame()
	if len(cv.Queue) != before+1 || cv.Queue[before].Text != original || w.app.Composer(cv.ID).Draft != "" {
		return fmt.Errorf("Tab did not queue the draft: %d items, draft %q", len(cv.Queue), w.app.Composer(cv.ID).Draft)
	}
	if err := w.button(label("queue_messages_count", "count", len(cv.Queue))); err != nil {
		return err
	}
	if err := w.advance(300 * time.Millisecond); err != nil {
		return err
	}
	if !w.tester.HasText(original) {
		return fmt.Errorf("unfolded queue did not render the queued message")
	}
	var edit ui.AccessInfo
	for _, control := range w.tester.AccessAll() {
		if control.Role == ui.RoleButton && control.Name == label("queue_edit") && control.Bounds.Y > edit.Bounds.Y {
			edit = control
		}
	}
	if edit.Name == "" || edit.Bounds.W <= 0 || edit.Bounds.H <= 0 {
		return fmt.Errorf("queued message has no visible Edit button")
	}
	w.tester.ClickAt(edit.Bounds.X+edit.Bounds.W/2, edit.Bounds.Y+edit.Bounds.H/2)
	w.tester.Frame()
	area, err := w.control(ui.RoleTextField, label("queue_edit"))
	if err != nil {
		return err
	}
	if area.Value != original {
		return fmt.Errorf("queue editor loaded %q, want %q", area.Value, original)
	}
	w.tester.ClickAt(area.Bounds.X+area.Bounds.W/2, area.Bounds.Y+area.Bounds.H/2)
	w.tester.Key(ui.Cmd, ui.KeyA)
	w.tester.Type(replacement)
	w.tester.Frame()
	if err := w.button(label("queue_save")); err != nil {
		return err
	}
	if len(cv.Queue) != before+1 || cv.Queue[before].Text != replacement || !w.tester.HasText(replacement) {
		return fmt.Errorf("Save did not replace the queued message: %v", cv.Queue)
	}
	if draft := w.app.Composer(cv.ID).Draft; draft != "" {
		return fmt.Errorf("queue editing changed the composer draft to %q", draft)
	}
	w.evidence("assertion", fmt.Sprintf("Tab queued %q; opened its editor and saved %q; count stayed %d and composer stayed empty", original, replacement, len(cv.Queue)))
	return nil
}

func switchSessions(w *window) error {
	if err := w.open(nil); err != nil {
		return err
	}
	if err := w.selectConversation(idleSession); err != nil {
		return err
	}
	const first, second = "Acceptance: first session draft", "Acceptance: second session draft"
	if err := w.typeMessage(first); err != nil {
		return err
	}
	if err := w.selectConversation("sailry-go/i18n"); err != nil {
		return err
	}
	if err := w.typeMessage(second); err != nil {
		return err
	}
	if err := w.selectConversation(idleSession); err != nil {
		return err
	}
	input, err := w.control(ui.RoleTextField, label("composer"))
	if err != nil {
		return err
	}
	if input.Value != first || w.app.Composer("sailry-go/i18n").Draft != second {
		return fmt.Errorf("session switch lost or mixed drafts: shown %q, other %q", input.Value, w.app.Composer("sailry-go/i18n").Draft)
	}
	w.tester.Key(ui.Cmd, ui.KeyK)
	w.tester.Frame()
	w.tester.Frame()
	w.tester.Key(0, ui.KeyEnter)
	w.tester.Type("weeknight")
	w.tester.Frame()
	if !w.tester.HasText("Weeknight dinner ideas") {
		return fmt.Errorf("conversation search did not render its matching session")
	}
	w.tester.Key(0, ui.KeyEnter)
	w.tester.Frame()
	if w.app.Sidebar.Selected != "chat/recipe" {
		return fmt.Errorf("search selected %q, want chat/recipe", w.app.Sidebar.Selected)
	}
	input, err = w.control(ui.RoleTextField, label("composer"))
	if err != nil {
		return err
	}
	if input.Value != "" || w.app.Composer(idleSession).Draft != first {
		return fmt.Errorf("search mixed drafts: shown %q, first session %q", input.Value, w.app.Composer(idleSession).Draft)
	}
	w.result.Session = "chat/recipe"
	w.evidence("assertion", "Clicked between two sessions and retained their distinct drafts; Cmd+K search plus Enter opened chat/recipe with its empty draft")
	return nil
}

func loadHistory(w *window) error {
	if err := w.open(nil); err != nil {
		return err
	}
	cv := w.app.Data.Find(preview.Big)
	w.result.Session = cv.ID
	before, older := len(cv.Turns), cv.Session.Older
	if before != 20 || older == 0 {
		return fmt.Errorf("history fixture has %d loaded turns and %d older, want 20 and an older page", before, older)
	}
	first := cv.Turns[0].ID
	log, err := w.control(ui.RoleLog, "")
	if err != nil {
		return err
	}
	// Scroll the actual transcript to its start. Its visible older-page row
	// must request the page through the shell action, as it does in a window.
	w.tester.Scroll(log.Bounds.X+log.Bounds.W/2, log.Bounds.Y+80, 0, -1000000)
	w.tester.Frame()
	if !cv.Session.LoadingOlder {
		return fmt.Errorf("scrolling to the transcript start did not request an older page")
	}
	for step := 0; step < 6 && len(cv.Turns) == before; step++ {
		if err := w.advance(250 * time.Millisecond); err != nil {
			return err
		}
	}
	if len(cv.Turns) != before+20 || cv.Session.Older != older-20 || cv.Turns[20].ID != first {
		return fmt.Errorf("older page did not prepend intact: turns %d, older %d, first prior turn %q", len(cv.Turns), cv.Session.Older, first)
	}
	nav, err := w.control(ui.RoleSlider, label("chat_navigation"))
	if err != nil {
		return err
	}
	if !strings.HasSuffix(nav.Value, "/40") {
		return fmt.Errorf("message navigation did not include the older page: %q", nav.Value)
	}
	w.result.Turns = []string{cv.Turns[0].ID, first}
	w.evidence("assertion", fmt.Sprintf("Transcript scroll requested history; turns %d -> %d; older %d -> %d; prior first turn retained; navigation %q", before, len(cv.Turns), older, cv.Session.Older, nav.Value))
	return nil
}

func chooseReference(w *window) error {
	if err := w.open(nil); err != nil {
		return err
	}
	cv := w.app.Data.Find(preview.Big)
	w.result.Session = cv.ID
	if err := w.typeMessage("@"); err != nil {
		return err
	}
	options := 0
	for _, info := range w.tester.AccessAll() {
		if info.Role == ui.RoleOption {
			options++
		}
	}
	if options < 2 {
		return fmt.Errorf("@ opened %d picker options, want at least Files and Agents", options)
	}
	w.tester.Key(0, ui.KeyDown)
	w.tester.Key(0, ui.KeyEnter)
	w.tester.Frame()
	w.tester.Key(0, ui.KeyEnter)
	w.tester.Frame()
	comp := w.app.Composer(cv.ID)
	if comp.Draft != "@reviewer " || len(comp.References) != 1 {
		return fmt.Errorf("keyboard reference selection produced draft %q and %d references", comp.Draft, len(comp.References))
	}
	w.tester.Type(" @")
	w.tester.Frame()
	w.tester.Key(0, ui.KeyEscape)
	w.tester.Frame()
	for _, info := range w.tester.AccessAll() {
		if info.Role == ui.RoleOption {
			return fmt.Errorf("Escape left reference option %q open", info.Name)
		}
	}
	if !cv.Running() {
		return fmt.Errorf("Escape stopped the reply while closing the reference picker")
	}
	w.tester.Key(0, ui.KeyEscape)
	w.tester.Frame()
	if cv.Running() {
		return fmt.Errorf("Escape with the picker closed did not stop the reply")
	}
	w.evidence("assertion", "@ plus keyboard navigation created @reviewer and one reference; first Escape closed the picker and kept the reply running; second Escape stopped it")
	return nil
}

func resizeConversation(w *window) error {
	if err := w.open(nil); err != nil {
		return err
	}
	if err := w.selectConversation(idleSession); err != nil {
		return err
	}
	const text = "Acceptance: retain this draft when resizing"
	if err := w.typeMessage(text); err != nil {
		return err
	}
	w.tester.SetSize(760, 560)
	w.tester.Frame()
	input, err := w.control(ui.RoleTextField, label("composer"))
	if err != nil {
		return err
	}
	send, err := w.control(ui.RoleButton, label("send"))
	if err != nil {
		return err
	}
	for name, bounds := range map[string]ui.Rect{"composer": input.Bounds, "Send": send.Bounds} {
		if bounds.X < 0 || bounds.Y < 0 || bounds.X+bounds.W > 760.5 || bounds.Y+bounds.H > 560.5 {
			return fmt.Errorf("%s is outside the narrow window: %v", name, bounds)
		}
	}
	if input.Value != text {
		return fmt.Errorf("resizing changed draft to %q", input.Value)
	}
	cv := w.app.Data.Find(idleSession)
	before := len(cv.Turns)
	if err := w.button(label("send")); err != nil {
		return err
	}
	if len(cv.Turns) != before+1 || cv.Turns[before].User != text {
		return fmt.Errorf("narrow window Send did not append the expected message")
	}
	turn := cv.Turns[before].ID
	w.tester.SetSize(1280, 820)
	w.tester.Frame()
	if w.app.Sidebar.Selected != idleSession || cv.Turns[before].ID != turn || !cv.Running() {
		return fmt.Errorf("returning to wide layout changed the selected session or active turn")
	}
	if err := w.button(label("chat_stop")); err != nil {
		return err
	}
	w.result.Turns = []string{turn}
	w.evidence("assertion", fmt.Sprintf("Draft and controls fit 760x560; Send appended the message; session and active turn survived 1280x820; narrow composer %v; Send %v", input.Bounds, send.Bounds))
	return nil
}
