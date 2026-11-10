package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Settings › Dictation's speech models. They are the controller's: the
// selected one is the dictation.model preference, and no Node lists,
// registers, installs, cancels, deletes or saves a speech model (a Node
// refuses the speech kind). The controller's speech-model manager, which
// owns the registered models and their downloads, is the dictation
// feature's; until it exists the list is the recommended model alone and
// a download reports that dictation is unavailable.

// speechModels are the page's speech models and the selected one's index,
// -1 when the selected model is not listed. The recommended model has no
// ID; it is selected while the preference names none.
func speechModels(selected *protocol.ModelID) ([]preview.LocalModel, int) {
	models := []preview.LocalModel{{Status: preview.ModelMissing}}
	if selected == nil {
		return models, 0
	}
	id := selected.String()
	for i, model := range models {
		if model.ID == id {
			return models, i
		}
	}
	return models, -1
}

// speechSelection is the dictation.model preference for a row: none for
// the recommended model, and a registered model's ID otherwise.
func speechSelection(id string) (*protocol.ModelID, bool) {
	if id == "" {
		return nil, true
	}
	model, err := protocol.ParseModelID(id)
	if err != nil {
		return nil, false
	}
	return &model, true
}

// selectSpeechModel stores the selected speech model in the desktop
// preferences, which the frame then saves. The recommended model can
// always be selected; a registered one must be installed, and none is
// while the controller has no manager.
func (r *Runtime) selectSpeechModel(id string) {
	root := r.localRuntime()
	selected, valid := speechSelection(id)
	if !valid || selected != nil {
		root.enqueue(func(c *ui.Context) { kit.Message(c, i18n.T(c, "dictation_model_missing")) })
		return
	}
	d := &root.Preferences.Data
	if d.Dictation == nil {
		if selected == nil {
			return
		}
		d.Dictation = &preferences.Dictation{Language: preferences.DictationLanguages[0]}
	}
	d.Dictation.Model = selected
}

// speechUnavailable reports that speech models cannot be downloaded.
func (r *Runtime) speechUnavailable() {
	r.localRuntime().enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "dictation_unavailable")) })
}
