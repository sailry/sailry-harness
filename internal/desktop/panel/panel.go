package panel

import (
	"slices"

	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Panel layout metrics.
const (
	barHeight = 40
	tabHeight = kit.StripTabHeight
	tabMax    = 180
	rowHeight = 28
	footer    = 32
)

// notice tells that an action would leave the preview: write files, run
// git, a terminal or load a page.
func notice(c *ui.Context) { kit.PreviewNotice(c) }

// Shortcut is a shortcut's text as menus and the launcher show it.
type Shortcut func(mods ui.Modifiers, key ui.Key) string

// View builds the panel's content, the tab host's body: the active tab's
// item, or the launcher. Its tabs are not a row of the panel: the window's
// header shows them over the panel (Strip). pr is the shown project's state,
// nil when the conversation has none; keys writes the launcher's shortcuts.
func View(c *ui.Context, s *State, pr *Project, keys Shortcut) {
	s.Sync(pr)
	s.nameDocument()
	hadTabs := len(s.Tabs) != 0
	s.confirmClose(c)
	if pr != nil && len(s.Tabs) != 0 {
		pr.ActionDialog(c)
	}
	if len(s.Tabs) == 0 {
		if !hadTabs {
			launcher(c, s, keys)
		}
		return
	}
	t := s.Active
	ui.Column(c.Key(t)).Grow(1).MinHeight(0).Children(func() {
		if resource, exists := s.resource(t.Kind); exists && resource.Render != nil {
			resource.Render(c, s, t)
			return
		}
		switch t.Kind {
		case Files, Changes:
			RenderProjectResource(c, s, t)
		case History:
			s.history(c)
		case Documents:
			s.document(c, t.ID)
		case Artifact:
			s.artifact(c, t.ID)
		case Subagent:
			if s.Child == nil {
				empty(c, icons.Bot, "chat_child_unavailable")
				return
			}
			s.Child(c, t.ID)
		case Browser:
			RenderBrowserResource(c, s, t)
		case Terminal:
			s.TerminalView(c, t.ID)
		}
	})
}

// confirmClose asks before a live terminal's tab closes, naming the terminal
// and what closing does once, as the split workspace does for a pane: the
// Node then ends the shell.
func (s *State) confirmClose(c *ui.Context) {
	t := s.closing
	if t == (Tab{}) {
		return
	}
	if !slices.Contains(s.Tabs, t) || s.TerminalActions == nil || s.TerminalActions.Close == nil {
		// Its shell exited, or its conversation was left, meanwhile.
		s.closing = Tab{}
		return
	}
	open := true
	if kit.Confirm(c.Key("close-terminal/"+t.ID), &open, kit.Confirmation{
		Title: i18n.T(c, "workspace_close_terminal"), Message: s.Title(c, t),
		Buttons: []string{i18n.T(c, "settings_cancel"), i18n.T(c, "close")}, Default: 1, Destructive: []int{1},
	}) == 1 {
		s.closing = Tab{}
		if s.OnTerminalClose != nil {
			s.OnTerminalClose(t)
		}
		s.TerminalActions.Close(t.ID)
	}
	if !open {
		s.closing = Tab{}
	}
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
	case Browser:
		// A live page names its tab once it has a title.
		if s.Live && s.BrowserActions != nil {
			if title := s.BrowserActions.Page(t.ID).Title; title != "" {
				return icons.Web, title
			}
		}
		// The dataset's own page names its tab too.
		if b := s.pagesOf[t.ID]; b != nil {
			if page := s.sample(b); page != nil {
				return icons.Web, page.Title
			}
		}
	}
	r, found := s.resource(t.Kind)
	if !found {
		r = ResourceOf(t.Kind)
	}
	return r.Icon, i18n.T(c, r.Label)
}

