package picker

import (
	"slices"
	"strings"

	format "github.com/egoist/mygo/i18n"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The dialog is 1040 by 680 at most and 24 DIP clear of the window's edges.
// As it narrows, the Kind column, then the Modified column, then the sidebar
// give way to the names: under sidebarFrom the title moves over the header
// bar and the locations into its More menu. The window's least size, 760 by
// 560, leaves the names and the sizes.
const (
	kindFrom     = 960
	modifiedFrom = 860
	sidebarFrom  = 780
)

// View builds the picker over the window's other dialogs, its content edge
// to edge as a file manager's window: the host's locations down the left
// under the title, and beside them the header bar, the directory's entries
// and the footer bar with the selection, the counts, Cancel and Confirm. It
// must be built after the dialog that opened it.
func (s *State) View(c *ui.Context) {
	for _, n := range s.notices {
		text := i18n.T(c, n.key, "path", n.path)
		if n.host && s.o.Host != "" {
			text += " · " + i18n.T(c, "project_host") + ": " + s.o.Host
		}
		kit.Toast(c, n.outcome, text)
	}
	s.notices = nil
	if s.dismiss != "" {
		kit.OpenMenu(c, "picker-crumb/"+s.dismiss, false)
		s.dismiss = ""
	}
	if !s.open {
		return
	}
	valid := s.valid()
	title := i18n.T(c, s.o.Title)
	s.arrange(c)
	shown := true
	p := theme.Of(c)
	w, h := c.Size()
	width, height := min(w-48, 1040), min(h-48, 680)
	wide := width >= sidebarFrom
	panel := kit.Dialog(c.Key("picker"), &shown, func() {
		ui.Row(c.Key("frame")).Width(width).Height(height).AlignItems(ui.Stretch).Children(func() {
			if wide {
				s.places(c, valid, title)
				ui.Box(c.Key("rule")).Width(1).Shrink(0).Background(p.Border)
			}
			// The frost stays the sidebar's alone: the popover color covers it
			// under the entries, so what is behind the dialog does not read
			// through their text.
			// Opaque, so nothing behind the dialog reads through the
			// entries; the sidebar alone keeps the frost.
			ui.Column(c.Key("content")).Grow(1).MinWidth(0).Background(kit.Opaque(p.Popover)).Children(func() {
				if !wide {
					ui.Row(c.Key("title")).Height(44).Shrink(0).Padding(0, 48, 0, 16).Gap(8).AlignItems(ui.Center).Children(func() {
						s.heading(c, title)
					})
				}
				s.toolbar(c, valid, wide)
				notice := ""
				switch {
				case !valid:
					notice = "directory_stale"
				case s.o.Preview:
					notice = "directory_preview"
				}
				if notice != "" {
					ui.Row(c.Key("notice")).Height(28).Shrink(0).PaddingX(16).AlignItems(ui.Center).Background(p.Muted).Children(func() {
						ui.Text(c, i18n.T(c, notice)).FontSize(12).SingleLine().TextColor(p.MutedForeground)
					})
				}
				s.entries(c, valid, width)
				// A key or a click in the entries may have shown another directory.
				s.arrange(c)
				s.footer(c, valid)
			})
		})
	}, kit.DialogOptions{Bleed: true}).Label(title)
	if valid && !s.busy {
		if panel.Shortcut(ui.Cmd, ui.KeyL) {
			s.editing = true
		}
		if panel.Shortcut(0, ui.KeyF5) {
			s.reload()
		}
		if panel.Shortcut(ui.Cmd, ui.KeyUp) {
			s.up()
		}
		if panel.Shortcut(ui.Cmd|ui.Shift, ui.KeyG) {
			s.ask()
		}
	}
	if !shown {
		s.Close()
	}
	over := s.naming.open || s.trashing.open || s.jump.open
	s.nameDialog(c)
	s.trashDialog(c)
	s.jumpDialog(c)
	// The entries take the keys back from a dialog of the picker's that
	// closes.
	if over && !s.naming.open && !s.trashing.open && !s.jump.open {
		s.refocus = true
	}
}

// heading is the dialog's title and the host it browses, "" for none.
func (s *State) heading(c *ui.Context, title string) {
	ui.Text(c, title).FontSize(15).FontWeight(600).SingleLine().Shrink(0)
	if s.o.Host != "" {
		ui.Text(c, s.o.Host).FontSize(12).SingleLine().Shrink(1).MinWidth(0).TextColor(theme.Of(c).MutedForeground)
	}
}

// group is a group of the host's locations: its i18n key, and the locations
// of the listing it holds.
type group struct {
	key   string
	items []Location
}

// groups are the locations as the sidebar lists them: the host's places, the
// directories of its registered projects, then its root and volumes; a group
// with nothing is left out. While no directory shows, the host's home is the
// one place, so a read that failed still leaves a way in. near is the path
// of the location that holds the directory shown, "" for none.
func (s *State) groups() (out []group, near string) {
	places, projects, devices := group{key: "directory_locations"}, group{key: "projects"}, group{key: "directory_devices"}
	for _, loc := range s.listing.Locations {
		if loc.Kind == "volume" || loc.Kind == "root" {
			devices.items = append(devices.items, loc)
		} else {
			places.items = append(places.items, loc)
		}
	}
	if !s.loaded {
		places.items = []Location{{Kind: "home"}}
	}
	for _, loc := range s.o.Projects {
		projects.items = append(projects.items, Location{Kind: "project", Name: loc.Name, Path: loc.Path})
	}
	var all []Location
	for _, g := range []group{places, projects, devices} {
		if len(g.items) > 0 {
			out = append(out, g)
			all = append(all, g.items...)
		}
	}
	if s.loaded {
		near, _ = Nearest(s.listing, all)
	}
	return out, near
}

// ask opens Go to folder at the directory shown, or at the one that failed
// to show.
func (s *State) ask() {
	s.jump.open, s.jump.query, s.jump.row = true, s.target, 0
}

// placeName is a location's label: its own, or its kind's.
func placeName(c *ui.Context, loc Location) string {
	if loc.Name != "" {
		return loc.Name
	}
	return i18n.T(c, "directory_"+loc.Kind)
}

// places is the sidebar: the dialog's title and its host over the host's
// locations, flush with the dialog's left edge.
func (s *State) places(c *ui.Context, valid bool, title string) {
	p := theme.Of(c)
	// The wash is the step the window's sidebar takes off its background,
	// translucent so the dialog's frost shows through it.
	ui.Column(c.Key("places")).Width(204).Shrink(0).Background(p.Muted).Children(func() {
		ui.Column(c.Key("title")).Height(52).Shrink(0).PaddingX(16).Justify(ui.Center).Children(func() { s.heading(c, title) })
		ui.Scroll(c.Key("locations")).Grow(1).MinHeight(0).Padding(0, 8, 12, 8).Gap(2).Children(func() {
			groups, near := s.groups()
			for _, g := range groups {
				ui.Text(c.Key(g.key), i18n.T(c, g.key)).FontSize(12).Padding(10, 8, 4, 8).TextColor(p.MutedForeground)
				for i, loc := range g.items {
					label := placeName(c, loc)
					place := kit.Ghost(c.Key([2]any{g.key, i}), label, s.loaded && loc.Path == near).FillWidth().Height(30).PaddingX(8).Gap(8)
					if s.loaded && loc.Path == near {
						place.Current(ui.CurrentLocation)
					}
					place.Justify(ui.Start).Disabled(s.busy || !valid).Children(func() {
						ui.Icon(c, glyphs[loc.Kind]).FontSize(15).Shrink(0)
						ui.Text(c, label).FontSize(13).SingleLine().Shrink(1).MinWidth(0)
					}).OnClick(func() { s.navigate(loc.Path, push, 0) })
				}
			}
		})
	})
}

// glyphs are the icons of the locations, by kind.
var glyphs = map[string]*ui.SVG{"home": icons.House, "desktop": icons.Monitor, "documents": icons.FileText,
	"downloads": icons.Download, "project": icons.Folder, "volume": icons.HardDrive, "root": icons.HardDrive}

// kind is an entry's kind as the table shows and sorts it.
func kind(c *ui.Context, e Entry) string {
	key, extension := KindKey(e)
	if extension != "" {
		return i18n.T(c, key, "extension", extension)
	}
	return i18n.T(c, key)
}

// arrange filters and orders the entries shown, and keeps the row of the
// selected one. The files chosen in Files mode stay chosen only while they
// show.
func (s *State) arrange(c *ui.Context) {
	s.rows = Visible(s.listing.Entries, s.Hidden, s.filter)
	Order(s.listing.Entries, s.rows, s.sort.Column, s.sort.Descending, func(e Entry) string { return kind(c, e) })
	s.row = slices.IndexFunc(s.rows, func(i int) bool { return s.listing.Entries[i].Name == s.current })
	if s.row < 0 {
		s.current = ""
	}
	if s.chosen.Len() > 0 {
		shown := make(map[string]bool, len(s.rows))
		for _, i := range s.rows {
			shown[s.listing.Entries[i].Name] = true
		}
		for name := range s.chosen.All() {
			if !shown[name] {
				s.chosen.Remove(name)
			}
		}
	}
}

// toolbar is the header bar: Back, Forward and Up, the address, the filter,
// the list and grid switch, New folder, and a menu of the rarer actions:
// the hidden-files toggle, Refresh, Go to folder and, without the sidebar,
// the host's locations. It is 52 high over a hairline; beside the sidebar it
// keeps clear of the dialog's close button. A read in flight disables
// nothing here: another navigation replaces it.
func (s *State) toolbar(c *ui.Context, valid, wide bool) {
	p := theme.Of(c)
	off := s.busy || !valid
	end := float32(12)
	if wide {
		end = 48
	}
	ui.Row(c.Key("toolbar")).FillWidth().Height(52).Shrink(0).Padding(0, end, 0, 12).Gap(4).AlignItems(ui.Center).
		BorderWidth(0, 0, 1, 0).BorderColor(p.Border).Children(func() {
		kit.IconButton(c.Key("back"), icons.ArrowLeft, i18n.T(c, "directory_back"), 28, false).
			Disabled(off || s.cursor == 0).OnClick(func() { s.step(false) })
		kit.IconButton(c.Key("forward"), icons.ArrowRight, i18n.T(c, "directory_forward"), 28, false).
			Disabled(off || s.cursor+1 >= len(s.history)).OnClick(func() { s.step(true) })
		kit.IconButton(c.Key("up"), icons.ArrowUp, i18n.T(c, "directory_up"), 28, false).
			Disabled(off || s.listing.Parent == "").OnClick(s.up)
		s.addressBar(c, off)
		ui.Row(c.Key("filter")).Width(168).Height(28).PaddingX(8).Gap(6).Radius(p.Radius).Border(1, p.Input).Shrink(0).
			AlignItems(ui.Center).Children(func() {
			ui.Icon(c, icons.Search).FontSize(14).TextColor(p.MutedForeground)
			ui.TextInputBase(c.Key("input"), &s.filter).Grow(1).FontSize(13).Placeholder(i18n.T(c, "directory_filter")).
				Label(i18n.T(c, "directory_filter")).PlaceholderColor(p.MutedForeground).Disabled(!valid)
			if s.filter != "" {
				kit.IconButton(c.Key("clear"), icons.Close, i18n.T(c, "directory_filter"), 20, false).OnClick(func() { s.filter = "" })
			}
		})
		for _, grid := range []bool{false, true} {
			icon, key := icons.List, "directory_list"
			if grid {
				icon, key = icons.Grid, "directory_grid"
			}
			kit.IconButton(c.Key(key), icon, i18n.T(c, key), 28, s.Grid == grid).Selected(s.Grid == grid).OnClick(func() {
				if s.Grid != grid {
					s.Grid = grid
					s.changed()
				}
			})
		}
		kit.IconButton(c.Key("create"), icons.FolderPlus, i18n.T(c, "directory_create"), 28, false).
			Disabled(off || !s.loaded).OnClick(func() {
			s.naming.open, s.naming.from, s.naming.value = true, "", ""
		})
		kit.MenuButton(c.Key("more"), "picker-more", func(b ui.Element) {
			kit.IconLook(c, b, icons.Ellipsis, i18n.T(c, "more"), "", 28, false)
		}, func(m *kit.Menu) {
			if m.Item(i18n.T(c, "directory_hidden")).Checked(s.Hidden).Chosen() {
				s.Hidden = !s.Hidden
				s.changed()
			}
			if m.Item(i18n.T(c, "directory_refresh")).Shortcut(kit.KeyText(0, ui.KeyF5)).Disabled(off).Chosen() {
				s.reload()
			}
			if m.Item(i18n.T(c, "directory_go_to")).Shortcut(kit.KeyText(ui.Cmd|ui.Shift, ui.KeyG)).Disabled(off).Chosen() {
				s.ask()
			}
			if wide {
				return
			}
			groups, near := s.groups()
			for _, g := range groups {
				m.Separator()
				m.Submenu(i18n.T(c, g.key), func(m *kit.Menu) {
					for _, loc := range g.items {
						if m.Item(placeName(c, loc)).Icon(glyphs[loc.Kind]).Checked(s.loaded && loc.Path == near).Disabled(off).Chosen() {
							s.navigate(loc.Path, push, 0)
						}
					}
				})
			}
		})
	})
}

func (s *State) changed() {
	if s.Changed != nil {
		s.Changed()
	}
}

// addressBar is the directory's path: its crumbs, each opening the
// directories under it, or, once clicked beside them, a field taking a path.
func (s *State) addressBar(c *ui.Context, off bool) {
	p := theme.Of(c)
	// A path bar: the crumbs sit in a filled field.
	bar := ui.Row(c.Key("address")).Grow(1).MinWidth(0).Height(28).Margin(0, 4).PaddingX(6).Gap(6).Radius(p.Radius).
		Background(p.Muted).AlignItems(ui.Center).Clip()
	if s.editing {
		bar.Border(1, p.Ring).Children(func() {
			input := ui.TextInputBase(c.Key("input"), &s.address).Grow(1).FontSize(13).Label(i18n.T(c, "directory_address")).
				Disabled(off).AutoFocus()
			if input.Submitted() && strings.TrimSpace(s.address) != "" {
				s.navigate(strings.TrimSpace(s.address), push, 0)
			}
			if bar.Shortcut(0, ui.KeyEscape) {
				s.editing, s.address = false, s.target
			}
		})
		return
	}
	// The read in flight shows at the bar's end, over whatever directory
	// still shows.
	reading := func() {
		if s.reading {
			ui.Spinner(c.Key("reading")).Size(14, 14).Shrink(0).Label(i18n.T(c, "plugins_loading"))
		}
	}
	bar.Children(func() {
		ui.Icon(c, icons.HardDrive).FontSize(16).Shrink(0).TextColor(p.MutedForeground)
		if !s.loaded {
			// The path that is read, or failed to show, takes a click to
			// change as any other does.
			ui.Text(c, s.target).FontSize(13).SingleLine().Shrink(1).MinWidth(0).TextColor(p.MutedForeground)
			ui.ButtonBase(c.Key("edit")).Label(i18n.T(c, "directory_address")).Grow(1).MinWidth(16).Height(24).
				Disabled(off).OnClick(func() { s.editing, s.address = true, s.target })
			reading()
			return
		}
		// A long path scrolls, showing its end: the directory itself.
		crumbs := Crumbs(s.listing)
		ui.ScrollHorizontal(c.Key("crumbs")).TrackScroll(&s.crumbs).Shrink(1).MinWidth(0).Gap(6).AlignItems(ui.Center).Children(func() {
			for i, crumb := range crumbs {
				if i > 0 {
					ui.Icon(c.Key([2]any{"sep", i}), icons.ChevronRight).FontSize(14).Shrink(0).TextColor(p.MutedForeground)
				}
				id := "picker-crumb/" + crumb.Path
				kit.MenuButton(c.Key([2]any{"crumb", i}), id, func(b ui.Element) {
					kit.GhostLook(c, b.Label(crumb.Path), false).Height(24).PaddingX(6).MaxWidth(160).Shrink(0).Disabled(off)
					kit.Tooltip(c, b, crumb.Path)
					b.Children(func() {
						text := ui.Text(c, crumb.Label).FontSize(13).SingleLine()
						if i == len(crumbs)-1 {
							text.FontWeight(600)
						}
					})
				}, func(m *kit.Menu) {
					list, ok := s.children[crumb.Path]
					switch {
					case !ok:
						s.loadChildren(crumb.Path)
						m.Item(i18n.T(c, "plugins_loading")).Disabled(true)
					case len(list) == 0:
						m.Item(i18n.T(c, "directory_no_matches")).Disabled(true)
					}
					for _, child := range list {
						if m.Item(child.Label).Icon(icons.Folder).Chosen() {
							s.navigate(child.Path, push, 0)
						}
					}
				}, kit.MenuOptions{MaxHeight: 320})
			}
		})
		// The rest of the bar takes a click to type a path.
		ui.ButtonBase(c.Key("edit")).Label(i18n.T(c, "directory_address")).Grow(1).MinWidth(16).Height(24).
			Disabled(off).OnClick(func() { s.editing = true })
		reading()
	})
}

// modified is an entry's modification time as the list shows it, and in full
// as its tooltip does: today's and yesterday's with the time of day, any
// other as a date, a dash for none. The frame's language writes the dates,
// and the desktop's hour cycle the times.
func (s *State) modified(c *ui.Context, e Entry) (short, full string) {
	now := c.Now()
	if s.o.Preview {
		now = CatalogNow
	}
	locale, cycle := i18n.Match(c.Locale()), c.Region().HourCycle
	at := e.Modified.In(now.Location())
	clock := func() string {
		return format.FormatDate(locale, at, format.DateOptions{Skeleton: "jm", HourCycle: cycle})
	}
	switch Day(e.Modified, now) {
	case -1:
		return "—", ""
	case 0:
		short = i18n.T(c, "directory_modified_today", "time", clock())
	case 1:
		short = i18n.T(c, "directory_modified_yesterday", "time", clock())
	default:
		short = format.FormatDate(locale, at, format.DateOptions{Date: format.MediumStyle})
	}
	return short, format.FormatDate(locale, at, format.DateOptions{Date: format.LongStyle, Time: format.MediumStyle, HourCycle: cycle})
}

// entries is the directory in the table or the grid, with its native
// right-click menus and its keys. width is the dialog's, which the table's
// columns follow.
func (s *State) entries(c *ui.Context, valid bool, width float32) {
	p := theme.Of(c)
	entries, rows := s.listing.Entries, s.rows
	off := s.busy || !valid
	create := func() { s.naming.open, s.naming.from, s.naming.value = true, "", "" }
	area := ui.Column(c.Key("entries")).Grow(1).MinHeight(0).Focusable().Busy(s.reading).ContextMenu(func(m *ui.Menu) {
		if off || !s.loaded {
			return
		}
		if m.Item(i18n.T(c, "directory_create")).Chosen() {
			create()
		}
		s.pasteMenu(c, m)
		m.Separator()
		if m.Item(i18n.T(c, "directory_refresh")).Chosen() {
			s.reload()
		}
	})
	menu := func(m *ui.Menu, row int) {
		if off || row < 0 || row >= len(rows) {
			return
		}
		e := entries[rows[row]]
		// The menu acts on its own entry, and selects it where the mode
		// lets it be selected.
		if s.selectable(e) {
			s.current = e.Name
		}
		if m.Item(i18n.T(c, "directory_open")).Disabled(!e.Dir() && s.o.Mode == Directory).Chosen() {
			s.activate(rows[row])
		}
		m.Separator()
		if m.Item(i18n.T(c, "files_copy")).Chosen() {
			s.copy(e.Name, false)
		}
		if m.Item(i18n.T(c, "files_cut")).Chosen() {
			s.copy(e.Name, true)
		}
		if m.Item(i18n.T(c, "files_rename")).Chosen() {
			s.naming.open, s.naming.from, s.naming.value = true, e.Name, e.Name
		}
		s.pasteMenu(c, m)
		if m.Item(i18n.T(c, "directory_create")).Chosen() {
			create()
		}
		m.Separator()
		if m.Item(i18n.T(c, "directory_trash")).Chosen() {
			s.trashing.open, s.trashing.path, s.trashing.label = true, s.listing.Join(e.Name), e.Name
		}
	}
	area.Children(func() {
		switch {
		case !s.loaded:
			// Nothing shows yet: the read, or why it failed, with the way
			// to try it again. The address, Go to folder and the sidebar
			// lead elsewhere.
			switch {
			case s.reading:
				ui.Row(c.Key("loading")).Grow(1).Center().Children(func() { ui.Spinner(c).Label(i18n.T(c, "plugins_loading")) })
			case s.problem != "":
				kit.Result(c.Key("failed"), kit.ResultProps{Status: kit.ResultError, Title: i18n.T(c, s.problem), SubTitle: s.target, Extra: func() {
					kit.Secondary(c.Key("retry"), i18n.T(c, "settings_retry")).Disabled(off).OnClick(s.reload)
				}}).Grow(1).Justify(ui.Center)
			}
			if s.refocus {
				s.refocus = false
				area.Focus()
			}
			return
		case len(rows) == 0:
			key := "files_directory_empty"
			if s.filter != "" {
				key = "directory_no_matches"
			}
			kit.Result(c.Key("empty"), kit.ResultProps{Icon: icons.FolderOpen, Title: i18n.T(c, key)}).Grow(1).Justify(ui.Center)
			if s.refocus {
				s.refocus = false
				area.Focus()
			}
			return
		}
		var view ui.Element
		label := func(i int) string { return entries[rows[i]].Name }
		key := func(i int) any { return entries[rows[i]].Name }
		// Files mode chooses several entries, by their names; the other modes
		// one, the row the list holds.
		var chosen ui.Selector
		if s.o.Mode == Files {
			chosen = &s.chosen
		}
		// A chosen row is filled with the primary color, so its tile and
		// details take the row's own foreground.
		on := func(i int) bool {
			if s.o.Mode == Files {
				return s.chosen.Has(entries[rows[i]].Name)
			}
			return i == s.row
		}
		detail := func(text string, selected bool) ui.Element {
			el := ui.Text(c, text).FontSize(13).SingleLine()
			if !selected {
				el.TextColor(p.MutedForeground)
			}
			return el
		}
		// Hidden entries are dimmed, and so is what the mode cannot choose:
		// the files of a directory picker.
		dim := func(e Entry, el ui.Element) {
			if strings.HasPrefix(e.Name, ".") || !s.selectable(e) {
				el.Opacity(0.5)
			}
		}
		// The selection before this frame's keys and clicks: what an entry
		// the mode cannot choose gives the selection back to.
		before := s.current
		if s.Grid {
			s.grid.Key, s.grid.Label, s.grid.Selected, s.grid.Selection = key, label, &s.row, chosen
			view = ui.GridView(c.Key("grid/"+s.listing.Path), &s.grid, len(rows), 108, 104, func(i int) {
				e := entries[rows[i]]
				cell := ui.Column(c).Fill().Padding(8).Gap(6).AlignItems(ui.Center).ContextMenu(func(m *ui.Menu) { menu(m, i) })
				dim(e, cell)
				cell.Children(func() {
					tile(c, e, 44, on(i))
					// Two lines keep a long name's start and its end, the
					// extension; the tip has the rest.
					kit.Tooltip(c, ui.Text(c, e.Name).FontSize(12).MaxLines(2).TruncateMiddle().TextAlign(ui.Center), e.Name)
				})
			}).Grow(1).Padding(8)
		} else {
			s.list.Key, s.list.Label, s.list.Selected, s.list.Selection, s.list.Sort = key, label, &s.row, chosen, &s.sort
			s.list.CellMenu = func(m *ui.Menu, row, _ int) { menu(m, row) }
			// A file manager's columns over its 32 DIP rows, rounded and a
			// step in from the edges.
			cols := []ui.TableColumn{
				{Title: i18n.T(c, "files_entry_name"), ID: "name", Sortable: true, MinWidth: 200},
				{Title: i18n.T(c, "directory_size"), ID: "size", Width: 96, Align: ui.End, Sortable: true},
			}
			if width >= modifiedFrom {
				cols = append(cols, ui.TableColumn{Title: i18n.T(c, "directory_modified"), ID: "modified", Width: 156, Sortable: true})
			}
			if width >= kindFrom {
				cols = append(cols, ui.TableColumn{Title: i18n.T(c, "directory_kind"), ID: "kind", Width: 140, Sortable: true})
			}
			s.list.RoundRows = true
			view = ui.Table(c.Key("list/"+s.listing.Path), &s.list, cols, len(rows), func(row, col int) {
				e := entries[rows[row]]
				switch cols[col].ID {
				case "name":
					// The row of CrychicUI's file list: a tinted tile of
					// the entry's type before its name, bold once chosen.
					cell := ui.Row(c).Gap(8).MinWidth(0).AlignItems(ui.Center)
					dim(e, cell)
					cell.Children(func() {
						tile(c, e, 20, on(row))
						name := ui.Text(c, e.Name).FontSize(13).SingleLine().Shrink(1).MinWidth(0)
						if on(row) {
							name.FontWeight(600)
						}
					})
				case "kind":
					detail(kind(c, e), on(row))
				case "size":
					detail(Bytes(e), on(row))
				case "modified":
					short, full := s.modified(c, e)
					if text := detail(short, on(row)); full != "" {
						kit.Tooltip(c, text, full)
					}
				}
			}).Grow(1).Padding(0, 6)
		}
		view.Disabled(off).Label(i18n.T(c, "files"))
		// The directory that arrived takes the keys, as the one before it did.
		if s.refocus {
			s.refocus = false
			if s.Grid {
				s.grid.Focus(c)
			} else {
				s.list.Focus(c)
			}
		}
		// A click or a key that lands on an entry the mode cannot choose
		// leaves the selection where it was, and its frame opens nothing:
		// the double click that ends on a file is still submitted in the
		// frame's later passes, over the selection given back.
		if s.row >= 0 && s.row < len(rows) && !s.selectable(entries[rows[s.row]]) {
			s.row = slices.IndexFunc(rows, func(i int) bool { return entries[i].Name == before })
			s.refused = c.Now()
		}
		if s.row >= 0 && s.row < len(rows) {
			s.current = entries[rows[s.row]].Name
		} else {
			s.current = ""
		}
		if !off && view.Submitted() && !c.Now().Equal(s.refused) && s.row >= 0 && s.row < len(rows) {
			s.activate(rows[s.row])
		}
	})
	if off {
		return
	}
	if area.Shortcut(0, ui.KeyBackspace) {
		s.up()
	}
	if area.Shortcut(ui.Cmd, ui.KeyC) {
		s.copy(s.current, false)
	}
	if area.Shortcut(ui.Cmd, ui.KeyX) {
		s.copy(s.current, true)
	}
	if area.Shortcut(ui.Cmd, ui.KeyV) {
		s.paste("stop")
	}
}

// pasteMenu is the Paste submenu: what to do when a name is taken.
func (s *State) pasteMenu(c *ui.Context, m *ui.Menu) {
	m.Submenu(i18n.T(c, "files_paste"), func(m *ui.Menu) {
		for _, choice := range [][2]string{{"directory_stop", "stop"}, {"files_upload_replace", "replace"}, {"directory_keep_both", "keep_both"}} {
			if m.Item(i18n.T(c, choice[0])).Chosen() {
				s.paste(choice[1])
			}
		}
	}).Disabled(len(s.clipboard) == 0)
}

// status counts the directories and files shown, and the hidden entries
// while they are not.
func (s *State) status(c *ui.Context) {
	p := theme.Of(c)
	dirs, files, hidden := 0, 0, 0
	for _, i := range s.rows {
		switch s.listing.Entries[i].Kind {
		case "directory":
			dirs++
		case "file":
			files++
		}
	}
	if !s.Hidden {
		for _, e := range s.listing.Entries {
			if strings.HasPrefix(e.Name, ".") {
				hidden++
			}
		}
	}
	text := i18n.T(c, "directory_counts", "directories", dirs, "files", files)
	if hidden > 0 {
		text = i18n.T(c, "directory_counts_hidden", "directories", dirs, "files", files, "hidden", hidden)
	}
	ui.Text(c.Key("status"), text).FontSize(12).SingleLine().Shrink(0).Role(ui.RoleStatus).TextColor(p.MutedForeground)
}

// footer is the footer bar, 52 high under a hairline: what Confirm would
// choose, the counts, Cancel and Confirm. A directory picker always has a
// choice, the selected directory or else the one shown, and names its path;
// a file picker names the one file's path, counts several, and says so when
// it has none.
func (s *State) footer(c *ui.Context, valid bool) {
	p := theme.Of(c)
	paths := s.selection()
	icon, text, path := icons.Folder, "", ""
	if s.o.Mode != Directory {
		icon = icons.File
	}
	switch {
	case len(paths) == 1:
		text, path = paths[0], paths[0]
	case len(paths) > 1:
		text = i18n.T(c, "directory_selected_files", "count", len(paths))
	case s.loaded && s.o.Mode != Directory:
		text = i18n.T(c, "directory_nothing_selected")
	}
	pending := s.pending()
	ui.Row(c.Key("footer")).FillWidth().Height(52).Shrink(0).Padding(0, 12, 0, 16).Gap(12).AlignItems(ui.Center).
		BorderWidth(1, 0, 0, 0).BorderColor(p.Border).Children(func() {
		ui.Row(c.Key("selection")).Grow(1).Shrink(1).MinWidth(0).Gap(8).AlignItems(ui.Center).TextColor(p.MutedForeground).Children(func() {
			if text == "" {
				return
			}
			ui.Icon(c, icon).FontSize(14).Shrink(0)
			if path == "" {
				ui.Text(c, text).FontSize(12).SingleLine().Shrink(1).MinWidth(0)
				return
			}
			kit.Tooltip(c, ui.Text(c, path).Font("monospace").FontSize(12).SingleLine().TruncateMiddle().Shrink(1).MinWidth(0), path)
		})
		if s.loaded {
			s.status(c)
		}
		kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).OnClick(s.Close)
		confirm := i18n.T(c, s.o.Confirm)
		button := kit.Primary(c.Key("confirm"), confirm, s.busy || s.reading || pending || !valid || paths == nil).Children(func() {
			if pending {
				ui.Spinner(c).Size(14, 14)
			}
			ui.Text(c, confirm).FontSize(14)
		}).OnClick(s.confirm)
		// The button says what it chooses: the path beside it, to assistive
		// technology and in its tip.
		if text != "" && paths != nil {
			kit.Tooltip(c, button, text)
		}
	})
}

