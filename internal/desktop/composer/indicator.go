package composer

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// Indicator is a progress ring of the toolbar, as a package declares one
// (plugin ui Indicator; plugins/contributions/controls/indicator.rs): the
// statistics package's context usage, directly left of the model chip.
type Indicator struct {
	ID string
	// Percent is the part of the ring filled, 0 to 100. An unmeasured value
	// is 0: the ring shows its track alone.
	Percent float64
	// Loading sweeps the ring instead of filling it: a compaction runs.
	Loading bool
	// Tone colors the ring: Caution and Negative are the warning and danger
	// colors, any other tone the muted text color.
	Tone kit.Tone
	// Hint is the tooltip and the accessible name, in the package's words;
	// "" while the value is unknown, which shows no tooltip.
	Hint string
	// Start puts it among the toolbar's leading controls (align start), not
	// before the model chip.
	Start bool
	// Panel builds the popover of an indicator with a handler, which makes
	// the ring a button: Open says whether it shows, Toggle asks to show or
	// hide it, Disabled keeps it from being asked. Without a Panel the ring
	// takes no clicks.
	Panel    func(c *ui.Context)
	Open     bool
	Toggle   func(open bool)
	Disabled bool
}

// ringInset is the room on each side of a ring (kit.Ring, 15 across).
const ringInset = 4

// indicators builds the toolbar's rings on one side, in the order given.
func (s *State) indicators(c *ui.Context, ctx *Context, start bool) {
	for i := range ctx.Indicators {
		if in := &ctx.Indicators[i]; in.Start == start {
			shown := *in
			shown.Loading = shown.Loading && !ctx.Activity.Unsynced
			s.indicator(c.Key("indicator/"+in.ID), &shown)
		}
	}
}

// indicator is one ring: 4 on each side of it and its hint as the tooltip.
// One with a panel is a small round ghost button whose popover opens over
// its end; the others are not pressable. The caller's context carries the
// ring's key, so the root element takes c as it is: keying it again would
// give every ring the same key.
func (s *State) indicator(c *ui.Context, in *Indicator) {
	if in.Panel == nil {
		box := ui.Row(c).Shrink(0).PaddingX(ringInset).AlignItems(ui.Center)
		kit.Tooltip(c, box, in.Hint)
		box.Children(func() { kit.Ring(c, in.Percent, in.Loading, in.Tone, in.Hint) })
		return
	}
	open := in.Open
	b := kit.GhostLook(c, ui.ButtonBase(c), open).Label(in.Hint).Height(24).PaddingX(ringInset).Radius(12).
		Shrink(0).Disabled(in.Disabled)
	if !open {
		kit.Tooltip(c, b, in.Hint)
	}
	b.HasPopup(ui.PopupDialog).Expanded(open).Children(func() { kit.Ring(c, in.Percent, in.Loading, in.Tone, "") }).
		OnClick(func() {
			if in.Toggle != nil {
				in.Toggle(!open)
			}
		})
	panel := kit.Popover(c, b, &open, func() { in.Panel(c) })
	if panel != (ui.Element{}) {
		panel.AttachTo(b, ui.AnchorTopEnd, ui.AnchorBottomEnd).Margin(0, 0, 8, 0)
	}
	// Kit closes the popover on an outside click or Escape: the owner hears it.
	if in.Open && !open && in.Toggle != nil {
		in.Toggle(false)
	}
}
