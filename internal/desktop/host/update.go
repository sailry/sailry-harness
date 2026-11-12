package host

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// updateDialog is the host's update: 480 wide, the preview's note, the
// headline, the versions, the note of an unavailable update, the progress,
// then Close and the phase's action. A failure toasts its detail.
func updateDialog(c *ui.Context, s *State, h *Host, act Actions) {
	if !s.updating {
		return
	}
	p := theme.Of(c)
	w, wh := c.Size()
	title := i18n.T(c, "host_update")
	u := h.Update
	kit.Dialog(c.Key("host-update"), &s.updating, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600).TextColor(p.PopoverForeground)
		if !h.plain(c) {
			ui.Text(c, i18n.T(c, "update_preview")).FontSize(14).TextColor(p.MutedForeground)
		}
		ui.Scroll(c.Key("status")).MaxHeight(min(max(wh-300, 80), 300)).Gap(12).Children(func() {
			if u == nil {
				ui.Text(c, i18n.T(c, "update_target_changed")).FontSize(14)
				return
			}
			headline := u.Headline()
			if h.plain(c) {
				headline = "live_" + headline
			}
			ui.Text(c, i18n.T(c, headline)).FontSize(14).FontWeight(600)
			current := u.Current
			if current == "" {
				current = i18n.T(c, "host_unknown")
			}
			versions := i18n.T(c, "update_version", "current", current)
			if u.Target != "" && u.Target != u.Current {
				versions = i18n.T(c, "update_versions", "current", current, "target", u.Target)
			}
			ui.Text(c, versions).FontSize(14).SingleLine().TextColor(p.MutedForeground)
			if u.Note != "" && (u.Phase == Idle || h.Live) {
				ui.Text(c, i18n.T(c, u.Note)).FontSize(14).TextColor(p.MutedForeground)
			}
			if u.Progress >= 0 {
				ui.Progress(c.Key("progress"), float64(min(u.Progress, 100))/100).FillWidth().Height(4).Label(title)
			}
		})
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Padding(12, 0, 0, 0).Children(func() {
			kit.Secondary(c.Key("close"), i18n.T(c, "close")).OnClick(func() { s.updating = false })
			if u == nil {
				return
			}
			if _, label, ok := u.Primary(); ok {
				expected := *u
				if h.plain(c) {
					label = "live_" + label
				}
				text := i18n.T(c, label)
				kit.Primary(c.Key("action"), text, false).Children(func() { ui.Text(c, text).FontSize(14) }).OnClick(func() {
					if act.Update != nil {
						act.Update(expected)
						if action, _, _ := expected.Primary(); action == Apply {
							s.updating = false
						}
						return
					}
					u.Advance(expected)
				})
			}
		})
	}).Width(min(480, w-48)).Label(title)
	if !h.Live && u != nil && u.Error != "" && s.failed != u.Error {
		kit.Error(c, title, u.Error)
	}
	if u != nil {
		s.failed = u.Error
	}
}

// OpenUpdate opens the host's update dialog, for a host updated from
// here.
func (s *State) OpenUpdate(h *Host) {
	if h != nil && h.Update != nil {
		s.updating = true
	}
}
