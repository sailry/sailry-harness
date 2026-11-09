package panel

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/preview"
	"github.com/sailry/sailry-go/internal/theme"
)

// Panel metrics of tabs.rs, navigation_tabs.rs and the preview columns.
const (
	barHeight = 40
	tabHeight = 28
	tabMax    = 180
	rowHeight = 28
	footer    = 32
)

// notice tells that an action would leave the preview: write files, run
// git, a terminal or load a page.
func notice(c *ui.Context) { c.Toast(i18n.T(c, "preview_notice")) }

// Shortcut is a shortcut's text as menus and the launcher show it.
type Shortcut func(mods ui.Modifiers, key ui.Key) string

// View builds the panel, the tab host: the tab strip over the active
// resource, or the launcher. pr is the shown project's state, nil when the
// conversation has none; keys writes the launcher's shortcuts.
func View(c *ui.Context, s *State, pr *Project, keys Shortcut) {
	if len(s.Tabs) == 0 {
		launcher(c, s, keys)
		return
	}
	s.strip(c, keys)
	ui.Column(c.Key(s.Active)).Grow(1).MinHeight(0).Children(func() {
		switch s.Active {
		case Files, Changes:
			pr.view(c, s.Active, s.Active != s.Paged)
		case History:
			s.history(c)
		case Documents:
			s.documents(c)
		case Browser:
			browser(c, s)
		case Terminal:
			terminal(c)
		}
	})
}

// Page builds a project resource, Files or Changes, as an independent
// page in the main column: the page host, over the same Project the tab
// shows.
func Page(c *ui.Context, k Kind, pr *Project) {
	ui.Column(c.Key(k)).Grow(1).MinHeight(0).Children(func() { pr.view(c, k, true) })
}

// view is the Files or Git component in either host, or the project
// prompt when there is no project. The host that owns the scrolling keeps
// it in the project; while the page and the closing panel both show a
// resource, the page owns it, as MyGo binds a ScrollState to one
// container per build.
func (pr *Project) view(c *ui.Context, k Kind, own bool) {
	if pr == nil {
		kit.Empty(c, ResourceOf(k).Icon, i18n.T(c, "resource_project_empty"), "", nil).Grow(1).Justify(ui.Center)
		return
	}
	if k == Files {
		pr.files(c, own)
	} else {
		pr.changes(c, own)
	}
}

// scroll is the project's ScrollState s for the host that owns it, nil
// for the other.
func scroll(s *ui.ScrollState, own bool) *ui.ScrollState {
	if !own {
		return nil
	}
	return s
}

// strip is the tab row (tabs.rs): 28 DIP tabs up to 180 wide, the active
// one in tab_active, a close button showing on hover; it scrolls sideways,
// and past four tabs a chevron lists them all. "+" opens another resource.
func (s *State) strip(c *ui.Context, keys Shortcut) {
	p := theme.Of(c)
	ui.Row(c.Key("strip")).Height(barHeight).Shrink(0).PaddingX(8).Gap(4).BorderWidth(0, 0, 1, 0).BorderColor(p.Border).Children(func() {
		ui.ScrollHorizontal(c.Key("tabs")).Grow(1).Shrink(1).MinWidth(0).Scrollbars(false).Children(func() {
			ui.Row(c).Gap(4).Height(barHeight).Children(func() {
				closed := Kind("")
				for _, k := range s.Tabs {
					r := ResourceOf(k)
					active := k == s.Active
					label := i18n.T(c, r.Label)
					tab := ui.ButtonBase(c.Key(k)).Label(label).Height(tabHeight).MaxWidth(tabMax).PaddingStart(8).PaddingEnd(4).
						Gap(6).Radius(p.Radius).Cursor(ui.CursorPointer).TextColor(p.MutedForeground)
					hovered := tab.Hovered()
					switch {
					case active:
						tab.Background(p.TabActive).TextColor(p.TabActiveText).AccessSelected(true).ScrollIntoView()
					case hovered:
						tab.TextColor(p.Foreground)
					}
					tab.Children(func() {
						ui.Icon(c, r.Icon).FontSize(14).Shrink(0)
						ui.Text(c, label).FontSize(14).SingleLine().Shrink(1)
						x := kit.IconButton(c.Key("close"), icons.Close, i18n.T(c, "close"), 20, false).OnClick(func() { closed = k })
						if !hovered && !active && !x.FocusVisible() {
							x.Opacity(0)
						}
					}).OnClick(func() { s.Active = k })
				}
				if closed != "" {
					s.Close(closed)
				}
			})
		})
		if len(s.Tabs) > Overflow {
			kit.IconMenu(c, "all", icons.ChevronDown, i18n.T(c, "navigation_tabs"), 28, false, nil, func(m *kit.Menu) {
				for _, k := range s.Tabs {
					r := ResourceOf(k)
					if m.Item(kit.Item{Label: i18n.T(c, r.Label), Icon: r.Icon, Checked: k == s.Active}) {
						s.Active = k
					}
				}
			})
		}
		kit.IconMenu(c, "add", icons.Plus, i18n.T(c, "details"), 28, false, nil, func(m *kit.Menu) {
			for _, r := range Resources {
				hint := ""
				if r.Key != 0 {
					hint = keys(r.Mods, r.Key)
				}
				if m.Item(kit.Item{Label: i18n.T(c, r.Label), Icon: r.Icon, Hint: hint, Checked: r.Kind == s.Active}) {
					s.Open(r.Kind)
				}
			}
		})
	})
}

