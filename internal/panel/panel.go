package panel

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/sailry/sailry-go/internal/conversation"
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

// View builds the panel, the tab host: one flat strip of tabs, each an
// open item, over the active one, or the launcher. pr is the shown
// project's state, nil when the conversation has none; keys writes the
// launcher's shortcuts.
func View(c *ui.Context, s *State, pr *Project, keys Shortcut) {
	s.Sync(pr)
	if len(s.Tabs) == 0 {
		launcher(c, s, keys)
		return
	}
	s.strip(c, pr, keys)
	t := s.Active
	own := s.owns(t)
	ui.Column(c.Key(t)).Grow(1).MinHeight(0).Children(func() {
		switch t.Kind {
		case Files, Changes:
			// The whole component for this tab's item: the side list
			// opens or focuses another item's tab.
			pr.view(c, t.Kind, t.ID, own, nil, func(path string) { s.Show(Tab{t.Kind, path}) })
		case History:
			s.history(c)
		case Documents:
			s.document(c, t.ID)
		case Artifact:
			s.artifact(c, t.ID)
		case Subagent:
			if s.Child != nil {
				s.Child(c, t.ID)
			}
		case Browser:
			s.browser(c, t)
		case Terminal:
			s.terminal(c, t)
		}
	})
}

// look is a tab's icon and title.
func (s *State) look(c *ui.Context, t Tab) (*ui.SVG, string) {
	switch t.Kind {
	case Artifact:
		if f, ok := s.artifacts[t.ID]; ok {
			return conversation.FileIcon(f), f.Name()
		}
	case Subagent:
		title := s.children[t.ID]
		if title == "" {
			title = i18n.T(c, "chat_child")
		}
		return icons.Bot, title
	case Files, Changes:
		if t.ID != "" {
			return itemLook(t.Kind, t.ID)
		}
	case Documents:
		for _, d := range s.Data.Documents {
			if d.ID == t.ID {
				return icons.FileText, d.Title
			}
		}
	}
	r := ResourceOf(t.Kind)
	return r.Icon, i18n.T(c, r.Label)
}

// itemLook is a file or diff tab's icon and title: the file's name.
func itemLook(k Kind, path string) (*ui.SVG, string) {
	icon := icons.File
	if k == Changes {
		icon = icons.Network
	}
	return icon, path[strings.LastIndex(path, "/")+1:]
}

// Page builds a project resource, Files or Changes, as an independent
// page in the main column: the page host, over the same Project the tabs
// show. A strip of the open items heads its main area; the item shown
// last opens with the page.
func Page(c *ui.Context, k Kind, pr *Project) {
	ui.Column(c.Key(k)).Grow(1).MinHeight(0).Children(func() {
		if pr != nil && pr.shown(k) != "" {
			pr.show(k, pr.shown(k))
		}
		pr.view(c, k, pr.shownOr(k), true, func() { pr.strip(c, k) }, func(path string) { pr.show(k, path) })
	})
}

// shownOr is the item shown, "" without a project.
func (pr *Project) shownOr(k Kind) string {
	if pr == nil {
		return ""
	}
	return pr.shown(k)
}

// strip is the page's row of open items, as the panel's strip draws them.
func (pr *Project) strip(c *ui.Context, k Kind) {
	p := theme.Of(c)
	ui.Row(c.Key("items")).Height(barHeight).Shrink(0).PaddingX(8).BorderWidth(0, 0, 1, 0).BorderColor(p.Border).Children(func() {
		ui.ScrollHorizontal(c.Key("tabs")).Grow(1).Shrink(1).MinWidth(0).Scrollbars(false).Children(func() {
			ui.Row(c).Gap(4).Height(barHeight).Children(func() {
				closed := ""
				for _, path := range pr.items(k) {
					icon, label := itemLook(k, path)
					tab(c, path, icon, label, path == pr.shown(k), func() { pr.show(k, path) }, func() { closed = path })
				}
				if closed != "" {
					pr.close(k, closed)
				}
			})
		})
	})
}

