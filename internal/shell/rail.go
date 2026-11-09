package shell

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/panel"
	"github.com/sailry/sailry-go/internal/theme"
)

// Feature is a rail entry (shell/rail/features.rs).
type Feature struct {
	ID    string
	Icon  *ui.SVG
	Label string // i18n key
	Fixed bool   // always on the rail, never moved (Conversation)
	Last  bool   // always last (Market)
}

// Features are the rail's features in their default order: Conversation,
// Activity, Files, Git, Market.
var Features = []Feature{
	{ID: "page:conversation", Icon: icons.Chat, Label: "shell_sessions", Fixed: true},
	{ID: "page:activity", Icon: icons.Grid, Label: "activity"},
	{ID: "page:files", Icon: icons.Folder, Label: "files"},
	{ID: "page:git", Icon: icons.Network, Label: "git"},
	{ID: "page:settings_plugins", Icon: icons.Puzzle, Label: "market_title", Last: true},
}

func featureOf(id string) (Feature, bool) {
	for _, f := range Features {
		if f.ID == id {
			return f, true
		}
	}
	return Feature{}, false
}

// MoveFeature moves source before or after target in the rail's order
// (shell/rail/order.rs): the fixed Conversation and the last Market do not
// move, and nothing goes after Market.
func MoveFeature(order []string, source, target string, after bool) []string {
	src, _ := featureOf(source)
	dst, _ := featureOf(target)
	if source == target || src.Fixed || src.Last || dst.Fixed && !after {
		return order
	}
	if dst.Last {
		after = false
	}
	out := slices.DeleteFunc(slices.Clone(order), func(id string) bool { return id == source })
	at := slices.Index(out, target)
	if at < 0 {
		return order
	}
	if after {
		at++
	}
	return slices.Insert(out, at, source)
}

func (a *App) unpinnedList() []string {
	var out []string
	for _, id := range a.features {
		if a.unpinned[id] {
			out = append(out, id)
		}
	}
	return out
}

// selectedFeature is the feature the page shows.
func (a *App) selectedFeature() string {
	if a.Page == ConversationPage {
		return "page:conversation"
	}
	return ""
}

// openFeature opens a feature: the conversation, or Files and Git in the
// right panel (as the reference does with a live host). Activity and
// Market are later phases' pages.
func (a *App) openFeature(c *ui.Context, id string) {
	switch id {
	case "page:conversation":
		a.Page = ConversationPage
	case "page:files":
		a.openPanel(panel.Files)
	case "page:git":
		a.openPanel(panel.Changes)
	default:
		c.Toast(i18n.T(c, "preview_notice"))
	}
}

// rail is the column of features (shell/rail.rs): 36 DIP ghost buttons,
// the pinned features in their order and More at the top; notifications,
// hosts, search and settings at the bottom.
func (a *App) rail(c *ui.Context) {
	ui.Column(c.Key("rail")).Width(RailWidth).Shrink(0).Padding(8, 0).Gap(8).AlignItems(ui.Center).Children(func() {
		for _, id := range a.features {
			f, ok := featureOf(id)
			if !ok || a.unpinned[id] {
				continue
			}
			label := i18n.T(c, f.Label)
			kit.IconButton(c.Key(f.ID), f.Icon, label, 36, a.selectedFeature() == f.ID).OnClick(func() { a.openFeature(c, f.ID) })
		}
		a.moreButton(c)
		ui.Spacer(c)
		a.bell(c)
		a.hosts(c)
		kit.IconButtonHint(c.Key("search"), icons.Search, i18n.T(c, "search"), hint("app.search"), 36, a.search.open).
			OnClick(func() { a.openSearch() })
		kit.IconButtonHint(c.Key("settings"), icons.Settings, i18n.T(c, "settings"), hint("app.settings"), 36, a.Page == SettingsPage).
			OnClick(func() { a.Page = SettingsPage })
	})
}

// featureDrag is the value a More row drags.
type featureDrag struct{ id string }

