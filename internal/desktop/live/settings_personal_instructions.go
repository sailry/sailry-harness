package live

import (
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const personalInstructionsSaveDelay = 400 * time.Millisecond

type personalInstructionsState struct {
	editor   settings.PersonalInstructions
	revision *string
	timer    *time.Timer
	queued   bool
	writing  bool
	retained *protocol.CommandSavePersonalInstructionsPayload
	flushed  []func(bool) // close or quit continuations for this window's editors
}

func (r *Runtime) readPersonalInstructions() {
	s := &r.personalInstructions
	e := &s.editor
	if e.Pending || e.Uncertain {
		return
	}
	e.Pending = true
	r.executeScoped("", protocol.Command{Kind: "read_personal_instructions"}, func(c *ui.Context, out protocol.Output, err error) {
		e.Pending = false
		value, ok := detail[protocol.PersonalInstructions](out.Data)
		if err != nil || !ok || out.Kind != "personal_instructions" {
			e.Known = false
			if r.App.Settings.PersonalInstructions == e {
				r.reportSetting(c, kit.Failed, settings.PersonalInstructionsMessages)
			}
			return
		}
		if e.Draft == e.Saved {
			e.Draft = value.Text
		}
		s.revision, e.Saved, e.Known = value.Revision, value.Text, true
		r.flushPersonalInstructionsSave()
	}, r.deliverInstructions)
}

// The settings host that received the edit also owns its pending save when the
// user switches hosts or leaves the page.
func (r *Runtime) queuePersonalInstructionsSave() {
	s := &r.personalInstructions
	s.editor.Failed, s.queued = false, true
	if s.timer != nil {
		s.timer.Stop()
	}
	var timer *time.Timer
	timer = time.AfterFunc(personalInstructionsSaveDelay, func() {
		r.enqueue(func(*ui.Context) {
			if s.timer == timer {
				s.timer = nil
				r.flushPersonalInstructionsSave()
			}
		})
	})
	s.timer = timer
}

// FlushEdits sends settings edits without waiting for focus or the save delay.
// A false return allows the window to close immediately. A true return keeps
// it open until done receives the settled save outcome. A nil callback just
// starts the saves, for a session end the operating system cannot defer.
func (r *Runtime) FlushEdits(done func(bool)) bool {
	root := r.localRuntime()
	pending, clean := root.flushEdits()
	if pending && done != nil {
		root.personalInstructions.flushed = append(root.personalInstructions.flushed, done)
	} else if !clean && done != nil {
		done(false)
	}
	return pending || !clean
}

func (r *Runtime) flushEdits() (pending, clean bool) {
	if e := r.App.Settings.PersonalInstructions; e != nil && e.Detach() && r.instructionsOwner != nil {
		r.instructionsOwner.queuePersonalInstructionsSave()
	}
	clean = true
	flush := func(owner *Runtime) {
		s := &owner.personalInstructions
		// The text binding can change before a frame observes the edit.
		if s.editor.Draft != s.editor.Saved && !s.editor.Failed && !s.editor.Uncertain {
			s.queued = true
		}
		if s.timer != nil {
			s.timer.Stop()
			s.timer = nil
		}
		owner.flushPersonalInstructionsSave()
		pending = pending || s.writing || s.editor.Pending && s.queued
		clean = clean && s.editor.Draft == s.editor.Saved && !s.editor.Uncertain
	}
	flush(r)
	for _, bound := range r.bindings {
		flush(bound)
	}
	return pending, clean
}

func (r *Runtime) finishEditFlush() {
	waiters := r.personalInstructions.flushed
	if len(waiters) == 0 {
		return
	}
	if pending, clean := r.flushEdits(); !pending {
		r.personalInstructions.flushed = nil
		for _, done := range waiters {
			done(clean)
		}
	}
}

// In a hidden window a frame may be delayed indefinitely. Apply the editor
// result on the UI thread using the window's update callback. Any toast
// still belongs to its next frame.
func (r *Runtime) deliverInstructions(apply func(*ui.Context)) {
	root := r.localRuntime()
	root.mu.Lock()
	update := root.update
	root.mu.Unlock()
	if update == nil {
		root.enqueue(func(c *ui.Context) {
			apply(c)
			root.finishEditFlush()
		})
		return
	}
	update(func() {
		if root.ctx.Err() != nil {
			return
		}
		if r.ctx.Err() == nil {
			apply(nil)
		}
		root.finishEditFlush()
	})
}

func (r *Runtime) flushPersonalInstructionsSave() {
	s := &r.personalInstructions
	if s.queued && s.timer == nil && !s.editor.Pending && !s.editor.Failed && !s.editor.Uncertain {
		r.savePersonalInstructions()
	}
}

func (r *Runtime) savePersonalInstructions() {
	s := &r.personalInstructions
	e := &s.editor
	if e.Pending || !e.Known {
		return
	}
	if r.machineSettingsStatus() != "" {
		e.Failed = true
		return
	}
	request := protocol.CommandSavePersonalInstructionsPayload{Text: e.Draft, ExpectedRevision: s.revision}
	if s.retained != nil {
		request = *s.retained
	} else {
		s.queued = false
		if s.timer != nil {
			s.timer.Stop()
			s.timer = nil
		}
		if e.Draft == e.Saved {
			return
		}
	}
	e.Pending, e.Failed, s.writing = true, false, true
	command := protocol.Command{Kind: "save_personal_instructions", Data: request}
	r.saveSettingResult(command, settings.PersonalInstructionsMessages, func(c *ui.Context, out protocol.Output, err error) {
		e.Pending, s.writing = false, false
		e.Uncertain = client.Retained(err)
		s.retained = nil
		if e.Uncertain {
			s.retained = &request
		}
		if err != nil {
			e.Failed = true
			if !e.Uncertain && (client.Released(err) || faultCode(err) == protocol.ErrorCodeConflict || faultCode(err) == protocol.ErrorCodeRevisionConflict) {
				r.readPersonalInstructions()
			}
			return
		}
		value, ok := detail[protocol.PersonalInstructions](out.Data)
		if !ok || out.Kind != "personal_instructions" {
			e.Failed = true
			r.reportSetting(c, kit.Uncertain, settings.PersonalInstructionsMessages)
			r.readPersonalInstructions()
			return
		}
		if e.Draft == request.Text {
			e.Draft = value.Text
		}
		s.revision, e.Saved = value.Revision, value.Text
		r.flushPersonalInstructionsSave()
	}, r.deliverInstructions)
}
