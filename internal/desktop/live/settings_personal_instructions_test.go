package live

import (
	"testing"
	"testing/synctest"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Closing the window or quitting saves an edit still within its delay.
func TestPersonalInstructionsFlushBeforeClosing(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source := terminalTabsFixture(t)
		e := &r.personalInstructions.editor
		e.Known = true
		r.App.Settings.PersonalInstructions = e
		source.respond = func(request protocol.Request) link.Response {
			input, _ := detail[protocol.CommandSavePersonalInstructionsPayload](request.Command.Data)
			return link.Response{Output: protocol.Output{Kind: "personal_instructions", Data: protocol.PersonalInstructions{Text: input.Text, Revision: &input.Text}}}
		}
		e.Draft = "Closing prompt"
		r.queuePersonalInstructionsSave()
		r.FlushEdits()
		synctest.Wait()
		commands := source.sent("save_personal_instructions")
		if len(commands) != 1 || r.personalInstructions.timer != nil {
			t.Fatalf("the pending edit was not saved before closing: %d writes", len(commands))
		}
		if input, _ := detail[protocol.CommandSavePersonalInstructionsPayload](commands[0].Data); input.Text != "Closing prompt" {
			t.Fatalf("the flushed save wrote %q", input.Text)
		}
		r.FlushEdits()
		synctest.Wait()
		if len(source.sent("save_personal_instructions")) != 1 {
			t.Fatal("flushing again repeated the save")
		}
	})
}

func TestPersonalInstructionsSaveAfterEditing(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source := terminalTabsFixture(t)
		e := &r.personalInstructions.editor
		e.Known = true
		r.App.Settings.PersonalInstructions = e
		firstSave := make(chan struct{})
		defer func() {
			if firstSave != nil {
				close(firstSave)
			}
		}()
		source.respond = func(request protocol.Request) link.Response {
			input, _ := detail[protocol.CommandSavePersonalInstructionsPayload](request.Command.Data)
			if input.Text == "First prompt" {
				<-firstSave
			}
			return link.Response{Output: protocol.Output{Kind: "personal_instructions", Data: protocol.PersonalInstructions{Text: input.Text, Revision: &input.Text}}}
		}
		tester := pluginUpdateQueueTester(r)
		frame := func() {
			synctest.Wait()
			tester.Frame()
			synctest.Wait()
		}
		e.Draft = "First"
		r.queuePersonalInstructionsSave()
		time.Sleep(personalInstructionsSaveDelay / 2)
		e.Draft = "First prompt"
		r.queuePersonalInstructionsSave()
		time.Sleep(personalInstructionsSaveDelay / 2)
		frame()
		if len(source.sent("save_personal_instructions")) != 0 {
			t.Fatal("autosave did not wait for editing to pause")
		}
		time.Sleep(personalInstructionsSaveDelay / 2)
		frame()
		if !e.Pending || len(source.sent("save_personal_instructions")) != 1 {
			t.Fatal("the edited prompt did not start saving")
		}

		e.Draft = "Second prompt"
		r.queuePersonalInstructionsSave()
		other := &settings.PersonalInstructions{Known: true, Saved: "Other host", Draft: "Other host"}
		r.App.Settings.PersonalInstructions = other
		time.Sleep(personalInstructionsSaveDelay)
		frame()
		if len(source.sent("save_personal_instructions")) != 1 {
			t.Fatal("a newer edit saved before the pending write returned a revision")
		}
		close(firstSave)
		synctest.Wait()
		firstSave = nil
		frame()
		frame()
		commands := source.sent("save_personal_instructions")
		if len(commands) != 2 || e.Saved != "Second prompt" || e.Draft != e.Saved || other.Draft != "Other host" {
			t.Fatalf("autosave lost the newer draft or changed the shown host: %d writes, %+v, %+v", len(commands), e, other)
		}
		second, _ := detail[protocol.CommandSavePersonalInstructionsPayload](commands[1].Data)
		if second.ExpectedRevision == nil || *second.ExpectedRevision != "First prompt" {
			t.Fatal("the next draft did not use the revision of the committed save")
		}
	})
}
