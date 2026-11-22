package desktop

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func Page() settings.NativePage {
	return settings.NativePage{
		Section: settings.Media,
		Group:   "settings_group_ai",
		Icon:    icons.Image,
		Order:   200,
		Render:  render,
	}
}

// render offers each media purpose its tagged models. A choice saves at once;
// a failed save keeps it as the draft. A saved model that is unavailable or
// no longer offered stays selected and marked.
func render(c *ui.Context, s *settings.State) {
	known := s.ModelOptions(s.ManagedNode)
	kit.Group(c, "media", "", "", nil, func() {
		kit.Card(c, func() {
			for i, kind := range viewmodel.MediaKinds {
				draft := s.Media.Draft[i]
				var models []kit.ModelOption
				stored, unavailable := "", ""
				for _, option := range s.MediaOptions {
					model := mediaOption(known, option)
					switch {
					case slices.Contains(option.Kinds, kind):
						models = append(models, model)
					case option.Key == draft:
						stored, unavailable = model.Name, i18n.T(c, "media_model_unavailable")
					}
				}
				if s.MediaUnavailable[i] && draft != "" && draft == s.Media.Saved[i] {
					unavailable = i18n.T(c, "media_model_unavailable")
				}
				title := i18n.T(c, mediaTitles[i])
				kit.WideRow(c, kind, title, "", func() {
					kit.ModelSelect(c.Key("select"), kit.ModelSelectProps{Label: title, None: i18n.T(c, "media_follow"), Models: models,
						Model: draft, Stored: stored, Unavailable: unavailable, Disabled: s.Media.Pending,
						OnChange: func(id, _ string) {
							s.Media.Draft[i] = id
							if s.Live && s.SaveMedia != nil {
								s.SaveMedia()
							} else {
								settings.Report(c, s.Media.Commit(), settings.MediaMessages)
							}
						}}).Grow(1).Shrink(1).MinWidth(0)
				})
			}
		})
	})
}

// mediaOption is a media option as the model select offers it: the channel's
// model without efforts, or the provider and model from the option's label.
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
