package live

import (
	"slices"
	"testing"
	"testing/synctest"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A close waits for the latest draft and the revision from the preceding
// write even when the window builds no frames.
func TestPersonalInstructionsFlushBeforeClosing(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source := terminalTabsFixture(t)
		e := &r.personalInstructions.editor
		e.Known = true
		r.App.Settings.PersonalInstructions = e
		updates := make(chan func(), 8)
		r.SetUpdate(func(f func()) { updates <- f })
		drain := func() {
			synctest.Wait()
			for len(updates) > 0 {
				(<-updates)()
				synctest.Wait()
			}
		}
		first, latest := make(chan struct{}), make(chan struct{})
		release := func(ready chan struct{}) {
			select {
			case <-ready:
			default:
				close(ready)
			}
		}
		defer release(first)
		defer release(latest)
		source.respond = func(request protocol.Request) link.Response {
			input, _ := detail[protocol.CommandSavePersonalInstructionsPayload](request.Command.Data)
			switch input.Text {
			case "First prompt":
				<-first
			case "Latest prompt":
				<-latest
			}
			return link.Response{Output: protocol.Output{Kind: "personal_instructions", Data: protocol.PersonalInstructions{Text: input.Text, Revision: &input.Text}}}
		}
		e.Draft = "First prompt"
		r.queuePersonalInstructionsSave()
		finished := false
		if !r.FlushEdits(func(saved bool) { finished = saved }) {
			t.Fatal("close proceeded before the edit was saved")
		}
		drain()
		if len(source.sent("save_personal_instructions")) != 1 || r.personalInstructions.timer != nil || finished {
			t.Fatal("close did not flush the debounced edit and wait for its result")
		}
		// The next frame has not yet observed this edit.
		e.Draft = "Latest prompt"
		release(first)
		drain()
		commands := source.sent("save_personal_instructions")
		if len(commands) != 2 || finished || e.Saved != "First prompt" {
			t.Fatalf("close did not take the latest draft: %d writes, finished=%v, editor=%+v", len(commands), finished, e)
		}
		next, _ := detail[protocol.CommandSavePersonalInstructionsPayload](commands[1].Data)
		if next.Text != "Latest prompt" || next.ExpectedRevision == nil || *next.ExpectedRevision != "First prompt" {
			t.Fatalf("latest write used the wrong capture: %+v", next)
		}
		release(latest)
		drain()
		if !finished || e.Saved != e.Draft || r.FlushEdits(nil) || len(source.sent("save_personal_instructions")) != 2 {
			t.Fatalf("close did not proceed once the latest write settled: %+v", e)
		}
	})
}

// instructionsCloseFixture is a Runtime whose window delivers updates only
// when drain runs them.
func instructionsCloseFixture(t *testing.T) (*Runtime, *terminalTabsTestNode, *settings.PersonalInstructions, func()) {
	r, source := terminalTabsFixture(t)
	e := &r.personalInstructions.editor
	e.Known = true
	r.App.Settings.PersonalInstructions = e
	updates := make(chan func(), 8)
	r.SetUpdate(func(f func()) { updates <- f })
	return r, source, e, func() {
		synctest.Wait()
		for len(updates) > 0 {
			(<-updates)()
			synctest.Wait()
		}
	}
}

func queuedReports(r *Runtime) int {
	r.mu.Lock()
	defer r.mu.Unlock()
	return len(r.queue)
}

// A failed save keeps its draft and holds only the close that waited for it.
func TestPersonalInstructionsFailedSaveAllowsAnotherClose(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source, e, drain := instructionsCloseFixture(t)
		e.Draft = "Unsaved prompt"
		r.queuePersonalInstructionsSave()
		var settled []bool
		if !r.FlushEdits(func(saved bool) { settled = append(settled, saved) }) {
			t.Fatal("close did not wait for the save in flight")
		}
		drain()
		if !slices.Equal(settled, []bool{false}) || !e.Failed || e.Draft != "Unsaved prompt" {
			t.Fatalf("the failed save did not resume the close with its draft: %v, %+v", settled, e)
		}
		if r.FlushEdits(func(bool) { t.Fatal("the repeated close waited again") }) {
			t.Fatal("a failed save kept the window from closing")
		}
		if len(source.sent("save_personal_instructions")) != 1 {
			t.Fatal("the repeated close sent the failed save again")
		}
	})
}

// A draft that cannot be saved holds one close to report it, then lets the
// next close go through.
func TestPersonalInstructionsUnsavableDraftReportsOnce(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source, e, _ := instructionsCloseFixture(t)
		// The instructions could not be read, so no save can run.
		e.Known, e.Draft = false, "Unsaved prompt"
		before := queuedReports(r)
		held := false
		if !r.FlushEdits(func(saved bool) { held = !saved }) || !held || queuedReports(r) == before {
			t.Fatal("the first close did not report the unsaved draft")
		}
		if r.FlushEdits(func(bool) { t.Fatal("the repeated close waited again") }) {
			t.Fatal("an unsaved draft kept the window from closing")
		}
		if len(source.sent("save_personal_instructions")) != 0 || e.Draft != "Unsaved prompt" {
			t.Fatalf("close changed the draft or saved without instructions: %+v", e)
		}
	})
}

// A save that outlives its window's context still resumes the waiting close.
func TestPersonalInstructionsCloseSettlesAfterCancellation(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source, e, drain := instructionsCloseFixture(t)
		// The host answers nothing until the window's context ends.
		source.respond = func(protocol.Request) link.Response {
			<-r.ctx.Done()
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "host is not answering"}}
		}
		e.Draft = "Unsaved prompt"
		r.queuePersonalInstructionsSave()
		var settled []bool
		if !r.FlushEdits(func(saved bool) { settled = append(settled, saved) }) {
			t.Fatal("close did not wait for the save in flight")
		}
		drain()
		if settled != nil {
			t.Fatal("close resumed before the save settled")
		}
		r.cancel()
		drain()
		if !slices.Equal(settled, []bool{false}) || len(r.personalInstructions.flushed) != 0 {
			t.Fatalf("a cancelled save left its close waiting: %v", settled)
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
