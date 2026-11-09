package shell

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/settings"
	"github.com/sailry/sailry-go/internal/sidebar"
	"github.com/sailry/sailry-go/internal/theme"
)

// Choice is a row of the search (shell/search/catalog.rs): a label with
// keywords that match too, a folder it opens, or an action.
type Choice struct {
	ID       string
	Label    string
	Icon     *ui.SVG
	Keywords []string
	Folder   string // the folder it opens, "" for an action
	Selected bool   // what the window shows: a check
	run      func()
}

// Group is a section of choices under an optional heading.
type Group struct {
	Heading string
	Choices []Choice
}

// Filter keeps the choices whose label or a keyword holds the query,
// ignoring case, and drops groups left empty; the order stays (no score,
// no limit). An empty query keeps all (Rows::filter).
func Filter(groups []Group, query string) []Group {
	q := strings.ToLower(query)
	var out []Group
	for _, g := range groups {
		kept := slices.DeleteFunc(slices.Clone(g.Choices), func(ch Choice) bool {
			if strings.Contains(strings.ToLower(ch.Label), q) {
				return false
			}
			for _, k := range ch.Keywords {
				if strings.Contains(strings.ToLower(k), q) {
					return false
				}
			}
			return true
		})
		if len(kept) > 0 {
			out = append(out, Group{Heading: g.Heading, Choices: kept})
		}
	}
	return out
}

// Next moves the selection among n rows, round at either end; with none,
// down goes to the first and up to the last. It returns -1 for no rows
// (command_picker.rs next).
func Next(n, selected int, reverse bool) int {
	switch {
	case n == 0:
		return -1
	case selected < 0 || selected >= n:
		if reverse {
			return n - 1
		}
		return 0
	case reverse:
		return (selected - 1 + n) % n
	}
	return (selected + 1) % n
}

// search is the overlay's state: its folder, query and selected row.
type search struct {
	open   bool
	folder string
	query  string
	sel    int
}

func (a *App) openSearch() {
	a.search = search{open: true}
}

// catalog is the folder's groups (catalog::entries).
func (a *App) catalog(c *ui.Context, folder string) []Group {
	t := func(key string) string { return i18n.T(c, key) }
	switch folder {
	case "sessions":
		var list []Choice
		for _, id := range a.Sidebar.Order() {
			cv := a.Data.Find(id)
			var keys []string
			if pr := a.Data.ProjectOf(id); pr != nil {
				keys = []string{pr.Name}
			}
			list = append(list, Choice{ID: id, Label: sidebar.Title(c, cv), Icon: icons.Chat, Keywords: keys,
				Selected: id == a.Sidebar.Selected && a.Page == ConversationPage, run: func() {
					a.Page, a.Sidebar.Selected = ConversationPage, id
				}})
		}
		return []Group{{Choices: list}}
	case "projects":
		var list []Choice
		for _, pr := range a.Data.Projects {
			list = append(list, Choice{ID: pr.ID, Label: pr.Name, Icon: icons.Folder, Keywords: []string{pr.Path}, run: func() {
				a.Page, a.NavOpen = ConversationPage, true
				a.Sidebar.Reveal(pr.ID)
			}})
		}
		return []Group{{Choices: list}}
	case "hosts":
		var list []Choice
		for _, h := range a.Data.Hosts {
			list = append(list, Choice{ID: h.ID, Label: t(h.Name), Icon: icons.Cpu, Selected: h.Local, run: func() {
				c.Toast(t("preview_notice"))
			}})
		}
		return []Group{{Choices: list}}
	case "settings":
		var list []Choice
		for _, sec := range []settings.Section{settings.Appearance, settings.Providers, settings.Plugins} {
			list = append(list, Choice{ID: string(sec), Label: t(string(sec)), Icon: icons.Settings,
				Selected: a.Page == SettingsPage && a.Settings.Section == sec, run: func() {
					a.Page, a.Settings.Section = SettingsPage, sec
				}})
		}
		return []Group{{Heading: t("settings"), Choices: list}}
	}
	root := []Choice{
		{ID: "sessions", Label: t("shell_sessions"), Icon: icons.Chat, Folder: "sessions"},
		{ID: "projects", Label: t("projects"), Icon: icons.Folder, Folder: "projects"},
		{ID: "hosts", Label: t("hosts"), Icon: icons.Cpu, Folder: "hosts"},
		{ID: "settings", Label: t("settings"), Icon: icons.Settings, Folder: "settings"},
	}
	for _, f := range Features {
		if f.ID == "page:conversation" || f.ID == "page:activity" {
			continue
		}
		root = append(root, Choice{ID: f.ID, Label: t(f.Label), Icon: f.Icon, run: func() { a.openFeature(c, f.ID) }})
	}
	return []Group{{Choices: root}}
}