// busy reports whether a tab's content is loading: a live browser page.
func (s *State) busy(t Tab) bool {
	return t.Kind == Browser && s.Live && s.BrowserActions != nil && s.BrowserActions.Page(t.ID).Loading
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

// DocumentTabs draws the project's open file or diff tabs.
func (pr *Project) DocumentTabs(c *ui.Context, k Kind) {
	p := theme.Of(c)
	ui.Row(c.Key("items")).Height(barHeight).Shrink(0).PaddingX(8).BorderWidth(0, 0, 1, 0).BorderColor(p.Border).Children(func() {
		ui.ScrollHorizontal(c.Key("tabs")).Grow(1).Shrink(1).MinWidth(0).Scrollbars(false).Children(func() {
			ui.Row(c).Gap(4).Height(barHeight).Children(func() {
				for _, path := range pr.items(k) {
					icon, label := itemLook(c, k, path)
					if k == Files {
						label = pr.fileTabLabel(path, label)
					}
					if k == Changes {
						label = pr.gitDocumentLabel(path, label)
					}
					tab(c, path, tabLook{icon: icon, label: label, active: path == pr.shown(k), passing: pr.passes(k, path)},
						func() { pr.show(k, path) }, func() { pr.close(k, path) },
						func() { pr.keep(k, path) }, func() { pr.release(k, path) })
				}
			})
		})
	})
}

// tabLook is how a strip's tab shows: its icon and title, its tip, and
// whether it is the active one, busy, or only passing.
type tabLook struct {
	icon                  *ui.SVG
	label, tip            string
	active, busy, passing bool
}

// tab is a strip's tab: 28 DIPs up to 180 wide, the icon and the title, the
// active one in tab_active, its close button showing on hover. It works as
// the title band's tabs do: a passing tab is in italics until a double
// click keeps it, and its system menu keeps or stops keeping it and closes
// it. keep and release are nil for a tab that is always kept. show and
// close run after the build, as MyGo runs OnClick: they act themselves, as
// nothing set by them is read in the pass that built the tab.
func tab(c *ui.Context, key any, l tabLook, show, close, keep, release func()) {
	b, hovered := kit.StripTab(c, key, l.label, l.active, tabMax)
	// A tab names its item short; the tip is the whole, a file's path.
	if l.tip != "" && l.tip != l.label {
		kit.Tooltip(c, b, l.tip)
	}
	if l.passing {
		b.Italic()
		if b.DoubleClicked() && keep != nil {
			keep()
		}
	}
	b.ContextMenu(func(m *ui.Menu) {
		if keep != nil {
			if !l.passing {
				if m.Item(i18n.T(c, "workspace_tab_release")).Chosen() {
					release()
				}
			} else if m.Item(i18n.T(c, "workspace_tab_keep")).Chosen() {
				keep()
			}
			m.Separator()
		}
		if m.Item(i18n.T(c, "close")).Chosen() {
			close()
		}
	})
	kit.StripTabBody(c, b, l.icon, l.label, l.active, hovered, l.busy, i18n.T(c, "close"), close).OnClick(show)
}

// view is the Files or Git component in a panel tab for the item path,
// or the project prompt when there is no project. The side list's choice
// calls open. The host that owns the scrolling keeps it in the project;
// while a page and closing panel both show a resource, the page owns it,
// as MyGo binds a ScrollState to one container per build.
func (pr *Project) view(c *ui.Context, k Kind, path string, own bool, open, reveal func(path string)) {
	if pr == nil {
		kit.Result(c, kit.ResultProps{Icon: ResourceOf(k).Icon, Title: i18n.T(c, "resource_project_empty")}).Grow(1).Justify(ui.Center)
		return
	}
	if k == Files {
		pair(c, "files", &pr.Files.side, filesLeast,
			func(bool) { pr.FileEditor(c, path, own) }, func(bool) { pr.FileTree(c, own, open) })
		return
	}
	box := ui.Column(c.Key("git")).Grow(1).MinHeight(0)
	pr.GitKeys(c, box)
	box.Children(func() {
		pair(c, "changes", &pr.Git.side, gitLeast,
			func(narrow bool) { pr.gitDiff(c, path, narrow) },
			func(stacked bool) { pr.GitRepository(c, own, stacked, open, reveal) })
	})
	pr.GitDialogs(c)
}

// FileEditor shows a file using the project's shared editor state.
func (pr *Project) FileEditor(c *ui.Context, path string, own bool) {
	if path != "" {
		pr.readFile(path)
	}
	pr.filePreview(c, path, own)
}

// GitDiff renders the selected diff in the available document width.
func (pr *Project) GitDiff(c *ui.Context, path string, width float32) {
	pr.gitDiff(c, path, width < diffSplitMin)
}

func (pr *Project) gitDiff(c *ui.Context, path string, narrow bool) {
	if path == allChanges {
		pr.combined(c, narrow)
		return
	}
	if path != "" && !pr.gitDocumentKnown(path) {
		pr.readDiff(path, pr.Git.scope)
	}
	pr.diff(c, path, narrow)
}

// scroll is the project's ScrollState s for the host that owns it, nil
// for the other.
func scroll(s *ui.ScrollState, own bool) *ui.ScrollState {
	if !own {
		return nil
	}
	return s
}

// Strip builds the panel's tabs into the host's row, the header segment over
// the panel: every open item is a 28 DIP tab up to 180 wide with its icon and
// title, the active one in tab_active, a close button showing on hover; they
// scroll sideways, and past four tabs a chevron lists them all. "+" opens
// another resource or document. The tabs take only their own width, so the
// row's gap before the chevron and "+" stays the host's. "+" lists what the
// launcher lists, with the same shortcuts. Without a tab it builds nothing:
// the launcher lists the resources.
func Strip(c *ui.Context, s *State, pr *Project, keys Shortcut) {
	s.Sync(pr)
	if len(s.Tabs) == 0 {
		return
	}
	ui.ScrollHorizontal(c.Key("tabs")).Shrink(1).MinWidth(0).Scrollbars(false).Children(func() {
		ui.Row(c).Gap(kit.StripGap).Height(barHeight).AlignItems(ui.Center).Children(func() {
			for i, t := range s.Tabs {
				if i > 0 {
					kit.StripDivider(c, "divider-"+string(t.Kind)+t.ID)
				}
				icon, label := s.look(c, t)
				tip := ""
				if f, ok := s.artifacts[t.ID]; ok && t.Kind == Artifact {
					tip = f.Path
				}
				keep, release := s.keeper(t)
				tab(c, t, tabLook{icon: icon, label: label, tip: tip, active: t == s.Active, busy: s.busy(t), passing: s.passes(t)},
					func() { s.Show(t) }, func() {
						// The button that closed is in the header: the focus
						// goes to the panel, where ⌘W closes the next tab.
						s.Close(t)
						if s.Focus != nil {
							s.Focus(c)
						}
					}, keep, release)
			}
		})
	})
	ui.Spacer(c)
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
		for _, r := range s.ResourceList() {
			if !s.lists(r.Kind) {
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
}

// launcher lists the resources to open: full-width 40 DIP rows, the icon,
// the label and the shortcut, up to 440 wide and centered both ways.
func launcher(c *ui.Context, s *State, keys Shortcut) {
	p := theme.Of(c)
	ui.Column(c.Key("launcher")).Grow(1).MinHeight(0).Justify(ui.Center).Children(func() {
		ui.Column(c.Key("launcher-rows")).FillWidth().MaxWidth(440 + 2*24).Padding(24).Gap(8).AlignSelf(ui.Center).Children(func() {
			for _, r := range s.ResourceList() {
				label := i18n.T(c, r.Label)
				if !s.lists(r.Kind) {
					continue
				}
				b := ui.ButtonBase(c.Key(r.Kind)).Label(label).FillWidth().Height(40).PaddingX(12).Gap(10).Radius(p.Radius).
					Background(p.Secondary).TextColor(p.SecondaryForeground).Cursor(ui.CursorPointer)
				if b.Hovered() {
					b.Background(p.SecondaryHover)
				}
				b.Children(func() {
					ui.Icon(c, r.Icon).FontSize(16)
					ui.Text(c, label).FontSize(14).SingleLine().Grow(1)
					if r.Key != 0 && keys != nil {
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
	statusBar(c, path, 12, after, system...)
}

// diffStatus is a diff's footer: the path, the change's +N −M, and at the
// far end the window's diff layout as one icon button (diffToggle). Every
// view of a diff ends with it, a document of several diffs once.
func diffStatus(c *ui.Context, path string, narrow bool, counts func(), system ...func()) {
	statusBar(c, path, 4, func() {
		counts()
		diffToggle(c, narrow)
	}, system...)
}

// diffSplit reports whether the host's diffs show side by side: the
// window's choice, where the document has the width for two columns.
func diffSplit(c *ui.Context, narrow bool) bool { return *kit.DiffSplit(c) && !narrow }

// diffToggle is the window's choice between a unified diff and a
// side-by-side one (kit.DiffSplit), named by what a press shows: the
// columns while unified, the rows while side by side. A document too
// narrow for two columns shows unified whatever the choice, which it
// leaves alone: the button is disabled and its tooltip says why.
func diffToggle(c *ui.Context, narrow bool) {
	split := kit.DiffSplit(c)
	icon, label := icons.DiffColumns, i18n.T(c, "git_diff_side_by_side")
	if diffSplit(c, narrow) {
		icon, label = icons.DiffRows, i18n.T(c, "git_diff_inline")
	}
	tip := label
	if narrow {
		tip = i18n.T(c, "git_diff_needs_width")
	}
	b := kit.GhostLook(c, ui.ButtonBase(c.Key("diff-layout")).Label(label), false).Size(24, 24).Shrink(0).
		TextColor(theme.Of(c).MutedForeground).Disabled(narrow)
	if b.Hovered() {
		b.TextColor(theme.Of(c).Foreground)
	}
	kit.Tooltip(c, b, tip)
	b.Children(func() { ui.Icon(c, icon).FontSize(15) }).OnClick(func() { *split = !*split })
}

// statusBar is status ending end DIPs before the column's edge.
func statusBar(c *ui.Context, path string, end float32, after func(), system ...func()) {
	p := theme.Of(c)
	ui.Row(c.Key("footer")).Height(footer).Shrink(0).PaddingStart(12).PaddingEnd(end).Gap(12).BorderWidth(1, 0, 0, 0).BorderColor(p.Border).
		TextColor(p.MutedForeground).Children(func() {
		ui.Text(c, path).FontSize(12).SingleLine().Grow(1).Shrink(1).ContextMenu(func(m *ui.Menu) {
			if m.Item(i18n.T(c, "files_copy_path")).Chosen() {
				c.WriteClipboard(path)
			}
			// The demo's files are on no disk for another app to open.
			if len(system) > 0 && system[0] != nil && !kit.Demo(c) && m.Item(i18n.T(c, "files_open_system")).Chosen() {
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
		empty(c, icons.File, "file_empty")
		return
	}
	v := pr.Files.item(path)
	if pr.Actions != nil && !v.loaded {
		pr.SetFile(path, f.Text)
	}
	md := strings.HasSuffix(f.Name, ".md")
	toolbar(c, "toolbar", func() {
		kit.IconButton(c.Key("save"), icons.Save, i18n.T(c, "files_save"), 28, false).Disabled(pr.Actions == nil || !v.dirty || !pr.fileWritable(v)).OnClick(func() {
			if pr.Actions != nil && pr.Actions.SaveFile != nil {
				text, _ := pr.FileDraft(path)
				pr.Actions.SaveFile(c, path, text)
			}
		})
		separator(c)
		kit.IconButton(c.Key("undo"), icons.Undo, i18n.T(c, "files_undo"), 28, false).Disabled(!pr.fileWritable(v) || v.editor == nil || !v.editor.Buffer.CanUndo()).OnClick(func() {
			if pr.fileWritable(v) && v.editor != nil && v.editor.Buffer.Undo() {
				pr.FileChanged(path, v.editor.Buffer.String())
			}
		})
		kit.IconButton(c.Key("redo"), icons.Redo, i18n.T(c, "files_redo"), 28, false).Disabled(!pr.fileWritable(v) || v.editor == nil || !v.editor.Buffer.CanRedo()).OnClick(func() {
			if pr.fileWritable(v) && v.editor != nil && v.editor.Buffer.Redo() {
				pr.FileChanged(path, v.editor.Buffer.String())
			}
		})
		separator(c)
		// The demo's read-only view has no editor to search in.
		kit.IconButton(c.Key("find"), icons.Search, i18n.T(c, "files_find"), 28, false).Disabled(v.editor == nil && kit.Demo(c)).OnClick(func() {
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
			options.Editable = pr.fileWritable(v)
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

// diff is a changed file in MyGo's diff viewer over its footer, unified as
// content diff view or side by side by the footer's choice, with the change under the
// pointer's Revert and Keep (editor.DefaultHunkActions) and Stage: in the
// preview each toasts, as they would write the file and the index.
func (pr *Project) diff(c *ui.Context, path string, narrow bool) {
	if pr.gitDocument(c, path, narrow) {
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
		empty(c, icons.Network, "git_diff_empty")
		return
	}
	metadata := ch.Diff
	counts := func() {
		ui.Textf(c, "+%d", ch.Added).FontSize(12).TextColor(p.Success)
		ui.Textf(c, "−%d", ch.Removed).FontSize(12).TextColor(p.Danger)
	}
	system := func() { pr.fileAction(c, FileAction{Kind: "files_open_system", Paths: []string{ch.Path}}) }
	if metadata.Scope == pr.Git.scope && metadata.Binary {
		// No diff shows, so the footer has no layout to choose.
		ui.Column(c.Key("binary")).Grow(1).MinHeight(0).Children(func() {
			ui.Text(c, binaryDiff(c, metadata.Size)).FontSize(14).Padding(12).TextColor(p.MutedForeground)
		})
		status(c, ch.Path, counts, system)
		return
	}
	box := ui.Column(c.Key(ch.Path)).Grow(1).MinHeight(0)
	old, new := gitDiffTexts(box, *ch)
	selected := -1
	options := kit.DiffOptions{Language: editor.LanguageFor(ch.Path), Split: diffSplit(c, narrow),
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
						// The demo's index takes the hunk: the file is staged, and
						// still changed in the work tree while it has other hunks.
						sampled(c, func() bool {
							if pr.Actions != nil {
								return false
							}
							rest := ch.Unstaged
							if len(ch.Hunks) < 2 {
								rest = ""
							}
							stageChange(ch)
							ch.Unstaged = rest
							return true
						})
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
	if metadata.Scope == pr.Git.scope && metadata.Partial {
		ui.Text(c, i18n.T(c, "git_diff_partial")).FontSize(12).Padding(8, 12).TextColor(p.MutedForeground)
	}
	diffStatus(c, ch.Path, narrow, counts, system)
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
	if len(s.Data.Checkpoints) == 0 {
		s.vacant(c, History, "panel_history_empty")
		return
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
						} else if s.Actions == nil && s.Demo {
							// The demo asks as the application does, then drops the
							// checkpoint from its memory.
							s.historyConfirm = confirm{open: true, title: undo, message: i18n.T(c, "workspace_restore_confirm"), target: cp.Turn, run: func(*ui.Context) {
								s.Data.Checkpoints = slices.DeleteFunc(s.Data.Checkpoints, func(x preview.Checkpoint) bool { return x.ID == cp.ID })
							}}
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
		return
	}
	s.vacant(c, Documents, "panel_documents_empty")
}

// empty fills a tab that has nothing to show with the standard empty state:
// the icon and a short title, centered.
func empty(c *ui.Context, icon *ui.SVG, key string) {
	kit.Result(c, kit.ResultProps{Icon: icon, Title: i18n.T(c, key)}).Grow(1).Justify(ui.Center)
}

// vacant fills a session resource's tab without content: a spinner while
// the session's are read, else the resource's empty state.
func (s *State) vacant(c *ui.Context, k Kind, key string) {
	if s.Actions != nil && s.Actions.Loading != nil && s.Actions.Loading(k) {
		ui.Column(c.Key("loading")).Grow(1).Center().Children(func() { kit.Loader(c, 20).Label(i18n.T(c, ResourceOf(k).Label)) })
		return
	}
	empty(c, ResourceOf(k).Icon, key)
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
