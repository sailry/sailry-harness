// Package onboarding is the welcome guide (sailry-public onboarding.rs):
// the first launch's dialog with the brand banner, the providers' picker
// and the diagnostics note. See docs/onboarding.md.
package onboarding

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

// Width is the guide's width (onboarding.rs WIDTH), less 48 in a narrower
// window.
const Width = 560

// Guide is the welcome guide's state.
type Guide struct {
	Open bool
	// Seen is whether the guide was dismissed once (preferences.rs
	// welcome_seen): it opens at launch while false.
	Seen bool
}

// Actions are what the guide hands to the window.
type Actions struct {
	// Picker draws the providers' picker (settings.ProviderPicker), whose
	// choice continues in Settings › Providers.
	Picker func()
	// Settings opens Settings › General, where diagnostics are.
	Settings func()
}

// View is the guide (onboarding.rs Guide): a dialog 560 wide and 85% of
// the window high at most, without its own padding or close button, which
// a click outside does not close. The banner, 34 over 28 under, holds the
// white mark, the title and the description over the brand gradient, with
// a white close at its top right; the picker, then the diagnostics note
// and Settings. Closing it in any way marks it seen.
func View(c *ui.Context, g *Guide, act Actions) {
	if !g.Open {
		return
	}
	p := theme.Of(c)
	w, h := c.Size()
	title := i18n.T(c, "onboarding_title")
	panel := kit.Dialog(c.Key("welcome"), &g.Open, func() {
		ui.Scroll(c.Key("guide")).Grow(1).Shrink(1).Children(func() {
			ui.Column(c.Key("banner")).FillWidth().AlignItems(ui.Center).Padding(34, 36, 28, 36).Radius(p.RadiusLg, p.RadiusLg, 0, 0).
				LinearGradient(theme.Welcome.Banner).Children(func() {
				ui.Icon(c, icons.SailryMark).FontSize(56).TextColor(theme.Welcome.Text)
				ui.Text(c, title).FontSize(24).FontWeight(600).TextAlign(ui.Center).Margin(14, 0, 0, 0).TextColor(theme.Welcome.Text)
				ui.Text(c, i18n.T(c, "onboarding_description")).FontSize(14).TextAlign(ui.Center).Margin(6, 0, 0, 0).TextColor(theme.Welcome.Text)
			})
			ui.Column(c.Key("picker")).FillWidth().Padding(20, 28, 26, 28).Children(act.Picker)
			ui.Column(c.Key("privacy")).FillWidth().Padding(0, 36, 24, 36).Gap(8).AlignItems(ui.Center).Children(func() {
				ui.Text(c, i18n.T(c, "telemetry_welcome")).FontSize(14).TextAlign(ui.Center).TextColor(p.MutedForeground)
				label := i18n.T(c, "settings")
				kit.GhostLook(c, ui.ButtonBase(c.Key("settings")).Label(label), false).Height(28).PaddingX(8).
					Children(func() { ui.Text(c, label).FontSize(14) }).
					OnClick(func() {
						g.Open = false
						act.Settings()
					})
			})
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
