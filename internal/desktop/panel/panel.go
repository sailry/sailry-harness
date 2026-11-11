package panel

import (
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Panel layout metrics.
const (
	barHeight = 40
	tabHeight = 28
	tabMax    = 180
	rowHeight = 28
	footer    = 32
)

// notice tells that an action would leave the preview: write files, run
// git, a terminal or load a page.
func notice(c *ui.Context) { kit.Message(c, i18n.T(c, "preview_notice")) }

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
	if pr != nil && t.Kind != Files && t.Kind != Changes {
		pr.promptDialog(c)
	}
	own := s.owns(t)
	ui.Column(c.Key(t)).Grow(1).MinHeight(0).Children(func() {
		if s.Live && t.Kind != Terminal && t.Kind != Files && t.Kind != Changes && t.Kind != History && t.Kind != Documents && t.Kind != Artifact && (t.Kind != Browser || s.BrowserActions == nil) {
			key := "unavailable"
			if (t.Kind == Files || t.Kind == Changes) && !s.HasProject {
				key = "resource_project_empty"
			}
			kit.Result(c, kit.ResultProps{Icon: ResourceOf(t.Kind).Icon, Title: i18n.T(c, key)}).Grow(1).Justify(ui.Center)
			return
		}
		switch t.Kind {
		case Files, Changes:
			// The whole component for this tab's item: the side list
			// opens or focuses another item's tab.
			if pr != nil {
				pr.reveal = func(path string) { s.Show(Tab{Files, path}) }
			}
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
			icon, label := itemLook(c, t.Kind, t.ID)
			if t.Kind == Files && s.pr != nil {
				label = s.pr.fileTabLabel(t.ID, label)
			}
			if t.Kind == Changes && s.pr != nil {
				label = s.pr.gitDocumentLabel(t.ID, label)
			}
			return icon, label
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

// Title is a tab's title, as its tab shows it.
func (s *State) Title(c *ui.Context, t Tab) string {
	_, label := s.look(c, t)
	return label
}

// itemLook is a file or diff tab's icon and title: the file's name, or
// All changes.
func itemLook(c *ui.Context, k Kind, path string) (*ui.SVG, string) {
	icon := icons.File
	if k == Changes {
		icon = icons.Network
		if path == allChanges {
			return icon, i18n.T(c, "git_all_changes")
		}
	}
	return icon, path[strings.LastIndex(path, "/")+1:]
}

// Page builds a project resource, Files or Changes, as an independent
// page in the main column: the page host, over the same Project the tabs
// show. A strip of the open items heads its main area; the item shown
// last opens with the page. files goes to the Files page, where Git's
// Open source shows a file.
func Page(c *ui.Context, k Kind, pr *Project, files func()) {
	ui.Column(c.Key(k)).Grow(1).MinHeight(0).Children(func() {
		if pr != nil && pr.shown(k) != "" {
			pr.show(k, pr.shown(k))
		}
		if pr != nil {
			pr.reveal = func(string) { files() }
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
					icon, label := itemLook(c, k, path)
					if k == Files {
						label = pr.fileTabLabel(path, label)
					}
					if k == Changes {
						label = pr.gitDocumentLabel(path, label)
					}
					tab(c, path, icon, label, path == pr.shown(k), nil, func() { pr.show(k, path) }, func() { closed = path })
				}
				if closed != "" {
					pr.close(k, closed)
				}
			})
		})
	})
}