// launcher lists the resources to open (launcher.rs): full-width 40 DIP
// rows, the icon, the label and the shortcut, centered up to 440 wide.
func launcher(c *ui.Context, s *State, keys Shortcut) {
	p := theme.Of(c)
	ui.Column(c.Key("launcher")).Grow(1).Padding(24).AlignItems(ui.Center).Children(func() {
		ui.Column(c).FillWidth().MaxWidth(440).Gap(8).Children(func() {
			ui.Text(c, i18n.T(c, "details")).FontSize(14).FontWeight(500).Padding(0, 4, 4, 4).TextColor(p.MutedForeground)
			for _, r := range Resources {
				label := i18n.T(c, r.Label)
				b := ui.ButtonBase(c.Key(r.Kind)).Label(label).FillWidth().Height(40).PaddingX(12).Gap(10).Radius(p.Radius).
					Background(p.Secondary).TextColor(p.SecondaryForeground).Cursor(ui.CursorPointer)
				if b.Hovered() {
					b.Background(p.SecondaryHover)
				}
				b.Children(func() {
					ui.Icon(c, r.Icon).FontSize(16)
					ui.Text(c, label).FontSize(14).SingleLine().Grow(1)
					if r.Key != 0 {
						kbd(c, keys(r.Mods, r.Key))
					}
				}).OnClick(func() { s.Open(r.Kind) })
			}
		})
	})
}

// kbd is Kit's Kbd: a shortcut in a small outlined box.
func kbd(c *ui.Context, text string) {
	p := theme.Of(c)
	ui.Text(c, text).FontSize(12).SingleLine().Padding(1, 6).Radius(4).Border(1, p.Border).TextColor(p.MutedForeground)
}

// toolbar is a column's 40 DIP bar under the strip.
func toolbar(c *ui.Context, key string, children func()) {
	p := theme.Of(c)
	ui.Row(c.Key(key)).Height(barHeight).Shrink(0).PaddingX(8).Gap(4).BorderWidth(0, 0, 1, 0).BorderColor(p.Border).Children(children)
}

func separator(c *ui.Context) {
	ui.Box(c).Width(1).Height(16).Margin(0, 4).Background(theme.Of(c).Border)
}

// status is a column's 32 DIP footer: the path, and what goes after it.
func status(c *ui.Context, path string, after func()) {
	p := theme.Of(c)
	ui.Row(c.Key("footer")).Height(footer).Shrink(0).PaddingX(12).Gap(12).BorderWidth(1, 0, 0, 0).BorderColor(p.Border).
		TextColor(p.MutedForeground).Children(func() {
		ui.Text(c, path).FontSize(12).SingleLine().Grow(1).Shrink(1).ContextMenu(func(m *ui.Menu) {
			if m.Item(i18n.T(c, "files_copy_path")).Chosen() {
				c.WriteClipboard(path)
			}
		})
		if after != nil {
			after()
		}
	})
}

