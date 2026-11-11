package settings

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// MediaMessages are the toasts of a media models save.
var MediaMessages = Messages{"", "media_failed", "media_conflict", "media_unknown"}

// mediaModels is the Media models page: the model each media purpose uses, Vision, Image, Video and
// Speech, each a select of the models tagged for it under Follow conversation
// model. A choice saves at once; a failed save keeps it as the draft. A saved
// model that can no longer run is marked and stays the select's choice.
func mediaModels(c *ui.Context, m *Model) {
	p := theme.Of(c)
	kit.Group(c, "media", "", "", nil, func() {
		kit.Card(c, func() {
			for i, kind := range viewmodel.MediaKinds {
				labels, values := []string{i18n.T(c, "media_follow")}, []string{""}
				for _, option := range m.MediaOptions {
					if slices.Contains(option.Kinds, kind) || option.Key == m.Media.Draft[i] {
						labels, values = append(labels, option.Label), append(values, option.Key)
					}
				}
				title := i18n.T(c, mediaTitles[i])
				kit.WideRow(c, kind, title, "", func() {
					if m.MediaUnavailable[i] && m.Media.Draft[i] != "" && m.Media.Draft[i] == m.Media.Saved[i] {
						ui.Row(c.Key("status")).Shrink(0).Gap(6).Children(func() {
							ui.Box(c).Size(6, 6).Radius(3).Background(p.Warning)
							ui.Text(c, i18n.T(c, "media_model_unavailable")).FontSize(14).TextColor(p.MutedForeground).SingleLine()
						})
					}
					b, changed := kit.Select(c.Key("select"), "media-"+kind, title, &m.Media.Draft[i], labels, m.Media.Pending, values)
					b.Grow(1).Shrink(1).MinWidth(0)
					if !changed {
						return
					}
					if m.Live && m.SaveMedia != nil {
						m.SaveMedia()
					} else {
						report(c, m.Media.Commit(), MediaMessages)
					}
				})
			}
		})
	})
}

// mediaTitles are the media rows' titles, in viewmodel.MediaKinds' order.
var mediaTitles = [4]string{"provider_cap_vision", "media_image", "media_video", "media_speech"}
