// Package onboarding is the welcome guide: the first launch's dialog with the
// brand banner and the providers' picker. See docs/onboarding.md.
package onboarding

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Width is the guide's width, less 48 in a narrower window.
const Width = 560

// Guide is the welcome guide's state.
type Guide struct {
	Open bool
	// Seen is whether the guide was dismissed once: it opens at launch while
	// false.
	Seen bool
}

// View is the guide: a dialog 560 wide and 85% of the window high at most,
// without the dialog's padding or close button. The banner, 34 over 28 under,
// holds the white mark, the title and the description over the brand
// gradient, with a white close at its top right; the picker follows. picker
// draws the providers' picker (settings.ProviderPicker), whose choice
// continues in Settings › Providers. Closing it in any way marks it seen.
func View(c *ui.Context, g *Guide, picker func()) {
	if !g.Open {
		return
	}
	p := theme.Of(c)
	w, h := c.Size()
	title := i18n.T(c, "onboarding_title")
	panel := kit.BareDialog(c.Key("welcome"), &g.Open, func() {
		ui.Scroll(c.Key("guide")).Grow(1).Shrink(1).Children(func() {
			ui.Column(c.Key("banner")).FillWidth().AlignItems(ui.Center).Padding(34, 36, 28, 36).Radius(p.RadiusLg, p.RadiusLg, 0, 0).
				LinearGradient(theme.Welcome.Banner).Children(func() {
				ui.Icon(c, icons.SailryMark).FontSize(56).TextColor(theme.Welcome.Text)
				ui.Text(c, title).FontSize(24).FontWeight(600).TextAlign(ui.Center).Margin(14, 0, 0, 0).TextColor(theme.Welcome.Text)
				ui.Text(c, i18n.T(c, "onboarding_description")).FontSize(14).TextAlign(ui.Center).Margin(6, 0, 0, 0).TextColor(theme.Welcome.Text)
			})
			ui.Column(c.Key("picker")).FillWidth().Padding(20, 28, 26, 28).Children(picker)
		})
		x := kit.IconButton(c.Key("close"), icons.Close, i18n.T(c, "close"), 32, false).TextColor(theme.Welcome.Text).OnClick(func() { g.Open = false })
		x.Absolute().Top(12).Right(12)
	})
	if panel != (ui.Element{}) {
		panel.Padding(0).Gap(0).Width(min(Width, max(w-48, 0))).MaxHeight(h*0.85).Border(1, p.Border).Radius(p.RadiusLg).Label(title)
	}
	if !g.Open {
		g.Seen = true
	}
}