// files is the project's files: the shown file's preview beside the tree
// (preview_files.rs: the preview first, the list after).
func (pr *Project) files(c *ui.Context, own bool) {
	v := &pr.Files
	p := theme.Of(c)
	ui.Row(c).Grow(1).MinHeight(0).AlignItems(ui.Stretch).Children(func() {
		ui.Column(c.Key("preview")).Grow(1).MinWidth(0).Children(func() { pr.filePreview(c, own) })
		ui.Column(c.Key("tree")).Width(220).Shrink(0).BorderWidth(0, 0, 0, 1).BorderColor(p.Border).Children(func() {
			ui.Text(c, i18n.T(c, "file_tree")).FontSize(14).FontWeight(500).Height(barHeight).PaddingX(12).Shrink(0)
			ui.Scroll(c.Key("scroll")).TrackScroll(scroll(&v.tree, own)).Grow(1).Padding(0, 8, 8, 8).Children(func() {
				tree := ui.TreeBase(c)
				tree.Root.Children(func() {
					var node func(n *preview.FileNode)
					node = func(n *preview.FileNode) {
						c.Key(n.Path)
						var open *bool
						if n.Children != nil {
							if v.folders[n.Path] == nil {
								o := true
								v.folders[n.Path] = &o
							}
							open = v.folders[n.Path]
						}
						item := tree.Item(open)
						row := item.Row.Height(rowHeight).PaddingStart(8 + 16*float32(item.Depth())).PaddingEnd(8).Gap(8).
							Radius(p.Radius).Cursor(ui.CursorPointer).Label(n.Name)
						selected := n.Path == v.file
						text := p.MutedForeground
						switch {
						case selected:
							row.Background(p.Hover)
							text = p.HoverForeground
						case row.Hovered():
							text = p.Foreground
						}
						if row.Clicked() {
							if open != nil {
								*open = !*open
							} else {
								v.Show(n.Path)
							}
						}
						row.TextColor(text).Children(func() {
							icon := icons.File
							if open != nil {
								icon = icons.Folder
							}
							ui.Icon(c, icon).FontSize(16).TextColor(p.MutedForeground)
							ui.Text(c, n.Name).FontSize(14).SingleLine().Shrink(1)
						})
						item.Group(func() {
							for _, kid := range n.Children {
								node(kid)
							}
						})
					}
					node(pr.Data.Files)
				})
			})
		})
	})
}

func findFile(n *preview.FileNode, path string) *preview.FileNode {
	if n.Path == path {
		return n
	}
	for _, k := range n.Children {
		if f := findFile(k, path); f != nil {
			return f
		}
	}
	return nil
}

// filePreview is the shown file, read-only: the toolbar (Save, Undo and
// Redo disabled, Find, Document | Source for markdown), the text with
// line numbers, and the status bar.
func (pr *Project) filePreview(c *ui.Context, own bool) {
	p := theme.Of(c)
	v := &pr.Files
	f := findFile(pr.Data.Files, v.file)
	if f == nil {
		kit.Empty(c, icons.File, i18n.T(c, "file_empty"), "", nil)
		return
	}
	md := strings.HasSuffix(f.Name, ".md")
	toolbar(c, "toolbar", func() {
		kit.IconButton(c.Key("save"), icons.Save, i18n.T(c, "files_save"), 28, false).Disabled(true)
		separator(c)
		kit.IconButton(c.Key("undo"), icons.Undo, i18n.T(c, "files_undo"), 28, false).Disabled(true)
		kit.IconButton(c.Key("redo"), icons.Redo, i18n.T(c, "files_redo"), 28, false).Disabled(true)
		separator(c)
		kit.IconButton(c.Key("find"), icons.Search, i18n.T(c, "files_find"), 28, false).OnClick(func() { notice(c) })
		ui.Spacer(c)
		if md {
			ui.Segmented(c.Key("view"), &v.source, i18n.T(c, "content_document"), i18n.T(c, "content_source"))
		}
	})
	ui.Scroll(c.Key("text")).TrackScroll(scroll(&v.text, own)).Grow(1).Padding(12, 0).Children(func() {
		if md && v.source == 0 {
			ui.Column(c).PaddingX(16).Children(func() { markdown.View(c, f.Text, nil) })
			return
		}
		for i, line := range strings.Split(f.Text, "\n") {
			ui.Row(c.Key(i)).Gap(12).PaddingX(12).Children(func() {
				ui.Textf(c, "%d", i+1).FontSize(12).Font("monospace").Width(28).TextAlign(ui.End).Shrink(0).TextColor(p.MutedForeground.Alpha(0.6))
				ui.Text(c, line).FontSize(12).Font("monospace").PreserveSpaces().Grow(1)
			})
		}
	})
	lang := "Go"
	switch {
	case md:
		lang = "Markdown"
	case strings.HasSuffix(f.Name, ".mod"):
		lang = "Go Module"
	}
	status(c, f.Path, func() {
		ui.Text(c, i18n.T(c, "files_cursor_position", "line", 1, "column", 1)).FontSize(12).SingleLine()
		ui.Text(c, "UTF-8").FontSize(12).SingleLine()
		ui.Text(c, lang).FontSize(12).SingleLine()
	})
}

