package settings

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// MediaMessages are the toasts of a media models save.
var MediaMessages = Messages{"", "media_failed", "media_conflict", "media_unknown"}

// mediaModels is the Media models page: the model each media purpose uses, Vision, Image, Video and
// Speech, each a model select of the models tagged for it under Follow
// conversation model. A choice saves at once; a failed save keeps it as the
// draft. A saved model that can no longer run, or that is no longer
// offered, stays the select's choice, marked there.
func mediaModels(c *ui.Context, m *Model) {
	known := m.modelOptions()
	kit.Group(c, "media", "", "", nil, func() {
		kit.Card(c, func() {
			for i, kind := range viewmodel.MediaKinds {
				draft := m.Media.Draft[i]
				var models []kit.ModelOption
				stored, unavailable := "", ""
				for _, option := range m.MediaOptions {
					model := mediaOption(known, option)
					switch {
					case slices.Contains(option.Kinds, kind):
						models = append(models, model)
					case option.Key == draft:
						stored, unavailable = model.Name, i18n.T(c, "media_model_unavailable")
					}
				}
				if m.MediaUnavailable[i] && draft != "" && draft == m.Media.Saved[i] {
					unavailable = i18n.T(c, "media_model_unavailable")
				}
				title := i18n.T(c, mediaTitles[i])
				kit.WideRow(c, kind, title, "", func() {
					kit.ModelSelect(c.Key("select"), kit.ModelSelectProps{Label: title, None: i18n.T(c, "media_follow"), Models: models,
						Model: draft, Stored: stored, Unavailable: unavailable, Disabled: m.Media.Pending,
						OnChange: func(id, _ string) {
							m.Media.Draft[i] = id
							if m.Live && m.SaveMedia != nil {
								m.SaveMedia()
							} else {
								report(c, m.Media.Commit(), MediaMessages)
							}
						}}).Grow(1).Shrink(1).MinWidth(0)
				})
			}
		})
	})
}

// mediaOption is a media option as the model select offers it: the
// channel's model its key names, without efforts, which a media purpose
// does not take; one whose channel is not shown has its label's provider
// and model.
func mediaOption(known []kit.ModelOption, option viewmodel.MediaOption) kit.ModelOption {
	for _, model := range known {
		if model.ID == option.Key {
			model.Efforts, model.DefaultEffort = nil, ""
			return model
		}
	}
	provider, name, found := strings.Cut(option.Label, " · ")
	if !found {
		provider, name = "", option.Label
	}
	return kit.ModelOption{ID: option.Key, Name: name, Provider: provider}
}

// mediaTitles are the media rows' titles, in viewmodel.MediaKinds' order.
var mediaTitles = [4]string{"provider_cap_vision", "media_image", "media_video", "media_speech"}