// moreButton is More (shell/rail/more.rs): every feature in a frosted
// popover to its right, each opening it, with a pin that keeps it on the
// rail; rows drag to reorder, Market staying last.
func (a *App) moreButton(c *ui.Context) {
	p := theme.Of(c)
	label := i18n.T(c, "features_all")
	b := kit.IconButton(c.Key("more"), icons.Ellipsis, label, 36, a.more).OnClick(func() { a.more = !a.more })
	b.HasPopup(ui.PopupMenu).Expanded(a.more)
	ui.PopoverBase(c, b, &a.more, func(panel ui.Element) {
		kit.Floating(c, panel).AttachTo(b, ui.AnchorRight, ui.AnchorLeft).MarginStart(8).MinWidth(240).MaxHeight(480).Padding(4).Gap(1)
		ui.Scroll(c.Key("features")).Gap(1).Children(func() {
			for _, id := range a.features {
				f, ok := featureOf(id)
				if !ok {
					continue
				}
				name := i18n.T(c, f.Label)
				row := ui.ButtonBase(c.Key(f.ID)).Label(name).Height(36).PaddingX(8).Gap(8).Radius(p.Radius).Cursor(ui.CursorPointer).
					TextColor(p.PopoverForeground)
				hovered := row.Hovered()
				if hovered {
					row.Background(p.Hover)
				}
				if !f.Fixed && !f.Last {
					row.Drag(featureDrag{f.ID})
				}
				if v, over := ui.DragOver[featureDrag](row); over && v.id != f.ID {
					_, y, _ := row.PointerPosition()
					// The drop line: 4 DIPs of primary with a dot at its start.
					line := ui.Box(c.Key("drop")).Absolute().Left(4).Right(4).Height(4).Radius(2).Background(p.Primary).PassThrough()
					if y >= row.Bounds().H/2 && !f.Last {
						line.Bottom(-2)
					} else {
						line.Top(-2)
					}
				}
				if v, ok := ui.Drop[featureDrag](row); ok {
					_, y, _ := row.PointerPosition()
					a.features = MoveFeature(a.features, v.id, f.ID, y >= row.Bounds().H/2)
				}
				row.Children(func() {
					ui.Icon(c, f.Icon).FontSize(16)
					ui.Text(c, name).FontSize(14).SingleLine().Grow(1).Shrink(1)
					if f.Fixed {
						return
					}
					pinned := !a.unpinned[f.ID]
					pin := i18n.T(c, "feature_pin")
					if pinned {
						pin = i18n.T(c, "feature_unpin")
					}
					pb := kit.IconButton(c.Key("pin"), icons.Pin, pin, 20, false).OnClick(func() { a.unpinned[f.ID] = pinned })
					switch {
					case pinned:
						pb.TextColor(p.Foreground)
					case !hovered && !pb.FocusVisible():
						pb.Opacity(0).TextColor(p.MutedForeground)
					default:
						pb.TextColor(p.MutedForeground)
					}
				}).OnClick(func() {
					a.more = false
					a.openFeature(c, f.ID)
				})
			}
		})
	})
}

// hosts is the host picker (shell/rail/hosts.rs): the hosts with a check
// on this one, Update where one is ready, and Add host; a dot when a host
// has an update. Its right-click menu is the system's.
func (a *App) hosts(c *ui.Context) {
	p := theme.Of(c)
	update := false
	for _, h := range a.Data.Hosts {
		update = update || h.Update
	}
	b := kit.IconMenu(c, "hosts", icons.Monitor, i18n.T(c, "hosts"), 36, false, func(panel, t ui.Element) {
		panel.AttachTo(t, ui.AnchorRight, ui.AnchorLeft).MarginStart(8)
	}, func(m *kit.Menu) {
		for _, h := range a.Data.Hosts {
			hint := ""
			if h.Update {
				hint = i18n.T(c, "sidebar_host_update")
			}
			if m.Item(kit.Item{Label: i18n.T(c, h.Name), Icon: icons.Cpu, Checked: h.Local, Hint: hint}) {
				c.Toast(i18n.T(c, "preview_notice"))
			}
		}
		m.Separator()
		if m.Item(kit.Item{Label: i18n.T(c, "host_add"), Icon: icons.Plus}) {
			c.Toast(i18n.T(c, "preview_notice"))
		}
	})
	b.ContextMenu(func(m *ui.Menu) {
		for _, key := range []string{"workspace_host", "connection_action_edit"} {
			if m.Item(i18n.T(c, key)).Chosen() {
				c.Toast(i18n.T(c, "preview_notice"))
			}
		}
		m.Separator()
		if m.Item(i18n.T(c, "project_add")).Chosen() {
			a.Sidebar.OpenEditor(nil)
		}
	})
	if update {
		b.DrawOver(func(pt *ui.Painter, r ui.Rect) {
			pt.Fill(ui.Rect{X: r.X + r.W - 12, Y: r.Y + 6, W: 6, H: 6}, p.Primary, 3)
		})
	}
}