// statusColor is the color of a change's letter.
func statusColor(p *theme.Palette, st string) ui.Color {
	switch st {
	case "A":
		return p.Success
	case "D":
		return p.Danger
	}
	return p.Warning
}

// changes is the working tree's changes: the shown file's hunks beside the
// list, and the commit box under it (preview_git.rs, hunks/render.rs).
func (pr *Project) changes(c *ui.Context, own bool) {
	p := theme.Of(c)
	v := &pr.Git
	ui.Row(c).Grow(1).MinHeight(0).AlignItems(ui.Stretch).Children(func() {
		ui.Column(c.Key("diff")).Grow(1).MinWidth(0).Children(func() { pr.diff(c, own) })
		ui.Column(c.Key("list")).Width(240).Shrink(0).BorderWidth(0, 0, 0, 1).BorderColor(p.Border).Children(func() {
			ui.Row(c).Height(barHeight).PaddingStart(12).PaddingEnd(8).Shrink(0).Children(func() {
				ui.Text(c, i18n.T(c, "changes")).FontSize(14).FontWeight(500).Grow(1)
				kit.IconButtonHint(c.Key("stage"), icons.Plus, i18n.T(c, "git_stage_all"), i18n.T(c, "git_actions_preview"), 24, false).
					Disabled(true)
			})
			ui.Scroll(c.Key("files")).TrackScroll(scroll(&v.list, own)).Grow(1).Padding(0, 8, 8, 8).Gap(2).Children(func() {
				for _, ch := range pr.Data.Changes {
					name := ch.Path[strings.LastIndex(ch.Path, "/")+1:]
					b := ui.ButtonBase(c.Key(ch.Path)).Label(ch.Path).Height(rowHeight).PaddingX(8).Gap(8).Radius(p.Radius).
						Cursor(ui.CursorPointer).TextColor(p.MutedForeground)
					switch {
					case ch.Path == v.change:
						b.Background(p.Hover).TextColor(p.HoverForeground).AccessSelected(true)
					case b.Hovered():
						b.TextColor(p.Foreground)
					}
					kit.Tip(c, b, ch.Path).Children(func() {
						ui.Icon(c, icons.File).FontSize(16).TextColor(p.MutedForeground)
						ui.Text(c, name).FontSize(14).SingleLine().Grow(1).Shrink(1)
						ui.Text(c, ch.Status).FontSize(12).FontWeight(600).TextColor(statusColor(p, ch.Status))
					}).OnClick(func() { v.Show(ch.Path) })
				}
			})
			ui.Column(c.Key("commit")).Padding(12).Gap(8).Shrink(0).BorderWidth(1, 0, 0, 0).BorderColor(p.Border).Children(func() {
				ui.TextArea(c.Key("message"), &v.commit).Placeholder(i18n.T(c, "git_commit_message")).Label(i18n.T(c, "git_commit_message")).
					FillWidth().Height(64)
				commit := i18n.T(c, "git_commit")
				kit.Primary(c.Key("commit"), commit, true).FillWidth().Children(func() {
					ui.Icon(c, icons.Check).FontSize(14)
					ui.Text(c, commit).FontSize(14)
				})
				ui.Text(c, i18n.T(c, "git_preview_only")).FontSize(12).TextColor(p.MutedForeground)
			})
		})
	})
}

