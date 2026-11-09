package shell

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// queueButtons returns the buttons named name, and how many are disabled.
func queueButtons(tt *ui.Tester, name string) (all, disabled int) {
	for _, e := range tt.AccessAll() {
		if e.Role == ui.RoleButton && e.Name == name {
			all++
			if e.Disabled {
				disabled++
			}
		}
	}
	return all, disabled
}

// Enter with text while a reply runs guides it: the message waits in the
// queue with its guidance pending, the button stays Stop, and the row
// offers Withdraw with Edit, Send now and Remove disabled. Withdraw puts
// it back in the queue.
func TestComposerGuidesRunningTurn(t *testing.T) {
	a, tt := newTester(t)
	cv := a.Data.Find(preview.Big)
	comp := a.Composer(preview.Big)
	if !cv.Running() {
		t.Fatal("the big conversation does not run")
	}
	n, turns := len(cv.Queue), len(cv.Turns)
	click(t, tt, i18n.In("en", "composer"))
	tt.Type("Use the attached sample")
	tt.Key(0, ui.KeyEnter)
	tt.Frame()
	if !cv.Running() || len(cv.Turns) != turns || comp.Draft != "" {
		t.Fatalf("Enter did not guide: running %v, %d turns, draft %q", cv.Running(), len(cv.Turns), comp.Draft)
	}
	if len(cv.Queue) != n+1 || cv.Queue[n].Text != "Use the attached sample" || !cv.Queue[n].Guided {
		t.Fatalf("no pending guidance: %+v", cv.Queue)
	}
	if _, ok := tt.AccessByRole(ui.RoleButton, i18n.In("en", "chat_stop")); !ok {
		t.Error("the button is not Stop")
	}
	if all, _ := queueButtons(tt, i18n.In("en", "queue_withdraw")); all != 1 {
		t.Fatalf("%d Withdraw buttons", all)
	}
	for _, key := range []string{"queue_send_now", "queue_delete"} {
		if all, off := queueButtons(tt, i18n.In("en", key)); all != n+1 || off != 1 {
			t.Errorf("%s: %d buttons, %d disabled", key, all, off)
		}
	}
	if _, off := queueButtons(tt, i18n.In("en", "queue_edit")); off != 1 {
		t.Errorf("%d Edit buttons disabled", off)
	}
	tt.Advance(time.Second)
	tt.Frame()
	clickButton(t, tt, i18n.In("en", "queue_withdraw"))
	tt.Frame()
	if cv.Queue[n].Guided || !cv.Running() {
		t.Fatalf("Withdraw: %+v, running %v", cv.Queue[n], cv.Running())
	}
	if all, _ := queueButtons(tt, i18n.In("en", "queue_withdraw")); all != 0 {
		t.Error("Withdraw stays after withdrawing")
	}
}

// Send now joins a queued message to the running reply without stopping
// it; Enter in an empty draft does so for the first message not guiding
// yet, and sends it once nothing runs.
func TestQueueSendNowGuides(t *testing.T) {
	a, tt := newTester(t)
	cv := a.Data.Find(preview.Big)
	first, text, second := cv.Queue[0].ID, cv.Queue[0].Text, cv.Queue[1].ID
	var send ui.AccessInfo
	for _, e := range tt.AccessAll() {
		if e.Role == ui.RoleButton && e.Name == i18n.In("en", "queue_send_now") && (send.Name == "" || e.Bounds.Y < send.Bounds.Y) {
			send = e
		}
	}
	if send.Name == "" {
		t.Fatal("no Send now")
	}
	tt.ClickAt(send.Bounds.X+send.Bounds.W/2, send.Bounds.Y+send.Bounds.H/2)
	tt.Frame()
	if !cv.Running() || cv.Queue[0].ID != first || !cv.Queue[0].Guided {
		t.Fatalf("Send now: running %v, %+v", cv.Running(), cv.Queue)
	}
	click(t, tt, i18n.In("en", "composer"))
	tt.Key(0, ui.KeyEnter)
	tt.Frame()
	if !cv.Running() || !cv.Queue[1].Guided {
		t.Fatalf("an empty Enter did not guide the next message: %+v", cv.Queue)
	}
	preview.Stop(cv)
	tt.Frame()
	if cv.Queue[0].Guided || cv.Queue[1].Guided {
		t.Fatalf("the stopped reply kept the guidance: %+v", cv.Queue)
	}
	turns := len(cv.Turns)
	click(t, tt, i18n.In("en", "composer"))
	tt.Key(0, ui.KeyEnter)
	tt.Frame()
	if len(cv.Turns) != turns+1 || cv.Turns[turns].User != text || cv.Queue[0].ID != second || !cv.Playing() {
		t.Fatalf("an idle empty Enter did not send the first message: %d turns, %+v", len(cv.Turns), cv.Queue)
	}
}
