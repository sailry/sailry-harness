package shell

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
)

// Choice is a row of the search: a label with keywords that match too, a
// folder it opens, or an action.
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

// Next moves the selection among n rows, round at either end; with none, down
// goes to the first and up to the last. It returns -1 for no rows.
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
			list = append(list, Choice{ID: h.ID, Label: t(h.Name), Icon: icons.Cpu, Selected: h.ID == a.Host, run: func() {
				if a.Preview {
					kit.Message(c, t("preview_notice"))
				} else {
					a.openHost(h.ID, false)
				}
			}})
		}
		return []Group{{Choices: list}}
	case "settings":
		var list []Choice
		for _, g := range settings.Groups {
			for _, it := range g.Items {
				sec := it.Section
				list = append(list, Choice{ID: string(sec), Label: t(string(sec)), Icon: it.Icon,
					Selected: a.Page == SettingsPage && a.Settings.Section == sec, run: func() {
						a.Page = SettingsPage
						a.Settings.Open(sec)
					}})
			}
		}
		for _, page := range a.Settings.PluginPages() {
			section := page.Section
			list = append(list, Choice{ID: string(section), Label: page.Label, Icon: page.Icon,
				Selected: a.Page == SettingsPage && a.Settings.Section == section, run: func() {
					a.Page = SettingsPage
					a.Settings.Open(section)
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
	for _, id := range a.features {
		f, exists := a.lookupFeature(id)
		if !exists {
			continue
		}
		if f.ID == "page:conversation" || f.ID == "page:activity" {
			continue
		}
		root = append(root, Choice{ID: f.ID, Label: t(f.Label), Icon: f.Icon, run: func() { a.openFeature(c, f.ID) }})
	}
	return []Group{{Choices: root}}
}

// searchOverlay is the search: a dialog 480 wide, the query over the rows of
// the folder, 36 DIPs each, under 28 DIP headings. Up and Down move, Enter
// opens a folder or runs the row. A folder has no back row: Backspace on an
// empty query goes back to the root, and Escape clears the query, then goes
// back to the root, then closes. The dialog has no close button.
func (a *App) searchOverlay(c *ui.Context) {
	s := &a.search
	if !s.open {
		return
	}
	groups := Filter(a.catalog(c, s.folder), s.query)
	var rows []Choice
	var items []kit.CommandItem
	for _, g := range groups {
		for _, ch := range g.Choices {
			rows = append(rows, ch)
			items = append(items, kit.CommandItem{ID: ch.ID, Label: ch.Label, Icon: ch.Icon, Group: g.Heading,
				Folder: ch.Folder != "", Checked: ch.Selected})
		}
	}
	kit.Command(c, "search", &s.open, &s.query, &s.sel, items, kit.CommandProps{
		Placeholder: i18n.T(c, "search"), Empty: i18n.T(c, "connection_search_empty"),
		// A folder has no back row: Backspace or Escape on an empty query goes back
		// to the root.
		Back: func() bool {
			if s.folder == "" {
				return false
			}
			s.folder = ""
			return true
		},
	}, func(i int) {
		if ch := rows[i]; ch.Folder != "" {
			s.folder, s.query, s.sel = ch.Folder, "", 0
		} else {
			s.open = false
			ch.run()
		}
	})
}