// diff is a changed file's hunks: a card each, its @@ line and Reject,
// Stage and Accept over the lines with both numbers.
func (pr *Project) diff(c *ui.Context, own bool) {
	p := theme.Of(c)
	v := &pr.Git
	var ch *preview.FileChange
	for i := range pr.Data.Changes {
		if pr.Data.Changes[i].Path == v.change {
			ch = &pr.Data.Changes[i]
		}
	}
	if ch == nil {
		kit.Empty(c, icons.Network, i18n.T(c, "git_diff_empty"), "", nil)
		return
	}
	ui.Scroll(c.Key(ch.Path)).TrackScroll(scroll(&v.diff, own)).Grow(1).Padding(12).Gap(12).Children(func() {
		for i, h := range ch.Hunks {
			id := fmt.Sprintf("%s#%d", ch.Path, i)
			ui.Column(c.Key(i)).Radius(p.RadiusLg).Border(1, p.Border).Clip().Children(func() {
				ui.Row(c).Padding(6, 8, 6, 12).Gap(4).Background(p.Muted).Children(func() {
					ui.Text(c, h.Header).FontSize(12).Font("monospace").SingleLine().Grow(1).Shrink(1).TextColor(p.MutedForeground)
					if v.accepted[id] {
						kit.Badge(c, i18n.T(c, "hunks_state_accepted"), kit.Positive)
						return
					}
					// Reject and Stage write the file and the index.
					reject := i18n.T(c, "hunks_reject")
					kit.Ghost(c.Key("reject"), reject, false).Height(24).PaddingX(8).Gap(4).Children(func() {
						ui.Icon(c, icons.Undo).FontSize(14)
						ui.Text(c, reject).FontSize(12)
					}).OnClick(func() { notice(c) })
					stage := i18n.T(c, "hunks_stage")
					kit.Ghost(c.Key("stage"), stage, false).Height(24).PaddingX(8).Children(func() {
						ui.Text(c, stage).FontSize(12)
					}).OnClick(func() { notice(c) })
					accept := i18n.T(c, "hunks_accept")
					kit.Primary(c.Key("accept"), accept, false).Height(24).PaddingX(8).Gap(4).Children(func() {
						ui.Icon(c, icons.Check).FontSize(14)
						ui.Text(c, accept).FontSize(12)
					}).OnClick(func() {
						v.accepted[id] = true
						kit.Toast(c, kit.Done, i18n.T(c, "hunks_accepted"))
					})
				})
				for j, l := range h.Lines {
					row := ui.Row(c.Key(j)).PaddingEnd(8)
					mark := " "
					switch l.Kind {
					case preview.Added:
						row.Background(p.Success.Alpha(0.12))
						mark = "+"
					case preview.Removed:
						row.Background(p.Danger.Alpha(0.12))
						mark = "-"
					}
					row.Children(func() {
						num := func(n int) {
							t := ""
							if n > 0 {
								t = fmt.Sprint(n)
							}
							ui.Text(c, t).FontSize(12).Font("monospace").Width(36).PaddingEnd(6).TextAlign(ui.End).Shrink(0).
								TextColor(p.MutedForeground.Alpha(0.6))
						}
						num(l.Old)
						num(l.New)
						ui.Text(c, mark+" "+l.Text).FontSize(12).Font("monospace").PreserveSpaces().Grow(1)
					})
				}
			})
		}
	})
	status(c, ch.Path, func() {
		ui.Textf(c, "+%d", ch.Added).FontSize(12).TextColor(p.Success)
		ui.Textf(c, "−%d", ch.Removed).FontSize(12).TextColor(p.Danger)
	})
}

// history is the turns' checkpoints, newest first: what each turn changed
// and Undo changes, which would restore the files.
func (s *State) history(c *ui.Context) {
	p := theme.Of(c)
	ui.Scroll(c.Key("checkpoints")).Grow(1).Padding(12).Gap(8).Children(func() {
		for _, cp := range s.Data.Checkpoints {
			ui.Column(c.Key(cp.ID)).Padding(12).Gap(8).Radius(p.RadiusLg).Border(1, p.Border).Background(p.Surface).Children(func() {
				ui.Row(c).Gap(8).Children(func() {
					ui.Text(c, cp.Turn).FontSize(14).FontWeight(500).SingleLine().Grow(1).Shrink(1)
					ui.Text(c, ago(c, cp.Time)).FontSize(12).TextColor(p.MutedForeground)
				})
				ui.Column(c).Gap(2).Children(func() {
					for _, f := range cp.Files {
						ui.Text(c, f).FontSize(12).Font("monospace").SingleLine().TextColor(p.MutedForeground)
					}
				})
				ui.Row(c).Gap(8).Children(func() {
					ui.Textf(c, "+%d", cp.Added).FontSize(12).TextColor(p.Success)
					ui.Textf(c, "−%d", cp.Removed).FontSize(12).TextColor(p.Danger)
					ui.Spacer(c)
					undo := i18n.T(c, "turn_changes_undo_all")
					kit.Ghost(c.Key("undo"), undo, false).Height(28).PaddingX(8).Gap(6).Border(1, p.Border).Children(func() {
						ui.Icon(c, icons.Restore).FontSize(14)
						ui.Text(c, undo).FontSize(14)
					}).OnClick(func() { notice(c) })
				})
			})
		}
	})
}

