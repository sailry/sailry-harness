package panel

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

type fileSearch struct {
	query      SearchQuery
	result     *SearchResult
	running    bool
	generation uint64
}

func (pr *Project) ResetFileSearch() {
	state := &pr.Files.search
	state.generation++
	state.running = false
	state.result = nil
	if pr.Actions != nil && pr.Actions.CancelSearch != nil {
		pr.Actions.CancelSearch()
	}
}
func (pr *Project) runFileSearch() {
	pr.ResetFileSearch()
	state := &pr.Files.search
	if strings.TrimSpace(state.query.Query) == "" || pr.Actions == nil || pr.Actions.Search == nil {
		return
	}
	generation := state.generation
	state.running = true
	pr.Actions.Search(state.query, func(result SearchResult) {
		if generation != state.generation {
			return
		}
		state.running = false
		state.result = &result
	})
}
func (pr *Project) filesHeader(c *ui.Context) {
	if pr.Actions == nil {
		return
	}
	p := theme.Of(c)
	ui.Row(c.Key("file-header")).Height(barHeight).Shrink(0).PaddingX(8).Gap(4).BorderWidth(0, 0, 1, 0).BorderColor(p.Border).Children(func() {
		ui.Text(c, i18n.T(c, "file_tree")).FontSize(14).SingleLine().Grow(1).Shrink(1)
		kit.MenuButton(c.Key("create"), "file-create-"+pr.ID, func(b ui.Element) { kit.IconLook(c, b, icons.Plus, i18n.T(c, "files_create"), "", 28, false) }, func(m *kit.Menu) {
			directory := pr.directory(pr.Files.sel.current)
			for _, key := range []string{"files_new_file", "files_new_directory"} {
				if m.Item(i18n.T(c, key)).Chosen() {
					pr.fileAction(c, FileAction{Kind: key, Directory: directory})
				}
			}
			if m.Item(i18n.T(c, "files_upload")).Chosen() && pr.Actions.Upload != nil {
				pr.Actions.Upload(directory)
			}
			paste := pr.Actions.PasteReady != nil && pr.Actions.PasteReady()
			if m.Item(i18n.T(c, "files_paste")).Disabled(!paste).Chosen() {
				pr.fileAction(c, FileAction{Kind: "files_paste", Directory: directory})
			}
			if pr.Actions.Pending != nil {
				pending := pr.Actions.Pending()
				if len(pending) > 0 {
					m.Separator()
				}
				for _, entry := range pending {
					if m.Item(i18n.T(c, "files_check_result")+" · "+entry.Label).Chosen() && entry.Run != nil {
						entry.Run(c)
					}
				}
			}
		}, kit.MenuOptions{At: ui.AnchorBottomEnd, Self: ui.AnchorTopEnd})
		kit.IconButton(c.Key("search"), icons.Search, i18n.T(c, "file_search"), 28, pr.Files.searchOpen).OnClick(func() {
			pr.Files.searchOpen = !pr.Files.searchOpen
			if !pr.Files.searchOpen {
				pr.ResetFileSearch()
			}
		})
	})
}
func (pr *Project) fileSearchView(c *ui.Context) {
	p := theme.Of(c)
	state := &pr.Files.search
	ui.Column(c.Key("search-fields")).Padding(8).Gap(8).Children(func() {
		submitted := false
		before := state.query
		ui.TextInput(c.Key("query"), &state.query.Query).Placeholder(i18n.T(c, "file_search_query")).Label(i18n.T(c, "file_search_query")).FillWidth().OnSubmit(func() { submitted = true; pr.runFileSearch() })
		ui.TextInput(c.Key("filter"), &state.query.Filter).Placeholder(i18n.T(c, "file_search_filter")).Label(i18n.T(c, "file_search_filter")).FillWidth().OnSubmit(func() { submitted = true; pr.runFileSearch() })
		if before != state.query && !submitted {
			pr.ResetFileSearch()
		}
		ui.Row(c.Key("search-controls")).Gap(4).Children(func() {
			for _, toggle := range []struct {
				key, text string
				value     *bool
			}{{"file_search_case", "Aa", &state.query.CaseSensitive}, {"file_search_regex", ".*", &state.query.Regex}} {
				button := kit.Ghost(c.Key(toggle.key), i18n.T(c, toggle.key), *toggle.value).Height(28).PaddingX(8).Children(func() { ui.Text(c, toggle.text).FontSize(13) })
				button.OnClick(func() { *toggle.value = !*toggle.value; pr.ResetFileSearch() })
			}
			ui.Spacer(c)
			icon, label := icons.Search, "file_search"
			if state.running {
				icon, label = icons.Close, "settings_cancel"
			}
			kit.IconButton(c.Key("run"), icon, i18n.T(c, label), 28, false).OnClick(func() {
				if state.running {
					pr.ResetFileSearch()
				} else {
					pr.runFileSearch()
				}
			})
		})
	})
	if state.running {
		ui.Text(c, i18n.T(c, "file_search_running")).FontSize(12).PaddingX(12).TextColor(p.MutedForeground)
	} else if state.result != nil && state.result.Partial {
		ui.Text(c, i18n.T(c, "file_search_partial")).FontSize(12).PaddingX(12).TextColor(p.MutedForeground)
	}
	ui.Scroll(c.Key("search-results")).Grow(1).MinHeight(0).Padding(8).Children(func() {
		if state.result == nil || state.result.Failed {
			return
		}
		if len(state.result.Matches) == 0 {
			kit.Result(c, kit.ResultProps{Icon: icons.Search, Title: i18n.T(c, "file_search_empty")}).Grow(1).Justify(ui.Center)
			return
		}
		for index, match := range state.result.Matches {
			kit.Ghost(c.Key(index), fmt.Sprintf("%s:%d", match.Path, match.Line), false).FillWidth().Padding(6, 8).Justify(ui.Start).Children(func() {
				ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Children(func() {
					ui.Textf(c, "%s:%d", match.Path, match.Line).FontSize(14).SingleLine()
					ui.Text(c, match.Text).FontSize(12).SingleLine().TextColor(p.MutedForeground)
				})
			}).OnClick(func() {
				if pr.Actions != nil && pr.Actions.OpenFileAt != nil {
					pr.Actions.OpenFileAt(match.Path, match.Line)
				}
			})
		}
	})
}
func (pr *Project) directoryTail(c *ui.Context, directory string) {
	state := pr.Files.directories[directory]
	if state.Loading {
		ui.Row(c.Key("directory-loading")).Height(28).PaddingX(12).Gap(8).Children(func() { kit.Busy(c) })
		return
	}
	if state.Failed {
		kit.Ghost(c.Key("directory-refresh"), i18n.T(c, "refresh"), false).Height(28).PaddingX(12).Children(func() { ui.Text(c, i18n.T(c, "refresh")).FontSize(12) }).OnClick(func() {
			if pr.Actions != nil && pr.Actions.RefreshDirectory != nil {
				pr.Actions.RefreshDirectory(directory)
			}
		})
	}
	if state.Next {
		kit.Ghost(c.Key("directory-more"), i18n.T(c, "files_load_more"), false).Height(28).PaddingX(12).Children(func() { ui.Text(c, i18n.T(c, "files_load_more")).FontSize(12) }).OnClick(func() {
			if pr.Actions != nil && pr.Actions.NextDirectory != nil {
				pr.Actions.NextDirectory(directory)
			}
		})
	}
	if state.Partial {
		ui.Text(c, i18n.T(c, "files_listing_partial")).FontSize(12).PaddingX(12).TextColor(theme.Of(c).MutedForeground)
	}
}