// tab is a strip's tab: 28 DIPs up to 180 wide, the icon and the title, the
// active one in tab_active, its close button showing on hover or on the
// active tab. A tab with drag drags it, wearing the drag preview's look while
// it does.
func tab(c *ui.Context, key any, icon *ui.SVG, label string, active bool, drag *panes.Drag, show, close func()) {
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
	if drag != nil {
		b.Drag(*drag)
		if kit.DragChip(c, b, label) {
			return
		}
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
	if k == Files && path != "" {
		pr.readFile(path)
	} else if k == Changes && path != "" && path != allChanges && !pr.gitDocumentKnown(path) {
		pr.readDiff(path, pr.Git.scope)
	}
	main := func() {
		if head != nil {
			head()
		}
		switch {
		case k == Files:
			pr.filePreview(c, path, own)
		case path == allChanges:
			pr.combined(c)
		default:
			pr.diff(c, path)
		}
	}
	pr.promptDialog(c)
	if k == Files {
		split(c, "files", &pr.Files.side, main, func() { pr.tree(c, own, open) })
		return
	}
	box := ui.Column(c.Key("git")).Grow(1).MinHeight(0)
	pr.gitKeys(c, box)
	box.Children(func() {
		split(c, "changes", &pr.Git.side, main, func() { pr.gitDetails(c, own, open) })
	})
}

// scroll is the project's ScrollState s for the host that owns it, nil
// for the other.
func scroll(s *ui.ScrollState, own bool) *ui.ScrollState {
	if !own {
		return nil
	}
	return s
}

// strip is the tab row: every open item is a 28 DIP tab up to 180 wide with
// its icon and title, the active one in tab_active, a close button showing on
// hover; it scrolls sideways, and past four tabs a chevron lists them all.
// "+" opens another resource or document.
func (s *State) strip(c *ui.Context, pr *Project, keys Shortcut) {
	p := theme.Of(c)
	ui.Row(c.Key("strip")).Height(barHeight).Shrink(0).PaddingX(8).Gap(4).BorderWidth(0, 0, 1, 0).BorderColor(p.Border).Children(func() {
		ui.ScrollHorizontal(c.Key("tabs")).Grow(1).Shrink(1).MinWidth(0).Scrollbars(false).Children(func() {
			ui.Row(c).Gap(4).Height(barHeight).Children(func() {
				var closed *Tab
				for _, t := range s.Tabs {
					icon, label := s.look(c, t)
					var drag *panes.Drag
					if t.Kind == Terminal {
						// A terminal drags onto the workspace to open in a split.
						drag = &panes.Drag{Target: panes.Target{Kind: panes.Terminal, ID: t.ID}, Title: label}
					}
					tab(c, t, icon, label, t == s.Active, drag, func() { s.Show(t) }, func() { closed = &t })
				}
				if closed != nil {
					s.Close(*closed)
				}
			})
		})
		if len(s.Tabs) > Overflow {
			// Every tab with a check on the one shown.
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

// launcher lists the resources to open: the "details" title in an unbordered
// header as tall as the tab strip, then full-width 40 DIP rows, the icon, the
// label and the shortcut, up to 440 wide and centered across. The rows start
// under the header instead of the reference's vertical centering.
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
func status(c *ui.Context, path string, after func(), system ...func()) {
	p := theme.Of(c)
	ui.Row(c.Key("footer")).Height(footer).Shrink(0).PaddingX(12).Gap(12).BorderWidth(1, 0, 0, 0).BorderColor(p.Border).
		TextColor(p.MutedForeground).Children(func() {
		ui.Text(c, path).FontSize(12).SingleLine().Grow(1).Shrink(1).ContextMenu(func(m *ui.Menu) {
			if m.Item(i18n.T(c, "files_copy_path")).Chosen() {
				c.WriteClipboard(path)
			}
			if len(system) > 0 && system[0] != nil && m.Item(i18n.T(c, "files_open_system")).Chosen() {
				system[0]()
			}
		})
		if after != nil {
			after()
		}
	})
}

func findFile(n *preview.FileNode, path string) *preview.FileNode {
	if n == nil {
		return nil
	}
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
	if pr.Actions != nil {
		pr.liveFileEditor(c, path, own)
		return
	}
	f := findFile(pr.Data.Files, path)
	if f == nil {
		kit.Result(c, kit.ResultProps{Icon: icons.File, Title: i18n.T(c, "file_empty")})
		return
	}
	v := pr.Files.item(path)
	if pr.Actions != nil && !v.loaded {
		pr.SetFile(path, f.Text)
	}
	md := strings.HasSuffix(f.Name, ".md")
	toolbar(c, "toolbar", func() {
		kit.IconButton(c.Key("save"), icons.Save, i18n.T(c, "files_save"), 28, false).Disabled(pr.Actions == nil || !v.dirty || !v.writable).OnClick(func() {
			if pr.Actions != nil && pr.Actions.SaveFile != nil {
				text, _ := pr.FileDraft(path)
				pr.Actions.SaveFile(c, path, text)
			}
		})
		separator(c)
		kit.IconButton(c.Key("undo"), icons.Undo, i18n.T(c, "files_undo"), 28, false).Disabled(v.editor == nil || !v.editor.Buffer.CanUndo()).OnClick(func() {
			if v.editor != nil && v.editor.Buffer.Undo() {
				pr.FileChanged(path, v.editor.Buffer.String())
			}
		})
		kit.IconButton(c.Key("redo"), icons.Redo, i18n.T(c, "files_redo"), 28, false).Disabled(v.editor == nil || !v.editor.Buffer.CanRedo()).OnClick(func() {
			if v.editor != nil && v.editor.Buffer.Redo() {
				pr.FileChanged(path, v.editor.Buffer.String())
			}
		})
		separator(c)
		kit.IconButton(c.Key("find"), icons.Search, i18n.T(c, "files_find"), 28, false).OnClick(func() {
			if v.editor != nil {
				v.editor.Find("")
			} else {
				notice(c)
			}
		})
		ui.Spacer(c)
		if md {
			kit.Segmented(c, "view", &v.source, []string{i18n.T(c, "content_document"), i18n.T(c, "content_source")}, kit.SegmentedProps{})
		}
	})
	ui.Scroll(c.Key("text")).TrackScroll(scroll(&v.text, own)).Grow(1).Padding(12, 0).Children(func() {
		if md && v.source == 0 {
			text := f.Text
			if pr.Actions != nil && v.loaded {
				text = v.draft + v.suffix
			}
			ui.Column(c).PaddingX(16).Children(func() { kit.Markdown(c, text, kit.MarkdownProps{}) })
			return
		}
		text := f.Text
		options := kit.CodeOptions{Language: editor.LanguageFor(f.Path), Numbers: true, Editable: true}
		if pr.Actions != nil {
			options.Editable = v.writable
			text = v.draft
			options.Changed = func(text string) { pr.FileChanged(path, text) }
			options.Bind = func(state *editor.State) { pr.bindEditor(path, state) }
		}
		kit.Code(c, ui.Column(c.Key(f.Path)), text, options)
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

// diff is a changed file in MyGo's unified diff viewer, with
// content diff view, with the change under the pointer's Revert and Keep
// (editor.DefaultHunkActions) and Stage: in the preview each toasts, as
// they would write the file and the index.
func (pr *Project) diff(c *ui.Context, path string) {
	if pr.gitDocument(c, path) {
		return
	}
	if pr.gitEmpty(c, false) {
		return
	}
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
	metadata := pr.Git.diffStates[pr.Git.scope+"/"+path]
	if metadata.Binary {
		ui.Text(c, i18n.T(c, "git_diff_binary")).FontSize(14).Padding(12).TextColor(p.MutedForeground)
		return
	}
	old, new := ch.Texts()
	box := ui.Column(c.Key(ch.Path)).Grow(1).MinHeight(0)
	selected := -1
	options := kit.DiffOptions{Language: editor.LanguageFor(ch.Path),
		Actions: func(c *ui.Context, bar ui.Element, a *editor.DiffHunkActions) {
			selected = diffHunkIndex(*ch, a.Hunk)
			bar.Row().Gap(4)
			bar.Children(func() {
				editor.DefaultHunkActions(c, ui.Row(c.Key("hunk")), a)
				stage := i18n.T(c, "hunks_stage")
				kit.Ghost(c.Key("stage"), stage, false).Height(20).PaddingX(8).Children(func() {
					ui.Text(c, stage).FontSize(12)
				}).OnClick(func() {
					if pr.Actions != nil && pr.Actions.Hunk != nil {
						pr.Actions.Hunk(c, ch.Path, selected, "stage")
					} else {
						notice(c)
					}
				})
			})
		}}
	if pr.Actions != nil && pr.Actions.Hunk != nil {
		options.HunkChanged = func(_ int, keep bool) {
			action := "reject"
			if keep {
				action = "keep"
			}
			pr.Actions.Hunk(c, ch.Path, selected, action)
		}
	} else {
		options.Changed = func() { notice(c) }
	}
	kit.Diff(c, box, old, new, options)
	if metadata.Partial {
		ui.Text(c, i18n.T(c, "git_diff_partial")).FontSize(12).Padding(8, 12).TextColor(p.MutedForeground)
	}
	status(c, ch.Path, func() {
		ui.Textf(c, "+%d", ch.Added).FontSize(12).TextColor(p.Success)
		ui.Textf(c, "−%d", ch.Removed).FontSize(12).TextColor(p.Danger)
	}, func() { pr.fileAction(c, FileAction{Kind: "files_open_system", Paths: []string{ch.Path}}) })
}

func diffHunkIndex(change preview.FileChange, native editor.DiffHunk) int {
	index := -1
	for i, hunk := range change.Hunks {
		cursor := 0
		if fields := strings.Fields(hunk.Header); len(fields) >= 3 {
			cursor, _ = strconv.Atoi(strings.SplitN(strings.TrimPrefix(fields[2], "+"), ",", 2)[0])
			cursor = max(0, cursor-1)
		}
		for _, line := range hunk.Lines {
			if line.Kind == preview.Added && line.New > native.NewStart && line.New <= native.NewEnd || line.Kind == preview.Removed && native.NewStart == native.NewEnd && cursor == native.NewStart {
				if index >= 0 && index != i {
					return -1
				}
				index = i
			}
			if line.Kind != preview.Removed {
				cursor = line.New
			}
		}
	}
	return index
}

// history is the turns' checkpoints, newest first: what each turn changed
// and Undo changes, which would restore the files.
func (s *State) history(c *ui.Context) {
	p := theme.Of(c)
	// The fixture's checkpoints are as old as its own clock says.
	now := preview.Now
	if s.Live {
		now = c.Now()
	}
	ui.Scroll(c.Key("checkpoints")).Grow(1).Padding(12).Gap(8).Children(func() {
		for _, cp := range s.Data.Checkpoints {
			kit.Frost(c, ui.Column(c.Key(cp.ID)).Padding(12).Gap(8).Radius(p.RadiusLg).Border(1, p.Border), p.Surface, kit.CardTint).Children(func() {
				ui.Row(c).Gap(8).Children(func() {
					ui.Text(c, cp.Turn).FontSize(14).FontWeight(500).SingleLine().Grow(1).Shrink(1)
					ui.Text(c, ago(c, cp.Time, now)).FontSize(12).TextColor(p.MutedForeground)
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
					}).OnClick(func() {
						if s.Actions != nil && s.Actions.Restore != nil {
							actions := s.Actions
							s.historyConfirm = confirm{open: true, title: undo, message: i18n.T(c, "workspace_restore_confirm"), target: cp.Turn, run: func(c *ui.Context) { actions.Restore(c, cp.ID) }}
						} else {
							notice(c)
						}
					})
				})
			})
		}
	})
	if confirmation := &s.historyConfirm; confirmation.open {
		if kit.AlertDialog(c.Key("checkpoint-confirm"), &confirmation.open, confirmation.title, confirmation.message+"\n"+confirmation.target, i18n.T(c, "settings_cancel"), confirmation.title) == 1 && confirmation.run != nil {
			confirmation.run(c)
		}
	}
}

// ago is the activity centre's age: just now, minutes, hours, days.
func ago(c *ui.Context, t, now time.Time) string {
	d := now.Unix() - t.Unix()
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
	if s.Actions != nil && s.Actions.ReadDocument != nil {
		s.Actions.ReadDocument(id)
	}
	for _, d := range s.Data.Documents {
		if d.ID != id {
			continue
		}
		if !s.liveDocument(c, d.ID) {
			ui.Scroll(c.Key(d.ID)).Grow(1).Padding(16).Children(func() { kit.Markdown(c, d.Text, kit.MarkdownProps{}) })
		}
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