// tab is a strip's tab (tabs.rs): 28 DIPs up to 180 wide, the icon and
// the title, the active one in tab_active, its close button showing on
// hover or on the active tab.
func tab(c *ui.Context, key any, icon *ui.SVG, label string, active bool, show, close func()) {
	p := theme.Of(c)
	b := ui.ButtonBase(c.Key(key)).Label(label).Height(tabHeight).MaxWidth(tabMax).PaddingStart(8).PaddingEnd(4).
		Gap(6).Radius(p.Radius).Cursor(ui.CursorPointer).TextColor(p.MutedForeground)
	hovered := b.Hovered()
	switch {
	case active:
		b.Background(p.TabActive).TextColor(p.TabActiveText).AccessSelected(true).ScrollIntoView()
	case hovered:
		b.TextColor(p.Foreground)
	}
	b.Children(func() {
		ui.Icon(c, icon).FontSize(14).Shrink(0)
		ui.Text(c, label).FontSize(14).SingleLine().Shrink(1)
		x := kit.IconButton(c.Key("close"), icons.Close, i18n.T(c, "close"), 20, false).OnClick(close)
		if !hovered && !active && !x.FocusVisible() {
			x.Opacity(0)
		}
	}).OnClick(show)
}

// view is the Files or Git component in either host for the item path,
// or the project prompt when there is no project: head heads the main
// area (the page's strip), and the side list's choice calls open. The
// host that owns the scrolling keeps it in the project; while the page
// and the closing panel both show a resource, the page owns it, as MyGo
// binds a ScrollState to one container per build.
func (pr *Project) view(c *ui.Context, k Kind, path string, own bool, head func(), open func(path string)) {
	if pr == nil {
		kit.Result(c, kit.ResultProps{Icon: ResourceOf(k).Icon, Title: i18n.T(c, "resource_project_empty")}).Grow(1).Justify(ui.Center)
		return
	}
	main := func() {
		if head != nil {
			head()
		}
		if k == Files {
			pr.filePreview(c, path, own)
		} else {
			pr.diff(c, path)
		}
	}
	if k == Files {
		split(c, "files", &pr.Files.side, main, func() { pr.tree(c, path, own, open) })
	} else {
		split(c, "changes", &pr.Git.side, main, func() { pr.changeList(c, path, own, open) })
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

// strip is the tab row (tabs.rs): every open item is a 28 DIP tab up to
// 180 wide with its icon and title, the active one in tab_active, a close
// button showing on hover; it scrolls sideways, and past four tabs a
// chevron lists them all. "+" opens another resource or document.
func (s *State) strip(c *ui.Context, pr *Project, keys Shortcut) {
	p := theme.Of(c)
	ui.Row(c.Key("strip")).Height(barHeight).Shrink(0).PaddingX(8).Gap(4).BorderWidth(0, 0, 1, 0).BorderColor(p.Border).Children(func() {
		ui.ScrollHorizontal(c.Key("tabs")).Grow(1).Shrink(1).MinWidth(0).Scrollbars(false).Children(func() {
			ui.Row(c).Gap(4).Height(barHeight).Children(func() {
				var closed *Tab
				for _, t := range s.Tabs {
					icon, label := s.look(c, t)
					tab(c, t, icon, label, t == s.Active, func() { s.Show(t) }, func() { closed = &t })
				}
				if closed != nil {
					s.Close(*closed)
				}
			})
		})
		if len(s.Tabs) > Overflow {
			// Every tab with a check on the one shown (navigation_tabs.rs).
			kit.MenuButton(c.Key("all"), "panel-tabs", func(b ui.Element) {
				kit.IconLook(c, b, icons.ChevronDown, i18n.T(c, "navigation_tabs"), "", 28, false)
			}, func(m *kit.Menu) {
				for _, t := range s.Tabs {
					_, label := s.look(c, t)
					if m.Item(label).Checked(t == s.Active).Chosen() {
						s.Show(t)
					}
				}
			})
		}
		kit.MenuButton(c.Key("add"), "panel-add", func(b ui.Element) {
			kit.IconLook(c, b, icons.Plus, i18n.T(c, "details"), "", 28, false)
		}, func(m *kit.Menu) {
			for _, r := range Resources {
				if r.Kind == Documents {
					for _, d := range s.Data.Documents {
						t := Tab{Documents, d.ID}
						if m.Item(d.Title).Icon(icons.FileText).Checked(t == s.Active).Chosen() {
							s.Show(t)
						}
					}
					continue
				}
				it := m.Item(i18n.T(c, r.Label)).Icon(r.Icon).Checked(r.Kind == s.Active.Kind)
				if r.Key != 0 && keys != nil {
					it.Shortcut(keys(r.Mods, r.Key))
				}
				if it.Chosen() {
					s.Open(r.Kind)
				}
			}
		}, kit.MenuOptions{CheckRight: true})
	})
}

// launcher lists the resources to open (launcher.rs): the "details"
// title in an unbordered header as tall as the tab strip, then full-width
// 40 DIP rows, the icon, the label and the shortcut, up to 440 wide and
// centered across. The rows start under the header instead of the
// reference's vertical centering.
func launcher(c *ui.Context, s *State, keys Shortcut) {
	p := theme.Of(c)
	ui.Column(c.Key("launcher")).Grow(1).MinHeight(0).Children(func() {
		ui.Row(c.Key("launcher-header")).Height(barHeight).Shrink(0).PaddingX(12).Children(func() {
			ui.Text(c, i18n.T(c, "details")).FontSize(14).SingleLine().Grow(1).Shrink(1)
		})
		ui.Column(c.Key("launcher-rows")).FillWidth().MaxWidth(440+2*24).Padding(0, 24, 24, 24).Gap(8).AlignSelf(ui.Center).Children(func() {
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

// tree is the project's file tree, starting at preview_files.rs's p_2 with
// no title: 28 DIP rows indented 16 per level from 8. A file's click
// calls open with its path; the file shown, shown, is selected.
func (pr *Project) tree(c *ui.Context, shown string, own bool, open func(path string)) {
	v := &pr.Files
	p := theme.Of(c)
	ui.Scroll(c.Key("scroll")).TrackScroll(scroll(&v.tree, own)).Grow(1).Padding(8).Children(func() {
		tree := ui.TreeBase(c)
		tree.Root.Children(func() {
			var node func(n *preview.FileNode)
			node = func(n *preview.FileNode) {
				c.Key(n.Path)
				var folder *bool
				if n.Children != nil {
					if v.folders[n.Path] == nil {
						o := true
						v.folders[n.Path] = &o
					}
					folder = v.folders[n.Path]
				}
				item := tree.Item(folder)
				row := item.Row.Height(rowHeight).PaddingStart(8 + 16*float32(item.Depth())).PaddingEnd(8).Gap(8).
					Radius(p.Radius).Cursor(ui.CursorPointer).Label(n.Name)
				selected := n.Path == shown
				text := p.MutedForeground
				switch {
				case selected:
					row.Background(p.Hover).AccessSelected(true)
					text = p.HoverForeground
				case row.Hovered():
					text = p.Foreground
				}
				if row.Clicked() {
					if folder != nil {
						*folder = !*folder
					} else {
						open(n.Path)
					}
				}
				row.TextColor(text).Children(func() {
					icon := icons.File
					if folder != nil {
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

// filePreview is an open file, read-only, in its own view: the toolbar
// (Save, Undo and Redo disabled, Find, Document | Source for markdown),
// the text with line numbers, and the status bar.
func (pr *Project) filePreview(c *ui.Context, path string, own bool) {
	f := findFile(pr.Data.Files, path)
	if f == nil {
		kit.Result(c, kit.ResultProps{Icon: icons.File, Title: i18n.T(c, "file_empty")})
		return
	}
	v := pr.Files.item(path)
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
			kit.Segmented(c, "view", &v.source, []string{i18n.T(c, "content_document"), i18n.T(c, "content_source")}, kit.SegmentedProps{})
		}
	})
	ui.Scroll(c.Key("text")).TrackScroll(scroll(&v.text, own)).Grow(1).Padding(12, 0).Children(func() {
		if md && v.source == 0 {
			ui.Column(c).PaddingX(16).Children(func() { kit.Markdown(c, f.Text, kit.MarkdownProps{}) })
			return
		}
		kit.Code(c, ui.Column(c.Key(f.Path)), f.Text, kit.CodeOptions{Language: editor.LanguageFor(f.Path), Numbers: true, Editable: true})
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

// changeList is the working tree's changes over the commit box. A
// change's click calls open with its path; the shown change is selected.
func (pr *Project) changeList(c *ui.Context, shown string, own bool, open func(path string)) {
	p := theme.Of(c)
	v := &pr.Git
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
			case ch.Path == shown:
				b.Background(p.Hover).TextColor(p.HoverForeground).AccessSelected(true)
			case b.Hovered():
				b.TextColor(p.Foreground)
			}
			kit.Tooltip(c, b, ch.Path)
			b.Children(func() {
				ui.Icon(c, icons.File).FontSize(16).TextColor(p.MutedForeground)
				ui.Text(c, name).FontSize(14).SingleLine().Grow(1).Shrink(1)
				ui.Text(c, ch.Status).FontSize(12).FontWeight(600).TextColor(statusColor(p, ch.Status))
			}).OnClick(func() { open(ch.Path) })
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
}

// diff is a changed file in MyGo's diff viewer, unified as Rust's
// content diff view, with the change under the pointer's Revert and Keep
// (editor.DefaultHunkActions) and Stage: in the preview each toasts, as
// they would write the file and the index.
func (pr *Project) diff(c *ui.Context, path string) {
	p := theme.Of(c)
	var ch *preview.FileChange
	for i := range pr.Data.Changes {
		if pr.Data.Changes[i].Path == path {
			ch = &pr.Data.Changes[i]
		}
	}
	if ch == nil {
		kit.Result(c, kit.ResultProps{Icon: icons.Network, Title: i18n.T(c, "git_diff_empty")})
		return
	}
	old, new := ch.Texts()
	box := ui.Column(c.Key(ch.Path)).Grow(1).MinHeight(0)
	kit.Diff(c, box, old, new, kit.DiffOptions{Language: editor.LanguageFor(ch.Path), Changed: func() { notice(c) },
		Actions: func(c *ui.Context, bar ui.Element, a *editor.DiffHunkActions) {
			bar.Row().Gap(4)
			bar.Children(func() {
				editor.DefaultHunkActions(c, ui.Row(c.Key("hunk")), a)
				stage := i18n.T(c, "hunks_stage")
				kit.Ghost(c.Key("stage"), stage, false).Height(20).PaddingX(8).Children(func() {
					ui.Text(c, stage).FontSize(12)
				}).OnClick(func() { notice(c) })
			})
		}})
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
			kit.Frost(c, ui.Column(c.Key(cp.ID)).Padding(12).Gap(8).Radius(p.RadiusLg).Border(1, p.Border), p.Surface).Children(func() {
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

// document is a session document's tab: the rendered document over its
// path.
func (s *State) document(c *ui.Context, id string) {
	for _, d := range s.Data.Documents {
		if d.ID != id {
			continue
		}
		ui.Scroll(c.Key(d.ID)).Grow(1).Padding(16).Children(func() { kit.Markdown(c, d.Text, kit.MarkdownProps{}) })
		status(c, d.Path, nil)
	}
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