// ago is the activity centre's age: just now, minutes, hours, days.
func ago(c *ui.Context, t interface{ Unix() int64 }) string {
	d := preview.Now.Unix() - t.Unix()
	switch {
	case d < 60:
		return i18n.T(c, "notifications_time_now")
	case d < 3600:
		return i18n.T(c, "notifications_time_minutes", "count", d/60)
	case d < 86400:
		return i18n.T(c, "notifications_time_hours", "count", d/3600)
	}
	return i18n.T(c, "notifications_time_days", "count", d/86400)
}

// documents is the session's documents: a row each over the one shown,
// rendered.
func (s *State) documents(c *ui.Context) {
	p := theme.Of(c)
	toolbar(c, "docs", func() {
		for _, d := range s.Data.Documents {
			b := kit.Ghost(c.Key(d.ID), d.Title, d.ID == s.doc).Height(28).PaddingX(8).Gap(6)
			b.Children(func() {
				ui.Icon(c, icons.FileText).FontSize(14).TextColor(p.MutedForeground)
				ui.Text(c, d.Title).FontSize(14).SingleLine()
			}).OnClick(func() { s.doc = d.ID })
		}
	})
	for _, d := range s.Data.Documents {
		if d.ID != s.doc {
			continue
		}
		ui.Scroll(c.Key(d.ID)).Grow(1).Padding(16).Children(func() { markdown.View(c, d.Text, nil) })
		status(c, d.Path, nil)
	}
}

// browser is the browser's chrome over its placeholder: back, forward,
// reload and the address, which would load pages.
func browser(c *ui.Context, s *State) {
	p := theme.Of(c)
	toolbar(c, "browser", func() {
		kit.IconButton(c.Key("back"), icons.ArrowLeft, i18n.T(c, "browser_back"), 28, false).Disabled(true)
		kit.IconButton(c.Key("forward"), icons.ArrowRight, i18n.T(c, "browser_forward"), 28, false).Disabled(true)
		kit.IconButton(c.Key("reload"), icons.Reload, i18n.T(c, "browser_reload"), 28, false).OnClick(func() { notice(c) })
		ui.TextInput(c.Key("address"), &s.address).Placeholder(i18n.T(c, "browser_address")).Label(i18n.T(c, "browser_address")).
			Grow(1).Height(28).OnSubmit(func() { notice(c) })
		kit.IconButton(c.Key("new"), icons.Plus, i18n.T(c, "browser_new_tab"), 28, false).OnClick(func() { notice(c) })
	})
	placeholder(c, icons.Web, i18n.T(c, "resource_browser_preview"), p.Background)
}

// terminal is the terminal's chrome over its placeholder.
func terminal(c *ui.Context) {
	p := theme.Of(c)
	toolbar(c, "terminal", func() {
		ui.Icon(c, icons.Terminal).FontSize(14).TextColor(p.MutedForeground)
		ui.Text(c, i18n.T(c, "terminal")).FontSize(14).Grow(1)
		kit.IconButton(c.Key("new"), icons.Plus, i18n.T(c, "workspace_new_terminal"), 28, false).OnClick(func() { notice(c) })
	})
	placeholder(c, icons.Terminal, i18n.T(c, "resource_terminal_preview"), p.Muted)
}

// placeholder fills a resource that does not render in the preview: its
// icon and what it would do.
func placeholder(c *ui.Context, icon *ui.SVG, text string, fill ui.Color) {
	p := theme.Of(c)
	ui.Column(c.Key("placeholder")).Grow(1).Background(fill).Center().Gap(8).Padding(24).Children(func() {
		ui.Icon(c, icon).FontSize(32).TextColor(p.MutedForeground)
		ui.Text(c, text).FontSize(14).TextAlign(ui.Center).TextColor(p.MutedForeground)
	})
}
