package shell

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/transfers"
)

// Feature is a rail entry.
type Feature struct {
	ID            string
	Icon          *ui.SVG
	Label         string // i18n key
	Fixed         bool   // always on the rail, never moved (Conversation)
	Last          bool   // always last (Market)
	DefaultPinned bool
}

func (a *App) lookupFeature(id string) (Feature, bool) {
	if page := a.pageForFeature(id); page != nil {
		return page.Feature, true
	}
	feature, exists := a.pluginFeatures[id]
	return feature, exists
}

func (a *App) SetPluginFeatures(features []Feature) {
	if a.Preview {
		return
	}
	a.pluginFeatures = make(map[string]Feature, len(features))
	for _, feature := range features {
		if feature.ID == "" || a.pageForFeature(feature.ID) != nil {
			continue
		}
		a.pluginFeatures[feature.ID] = feature
		if _, known := a.unpinned[feature.ID]; !known {
			a.unpinned[feature.ID] = !feature.DefaultPinned && !feature.Fixed
		}
	}
	position := 1
	for _, feature := range features {
		if _, available := a.pluginFeatures[feature.ID]; !available || slices.Contains(a.features, feature.ID) {
			continue
		}
		position = min(position, len(a.features))
		a.features = slices.Insert(a.features, position, feature.ID)
		position++
	}
	a.features = a.mergeFeatureOrder(a.features)
	if a.Page == PluginPage {
		if _, available := a.pluginFeatures[a.pluginPage]; !available {
			a.Navigate(ConversationPage)
		}
	}
}

// moveFeature moves a destination relative to another visible rail entry.
func (a *App) moveFeature(source, target string, after bool) {
	src, sourceAvailable := a.lookupFeature(source)
	dst, targetAvailable := a.lookupFeature(target)
	if !sourceAvailable || !targetAvailable || source == target || src.Fixed || src.Last || dst.Fixed && !after {
		return
	}
	if dst.Last {
		after = false
	}
	out := slices.DeleteFunc(slices.Clone(a.features), func(id string) bool { return id == source })
	at := slices.Index(out, target)
	if at < 0 {
		return
	}
	if after {
		at++
	}
	a.features = a.mergeFeatureOrder(slices.Insert(out, at, source))
}

func (a *App) unpinnedList() []string {
	var unordered, out []string
	for id, unpinned := range a.unpinned {
		if unpinned && !slices.Contains(a.features, id) {
			unordered = append(unordered, id)
		}
	}
	for _, id := range a.features {
		if a.unpinned[id] {
			out = append(out, id)
		}
	}
	slices.Sort(unordered)
	return append(out, unordered...)
}

func (a *App) PluginFeature() string {
	if a.Page == PluginPage {
		return a.pluginPage
	}
	return ""
}

// openFeature opens a registered rail destination in the main column.
func (a *App) openFeature(c *ui.Context, id string) {
	if p := a.pageForFeature(id); p != nil {
		a.Navigate(p.Page)
		return
	}
	if _, exists := a.pluginFeatures[id]; exists {
		if a.Hooks.SelectPlugin != nil && a.Hooks.SelectPlugin(id) {
			a.Navigate(PluginPage)
			a.pluginPage = id
		}
		return
	}
	a.notice(c)
}

// rail is the column of features: 36 DIP ghost buttons, the pinned features
// in their order and More at the top; notifications, search and
// settings at the bottom. The frame gives its width in content DIPs.
func (a *App) rail(c *ui.Context, width float32) {
	ui.Column(c.Key("rail")).Width(width).Shrink(0).Padding(8, 0).Gap(8).AlignItems(ui.Center).Children(func() {
		for _, id := range a.features {
			f, ok := a.lookupFeature(id)
			if !ok || a.unpinned[id] {
				continue
			}
			label := i18n.T(c, f.Label)
			railChosen(c, kit.IconButton(c.Key(f.ID), f.Icon, label, RailButton, a.feature() == f.ID), a.feature() == f.ID).OnClick(func() { a.openFeature(c, f.ID) })
		}
		a.moreButton(c)
		ui.Spacer(c)
		a.bell(c)
		if transfers.Visible(a.installs) {
			// Only the preview plays a retry itself; a live queue retries
			// through its RetryAction, and offers no Retry without one.
			var retry func(transfers.Entry)
			if a.Preview {
				retry = func(transfers.Entry) { a.notice(c) }
			}
			transfers.Button(c, a.installs, &a.installsOpen, retry)
		}
		railChosen(c, kit.IconButtonHint(c.Key("search"), icons.Search, i18n.T(c, "search"), a.hint("app.search"), RailButton, a.search.open), a.search.open).
			OnClick(func() { a.openSearch() })
		settings := railChosen(c, kit.IconButtonHint(c.Key("settings"), icons.Settings, i18n.T(c, "settings"), a.hint("app.settings"), RailButton, a.Page == SettingsPage), a.Page == SettingsPage).
			OnClick(a.ToggleSettings)
		// A found update marks Settings, as unread notices mark the bell,
		// until About shows it.
		if a.Settings.Updater.Attention() {
			p := theme.Of(c)
			settings.DrawOver(func(pt *ui.Painter, r ui.Rect) {
				pt.Fill(ui.Rect{X: r.X + r.W - 12, Y: r.Y + 6, W: 6, H: 6}, p.Danger, 3)
			})
		}
	})
}

// featureDrag is the value a More row drags.
type featureDrag struct{ id string }

// moreButton is More: every feature in a popover to its right, each opening
// it, with a pin that keeps it on the rail; rows drag to reorder, Market
// staying last.
func (a *App) moreButton(c *ui.Context) {
	p := theme.Of(c)
	label := i18n.T(c, "features_all")
	b := railChosen(c, kit.IconButton(c.Key("more"), icons.Ellipsis, label, RailButton, a.more), a.more).OnClick(func() { a.more = !a.more })
	b.HasPopup(ui.PopupMenu).Expanded(a.more)
	panel := kit.Popover(c, b, &a.more, func() {
		ui.Scroll(c.Key("features")).Gap(1).Children(func() {
			for _, id := range a.features {
				f, ok := a.lookupFeature(id)
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
					a.moveFeature(v.id, f.ID, edge)
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
// pointer is in its lower half, never after Market and never before the fixed
// Conversation. over is false where nothing would go.
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

// railChosen marks b, the rail's chosen destination, in the package's
// navigation accent (sidebar.accent.*), as the sidebar marks its chosen row;
// without one it keeps Kit's selected look.
func railChosen(c *ui.Context, b ui.Element, chosen bool) ui.Element {
	if p := theme.Of(c); chosen && p.SidebarAccent.A > 0 {
		b.Background(p.SidebarAccent).TextColor(p.SidebarAccentForeground)
	}
	return b
}
