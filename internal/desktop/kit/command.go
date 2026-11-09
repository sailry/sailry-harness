package kit

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// CommandItem is a row of a Command dialog.
type CommandItem struct {
	ID       string
	Label    string
	Detail   string // muted text at the row's end, as a commit or a count
	Icon     *ui.SVG
	Group    string // its heading; rows of a group are consecutive
	Folder   bool   // it opens a list of its own: a chevron
	Checked  bool   // the current one: a check
	Disabled bool
}

// CommandProps are a Command dialog's texts and hooks.
type CommandProps struct {
	Placeholder string // the query's placeholder and label
	Empty       string // shown when no row matches
	Width       float32
	// Back is Backspace on an empty query and Escape on an empty one: it
	// reports whether it went back (a folder to its root), else Escape
	// closes.
	Back func() bool
}

// Command is the searchable command dialog: the query over a divider, then
// the rows, 36 DIPs each, under 28 DIP headings with a divider between
// groups. Up and Down move, Enter or a click chooses; Escape clears the
// query, goes back, then closes. The caller keeps the query and the
// highlighted row, filters items by the query, and is told the index in items
// of the row chosen.
func Command(c *ui.Context, key any, open *bool, query *string, sel *int, items []CommandItem, props CommandProps, choose func(i int)) {
	if !*open {
		return
	}
	p := theme.Of(c)
	n := len(items)
	if *sel >= n || *sel < 0 {
		*sel = 0
	}
	pick := func(i int) {
		if i >= 0 && i < n && !items[i].Disabled {
			choose(i)
		}
	}
	w, _ := c.Size()
	width := props.Width
	if width == 0 {
		width = 480
	}
	Dialog(c.Key(key), open, func() {
		ui.Row(c.Key("query")).Height(45).PaddingX(12).Gap(8).Shrink(0).BorderWidth(0, 0, 1, 0).BorderColor(p.Border).Children(func() {
			ui.Icon(c, icons.Search).FontSize(16).TextColor(p.MutedForeground)
			in := ui.TextInputBase(c.Key("input"), query).Grow(1).FontSize(14).TextColor(p.PopoverForeground).
				Placeholder(props.Placeholder).Label(props.Placeholder).AutoFocus()
			if in.Changed() {
				*sel = 0
			}
			if in.Shortcut(0, ui.KeyDown) && n > 0 {
				*sel = (*sel + 1) % n
			}
			if in.Shortcut(0, ui.KeyUp) && n > 0 {
				*sel = (*sel - 1 + n) % n
			}
			in.OnSubmit(func() { pick(*sel) })
			if in.Shortcut(0, ui.KeyEscape) {
				switch {
				case *query != "":
					*query, *sel = "", 0
				case props.Back != nil && props.Back():
					*sel = 0
				default:
					*open = false
				}
			}
			// The input takes Backspace even when empty, so a Shortcut
			// never sees it: its key handler does.
			in.OnTextKey(func(k ui.TextKey) bool {
				if k.Key != ui.KeyBackspace || k.Mods != 0 || k.Text() != "" || props.Back == nil {
					return false
				}
				if props.Back() {
					*sel = 0
					return true
				}
				return false
			})
		})
		ui.Scroll(c.Key("rows")).MaxHeight(344).Padding(8).Children(func() {
			if n == 0 {
				ui.Text(c, props.Empty).FontSize(14).Padding(12).TextColor(p.MutedForeground)
				return
			}
			for i := range items {
				it := &items[i]
				if i == 0 || it.Group != items[i-1].Group {
					if i > 0 {
						ui.Box(c.Key(fmt.Sprint("divider-", i))).FillWidth().Height(1).Shrink(0).Margin(4, 0).Background(p.Border)
					}
					if it.Group != "" {
						ui.Text(c, it.Group).FontSize(14).Height(28).PaddingX(8).Shrink(0).TextColor(p.MutedForeground)
					}
				}
				id := it.ID
				if id == "" {
					id = it.Label
				}
				row := ui.ButtonBase(c.Key(fmt.Sprint(i, "-", id))).Label(it.Label).Height(36).PaddingX(8).Gap(8).Radius(p.Radius).Shrink(0).
					Disabled(it.Disabled).TextColor(p.PopoverForeground)
				if it.Disabled {
					row.Opacity(0.5)
				}
				if row.Hovered() && !it.Disabled {
					*sel = i
				}
				if i == *sel {
					row.Background(p.Hover).TextColor(p.HoverForeground).ScrollIntoView()
				}
				row.Children(func() {
					if it.Icon != nil {
						ui.Icon(c, it.Icon).FontSize(16).Shrink(0)
					}
					ui.Text(c, it.Label).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
					if it.Detail != "" {
						ui.Text(c, it.Detail).FontSize(12).SingleLine().Shrink(0).TextColor(p.MutedForeground)
					}
					switch {
					case it.Folder:
						ui.Icon(c, icons.ChevronRight).FontSize(14).TextColor(p.MutedForeground)
					case it.Checked:
						ui.Icon(c, icons.Check).FontSize(14)
					}
				}).OnClick(func() { pick(i) })
			}
		})
	}).Width(min(width, w-40)).MaxHeightPercent(60).Padding(0).Gap(0).Label(props.Placeholder).Clip()
}