// searchOverlay is the search (shell/search.rs, command_picker.rs): a
// frosted dialog 480 wide near the top, the query over the rows of the
// folder, 36 DIPs each, under 28 DIP headings. Up and Down move, Enter
// opens a folder or runs the row, Escape clears the query and then
// closes, Backspace on an empty query goes back to the root.
func (a *App) searchOverlay(c *ui.Context) {
	s := &a.search
	if !s.open {
		return
	}
	p := theme.Of(c)
	groups := Filter(a.catalog(c, s.folder), s.query)
	var rows []Choice
	for _, g := range groups {
		rows = append(rows, g.Choices...)
	}
	if s.sel >= len(rows) {
		s.sel = 0
	}
	choose := func(ch Choice) {
		if ch.Folder != "" {
			s.folder, s.query, s.sel = ch.Folder, "", 0
			return
		}
		s.open = false
		ch.run()
	}
	w, _ := c.Size()
	ui.DialogBase(c.Key("search"), &s.open, func(backdrop, panel ui.Element) {
		backdrop.Background(theme.Shade(0.2)).Justify(ui.Start).Padding(64, 0, 0, 0)
		kit.Floating(c, panel).Width(min(480, w-40)).MaxHeightPercent(60).Label(i18n.T(c, "search")).Clip()
		if s.folder != "" {
			ui.Row(c.Key("back")).Height(36).PaddingX(4).Shrink(0).BorderWidth(0, 0, 1, 0).BorderColor(p.Border).Children(func() {
				back := i18n.T(c, "reference_back")
				kit.Ghost(c.Key("button"), back, false).Height(28).PaddingX(8).Gap(6).Children(func() {
					ui.Icon(c, icons.ArrowLeft).FontSize(14)
					ui.Text(c, back).FontSize(14)
				}).OnClick(func() { s.folder, s.query, s.sel = "", "", 0 })
			})
		}
		ui.Row(c.Key("query")).Height(45).PaddingX(12).Gap(8).Shrink(0).BorderWidth(0, 0, 1, 0).BorderColor(p.Border).Children(func() {
			ui.Icon(c, icons.Search).FontSize(16).TextColor(p.MutedForeground)
			in := ui.TextInputBase(c.Key("input"), &s.query).Grow(1).FontSize(14).TextColor(p.PopoverForeground).
				Placeholder(i18n.T(c, "search")).Label(i18n.T(c, "search")).AutoFocus()
			if in.Changed() {
				s.sel = 0
			}
			if in.Shortcut(0, ui.KeyDown) {
				s.sel = Next(len(rows), s.sel, false)
			}
			if in.Shortcut(0, ui.KeyUp) {
				s.sel = Next(len(rows), s.sel, true)
			}
			in.OnSubmit(func() {
				if s.sel >= 0 && s.sel < len(rows) {
					choose(rows[s.sel])
				}
			})
			if in.Shortcut(0, ui.KeyEscape) {
				if s.query != "" {
					s.query, s.sel = "", 0
				} else {
					s.open = false
				}
			}
			if s.query == "" && s.folder != "" && in.Shortcut(0, ui.KeyBackspace) {
				s.folder, s.sel = "", 0
			}
		})
		ui.Scroll(c.Key(s.folder)).MaxHeight(344).Padding(8).Children(func() {
			if len(rows) == 0 {
				ui.Text(c, i18n.T(c, "connection_search_empty")).FontSize(14).Padding(12).TextColor(p.MutedForeground)
				return
			}
			i := 0
			for _, g := range groups {
				if g.Heading != "" {
					ui.Text(c, g.Heading).FontSize(14).Height(28).PaddingX(8).TextColor(p.MutedForeground)
				}
				for _, ch := range g.Choices {
					idx := i
					i++
					row := ui.ButtonBase(c.Key(ch.ID)).Label(ch.Label).Height(36).PaddingX(8).Gap(8).Radius(p.Radius).Shrink(0).
						TextColor(p.PopoverForeground)
					if row.Hovered() {
						s.sel = idx
					}
					if idx == s.sel {
						row.Background(p.Hover).TextColor(p.HoverForeground).ScrollIntoView()
					}
					row.Children(func() {
						ui.Icon(c, ch.Icon).FontSize(16)
						ui.Text(c, ch.Label).FontSize(14).SingleLine().Grow(1).Shrink(1)
						switch {
						case ch.Folder != "":
							ui.Icon(c, icons.ChevronRight).FontSize(14).TextColor(p.MutedForeground)
						case ch.Selected:
							ui.Icon(c, icons.Check).FontSize(14)
						}
					}).OnClick(func() { choose(ch) })
				}
			}
		})
	})
}
