package kit

import (
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/theme"
)

// Empty is a list's or a panel's empty state (empty_state.rs message):
// the icon in a 48 DIP round muted tile, the title in the muted color at
// normal weight, centered, an optional description and action (nil for
// none).
func Empty(c *ui.Context, icon *ui.SVG, title, description string, action func()) ui.Element {
	p := theme.Of(c)
	return ui.Column(c.Key("empty")).Padding(40, 16).Gap(12).AlignItems(ui.Center).Children(func() {
		if icon != nil {
			EmptyMedia(c, icon)
		}
		ui.Text(c, title).FontSize(14).TextAlign(ui.Center).TextColor(p.MutedForeground)
		if description != "" {
			ui.Text(c, description).FontSize(14).TextAlign(ui.Center).TextColor(p.Description())
		}
		if action != nil {
			ui.Row(c).Padding(4, 0, 0, 0).Children(action)
		}
	})
}

// EmptyMedia is an empty state's icon (Kit's EmptyMedia, Icon variant): 24
// DIP in a 48 DIP round tile of the muted fill.
func EmptyMedia(c *ui.Context, icon *ui.SVG) {
	p := theme.Of(c)
	ui.Row(c.Key("empty-media")).Size(48, 48).Radius(24).Background(p.Muted).Center().Shrink(0).Children(func() {
		ui.Icon(c, icon).FontSize(24).TextColor(p.MutedForeground)
	})
}

// List is a card of n rows, or its empty state when n is 0.
func List(c *ui.Context, n int, row func(i int), empty func()) {
	if n == 0 {
		p := theme.Of(c)
		ui.Column(c).Border(1, p.Border).Radius(p.RadiusLg).Background(p.Surface).Children(empty)
		return
	}
	Card(c, func() {
		for i := range n {
			row(i)
		}
	})
}

// Toolbar is a page's row over its list: a title with a count, a search
// field, and actions after it. count < 0 hides it; query nil hides the
// search; actions may be nil.
func Toolbar(c *ui.Context, title string, count int, query *string, placeholder string, actions func()) {
	p := theme.Of(c)
	ui.Row(c.Key("toolbar")).MinHeight(32).Gap(8).Children(func() {
		ui.Row(c).AlignItems(ui.Baseline).Gap(6).Grow(1).Shrink(1).MinWidth(0).Children(func() {
			ui.Text(c, title).FontSize(14).FontWeight(600).SingleLine()
			if count >= 0 {
				ui.Textf(c, "%d", count).FontSize(14).TextColor(p.MutedForeground)
			}
		})
		if query != nil {
			ui.SearchField(c.Key("search"), query).Placeholder(placeholder).Label(placeholder).Width(220)
		}
		if actions != nil {
			ui.Row(c).Shrink(0).Gap(8).Children(actions)
		}
	})
}

// Outcome is what a toast tells of an operation.
type Outcome int

const (
	Done      Outcome = iota // it succeeded
	Failed                   // it failed: the draft stays
	Conflict                 // someone changed it first
	Uncertain                // its result is unknown, as after a timeout
)

// Toast tells an operation's outcome; failures and conflicts stay until
// closed, so they are read. Pages show outcomes only as toasts.
func Toast(c *ui.Context, outcome Outcome, message string) {
	t := ui.Toast{Title: message}
	switch outcome {
	case Done:
		t.Type = "success"
	case Failed:
		t.Type, t.Timeout = "error", -1
	case Conflict:
		t.Type, t.Timeout = "warning", -1
	case Uncertain:
		t.Type, t.Timeout = "warning", 8*time.Second
	}
	c.AddToast(t)
}
