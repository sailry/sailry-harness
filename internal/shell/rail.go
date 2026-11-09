package shell

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
	"github.com/sailry/sailry-go/internal/transfers"
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
func (a *App) selectedFeature() string { return a.feature() }

// openFeature opens a feature's page in the main column: the
// conversation, Files and Git over the selected project, Activity and
// Market (shell/rail/features.rs: every feature is a Page destination).
func (a *App) openFeature(c *ui.Context, id string) {
	if p, ok := pageFeatures[id]; ok {
		a.Navigate(p)
		return
	}
	c.Toast(i18n.T(c, "preview_notice"))
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
		if transfers.Visible(a.installs) {
			transfers.Button(c, a.installs, &a.installsOpen, func(transfers.Entry) { c.Toast(i18n.T(c, "preview_notice")) })
		}
		a.hostPicker(c)
		kit.IconButtonHint(c.Key("search"), icons.Search, i18n.T(c, "search"), a.hint("app.search"), 36, a.search.open).
			OnClick(func() { a.openSearch() })
		kit.IconButtonHint(c.Key("settings"), icons.Settings, i18n.T(c, "settings"), a.hint("app.settings"), 36, a.Page == SettingsPage).
			OnClick(func() { a.Page = SettingsPage })
	})
}

// featureDrag is the value a More row drags.
type featureDrag struct{ id string }

// moreButton is More (shell/rail/more.rs): every feature in a popover to
// its right, each opening it, with a pin that keeps it on the
// rail; rows drag to reorder, Market staying last.
func (a *App) moreButton(c *ui.Context) {
	p := theme.Of(c)
	label := i18n.T(c, "features_all")
	b := kit.IconButton(c.Key("more"), icons.Ellipsis, label, 36, a.more).OnClick(func() { a.more = !a.more })
	b.HasPopup(ui.PopupMenu).Expanded(a.more)
	panel := kit.Popover(c, b, &a.more, func() {
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
				dragging := false
				if !f.Fixed && !f.Last {
					row.Drag(featureDrag{f.ID})
					dragging = row.Dragging()
				}
				if dragging {
					// The copy following the pointer is this row: shown as
					// the reference's selected-item preview, dimmed in place.
					row.Background(p.Hover).Opacity(0.5)
				}
				edge, over := featureEdge(row, f)
				if v, ok := ui.Drop[featureDrag](row); ok && over {
					a.features = MoveFeature(a.features, v.id, f.ID, edge)
				}
				row.Children(func() {
					if v, ok := ui.DragOver[featureDrag](row); ok && over && v.id != f.ID {
						featureDropLine(c, p, edge)
					}
					ui.Icon(c, f.Icon).FontSize(16)
					ui.Text(c, name).FontSize(14).SingleLine().Grow(1).Shrink(1)
					if f.Fixed || dragging {
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
	if panel != (ui.Element{}) {
		panel.AttachTo(b, ui.AnchorRight, ui.AnchorLeft).MarginStart(8).MinWidth(240).MaxHeight(480)
	}
}

// featureEdge is where a row takes a dragged feature: after it when the
// pointer is in its lower half, never after Market and never before the
// fixed Conversation (shell/rail/more.rs). over is false where nothing
// would go.
func featureEdge(row ui.Element, f Feature) (after, over bool) {
	_, y, in := row.PointerPosition()
	if !in {
		return false, false
	}
	after = !f.Last && y >= row.Bounds().H/2
	return after, after || !f.Fixed
}

// featureDropLine is the insertion line on a row's edge: 4 DIPs of solid
// primary with a dot at its start, centered on the gap between rows.
func featureDropLine(c *ui.Context, p *theme.Palette, after bool) {
	line := ui.Box(c.Key("drop")).Absolute().Left(0).Right(0).Height(4).Radius(2).Background(p.Primary).PassThrough()
	if after {
		line.Bottom(-2.5)
	} else {
		line.Top(-2.5)
	}
	line.Children(func() {
		ui.Box(c.Key("dot")).Absolute().Left(0).Top(-2).Size(8, 8).Radius(4).Background(p.Primary).PassThrough()
	})
}

// hostPicker is the host picker (shell/rail/hosts.rs): the hosts with a check
// on this one, and Add host; a dot when a host has an update, which the
// host's page offers. Its right-click menu is the system's.
func (a *App) hostPicker(c *ui.Context) {
	p := theme.Of(c)
	update := false
	for _, h := range a.Data.Hosts {
		update = update || h.Update
	}
	b := kit.MenuButton(c.Key("hosts"), "hosts", func(b ui.Element) {
		kit.IconLook(c, b, icons.Monitor, i18n.T(c, "hosts"), "", 36, a.Page == HostPage)
	}, func(m *kit.Menu) {
		for _, h := range a.Data.Hosts {
			// No trailing Update: the host's page offers it, and the
			// button's dot tells one is ready.
			if m.Item(i18n.T(c, h.Name)).Checked(h.ID == a.Host).Chosen() {
				a.openHost(h.ID, false)
			}
		}
		m.Separator()
		if m.Item(i18n.T(c, "host_add")).Icon(icons.Plus).Chosen() {
			a.addHost()
		}
	}, kit.MenuOptions{At: ui.AnchorRight, Self: ui.AnchorLeft, MaxHeight: 480})
	b.ContextMenu(func(m *ui.Menu) {
		if m.Item(i18n.T(c, "workspace_host")).Chosen() {
			a.openHost(a.Host, false)
		}
		if m.Item(i18n.T(c, "connection_action_edit")).Chosen() {
			c.Toast(i18n.T(c, "preview_notice"))
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
