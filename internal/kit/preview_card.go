package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/theme"
)

// Session is what a session's hover card shows. Branch and Path are ""
// where the card does not show them.
type Session struct {
	Title, Project, Branch, Path string
}

// SessionCard shows a session's hover card by anchor once the pointer
// rests on it (Kit's HoverCard, on MyGo's TooltipBase and its delay), in
// the popover color. Detailed is the sidebar's card (sidebar/details.rs):
// 320 wide, the title over the project and branch with their icons and
// the mono path. Otherwise it is the activity strip's preview
// (ui/session_preview.rs): 280 wide, the semibold title over the project.
// place puts the card by anchor, on side, from which it moves in with
// TipMotion.
func SessionCard(c *ui.Context, anchor ui.Element, s Session, detailed bool, side Placement, place func(card ui.Element)) {
	p := theme.Of(c)
	if TipsHidden(c) {
		return
	}
	w, _ := c.Size()
	ui.TooltipBase(c, anchor, func(card ui.Element) {
		Anchored(card, anchor, CardLayer)
		place(card)
		card.AttachedOrigin().Transition(TipMotion(side))
		width := float32(280)
		if detailed {
			width = 320
		}
		Floating(c, card).Width(min(width, w-48) + 2*12).Padding(12).Gap(8)
		card.Children(func() {
			if !detailed {
				ui.Text(c, s.Title).FontSize(14).FontWeight(600).LineHeight(20.0 / 14).MaxLines(2)
				ui.Text(c, s.Project).FontSize(12).SingleLine().TextColor(p.MutedForeground)
				return
			}
			ui.Text(c, s.Title).FontSize(14).FontWeight(500).MaxLines(3)
			if s.Project == "" && s.Branch == "" && s.Path == "" {
				return
			}
			ui.Column(c).Gap(4).TextColor(p.MutedForeground).Children(func() {
				line := func(icon *ui.SVG, text string) {
					if text == "" {
						return
					}
					ui.Row(c).Gap(6).MinWidth(0).Children(func() {
						ui.Icon(c, icon).FontSize(14).Shrink(0)
						ui.Text(c, text).FontSize(13).SingleLine().Grow(1).Shrink(1).MinWidth(0)
					})
				}
				line(icons.Folder, s.Project)
				line(icons.GitBranch, s.Branch)
				if s.Path != "" {
					ui.Text(c, s.Path).Font("monospace").FontSize(13)
				}
			})
		})
	})
}
