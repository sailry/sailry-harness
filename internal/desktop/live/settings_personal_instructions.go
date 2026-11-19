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
	retained *protocol.CommandSavePersonalInstructionsPayload
}

func (r *Runtime) readPersonalInstructions() {
	s := &r.personalInstructions
	e := &s.editor
	if e.Pending || e.Uncertain {
		return
	}
	e.Pending = true
	r.execute(protocol.Command{Kind: "read_personal_instructions"}, func(c *ui.Context, out protocol.Output, err error) {
		e.Pending = false
		value, ok := detail[protocol.PersonalInstructions](out.Data)
		if err != nil || !ok || out.Kind != "personal_instructions" {
			e.Known = false
			if r.App.Settings.PersonalInstructions == e {
				settings.Report(c, kit.Failed, settings.PersonalInstructionsMessages)
			}
			return
		}
		if e.Draft == e.Saved {
			e.Draft = value.Text
		}
		s.revision, e.Saved, e.Known = value.Revision, value.Text, true
		r.flushPersonalInstructionsSave()
	})
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
	e.Pending, e.Failed = true, false
	command := protocol.Command{Kind: "save_personal_instructions", Data: request}
	r.saveSettingResult(command, settings.PersonalInstructionsMessages, func(c *ui.Context, out protocol.Output, err error) {
		e.Pending = false
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
			settings.Report(c, kit.Uncertain, settings.PersonalInstructionsMessages)
			r.readPersonalInstructions()
			return
		}
		if e.Draft == request.Text {
			e.Draft = value.Text
		}
		s.revision, e.Saved = value.Revision, value.Text
		r.flushPersonalInstructionsSave()
	})
}
