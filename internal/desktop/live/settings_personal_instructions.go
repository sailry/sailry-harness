package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type personalInstructionsState struct {
	editor   settings.PersonalInstructions
	revision *string
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
				kit.Toast(c, kit.Failed, i18n.T(c, "settings_custom_prompt_failed"))
			}
			return
		}
		if e.Draft == e.Saved {
			e.Draft = value.Text
		}
		s.revision, e.Saved, e.Known = value.Revision, value.Text, true
	})
}

func (r *Runtime) savePersonalInstructions() {
	s := &r.personalInstructions
	e := &s.editor
	if r.App.Settings.PersonalInstructions != e || e.Pending || !e.Known || (!e.Uncertain && e.Draft == e.Saved) {
		return
	}
	command := protocol.Command{Kind: "save_personal_instructions", Data: protocol.CommandSavePersonalInstructionsPayload{Text: e.Draft, ExpectedRevision: s.revision}}
	e.Pending = true
	r.execute(command, func(c *ui.Context, out protocol.Output, err error) {
		e.Pending = false
		e.Uncertain = client.Retained(err)
		if err != nil {
			if r.App.Settings.PersonalInstructions == e {
				r.reportKeys(c, err, "settings_custom_prompt_failed", "settings_custom_prompt_conflict", "settings_custom_prompt_unknown")
			}
			if !e.Uncertain && (client.Released(err) || faultCode(err) == protocol.ErrorCodeConflict || faultCode(err) == protocol.ErrorCodeRevisionConflict) {
				r.readPersonalInstructions()
			}
			return
		}
		value, ok := detail[protocol.PersonalInstructions](out.Data)
		if !ok || out.Kind != "personal_instructions" {
			if r.App.Settings.PersonalInstructions == e {
				kit.Toast(c, kit.Uncertain, i18n.T(c, "settings_custom_prompt_unknown"))
			}
			r.readPersonalInstructions()
			return
		}
		s.revision, e.Saved, e.Draft = value.Revision, value.Text, value.Text
		if r.App.Settings.PersonalInstructions == e {
			kit.Toast(c, kit.Done, i18n.T(c, "workspace_saved"))
		}
	})
}