// nameDialog names a new folder or renames an entry.
func (s *State) nameDialog(c *ui.Context) {
	n := &s.naming
	if !n.open {
		return
	}
	title := i18n.T(c, "directory_create")
	if n.from != "" {
		title = i18n.T(c, "files_rename")
	}
	kit.Dialog(c.Key("picker-name"), &n.open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		input := ui.TextInput(c.Key("input"), &n.value).Placeholder(i18n.T(c, "directory_name")).Label(i18n.T(c, "directory_name")).
			FillWidth().AutoFocus()
		if input.Submitted() {
			s.saveName()
		}
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).OnClick(func() { n.open = false })
			save := i18n.T(c, "settings_save")
			kit.Primary(c.Key("save"), save, false).Children(func() { ui.Text(c, save).FontSize(14) }).OnClick(s.saveName)
		})
	}).Width(400).Label(title)
}

// trashDialog asks before moving an entry to the host's trash.
func (s *State) trashDialog(c *ui.Context) {
	t := &s.trashing
	if !t.open {
		return
	}
	label := i18n.T(c, "directory_trash")
	if kit.AlertDialog(c.Key("picker-trash"), &t.open, label, i18n.T(c, "directory_trash_confirm", "name", t.label),
		i18n.T(c, "settings_cancel"), label) == 1 {
		s.manage(Action{Kind: "trash", Paths: []string{t.path}})
	}
}

// jumpDialog is Go to folder: the command dialog taking a path, its one row
// the path's parts.
func (s *State) jumpDialog(c *ui.Context) {
	j := &s.jump
	if !j.open {
		return
	}
	var items []kit.CommandItem
	if query := strings.TrimSpace(j.query); query != "" {
		// Either separator splits the path while no directory has said
		// which its host uses.
		var parts []string
		for _, part := range strings.FieldsFunc(query, func(r rune) bool {
			return s.loaded && string(r) == s.listing.Separator || !s.loaded && (r == '/' || r == '\\')
		}) {
			if part != "" {
				parts = append(parts, part)
			}
		}
		label := query
		if len(parts) > 0 {
			label = strings.Join(parts, " › ")
		}
		items = append(items, kit.CommandItem{ID: "target", Label: label, Icon: icons.Folder, Disabled: s.busy})
	}
	kit.Command(c, "picker-go", &j.open, &j.query, &j.row, items, kit.CommandProps{Placeholder: i18n.T(c, "directory_address")},
		func(int) { s.navigate(strings.TrimSpace(j.query), goTo, 0) })
}
