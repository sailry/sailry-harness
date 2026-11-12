package panel

import (
	"runtime"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// fileDrag is a file tree row being dragged: its path.
type fileDrag struct{ path string }

// visible are the tree's rows shown, in order: every folder's while it
// is open.
func (pr *Project) visible() []string {
	var out []string
	var walk func(n *preview.FileNode)
	walk = func(n *preview.FileNode) {
		out = append(out, n.Path)
		if n.Children != nil && (pr.Files.folders[n.Path] != nil && *pr.Files.folders[n.Path] || pr.Files.folders[n.Path] == nil && (pr.Actions == nil || n.Path == "")) {
			for _, k := range n.Children {
				walk(k)
			}
		}
	}
	if pr.Data.Files != nil {
		walk(pr.Data.Files)
	}
	return out
}

// directory is where an action on path puts files: path when it is a
// folder, else its folder.
func (pr *Project) directory(path string) string {
	if f := findFile(pr.Data.Files, path); f != nil && f.Children != nil {
		return path
	}
	return parentDir(path)
}

// tree is 28 DIP rows indented 16 per level from 8. It selects as the files
// plugin's explorer does: a click selects a row, Cmd (Ctrl elsewhere) adds or
// takes it away, Shift selects the rows from the anchor; the arrows move the
// selection, Shift with Up or Down extends it, Right and Left open and close
// a folder and Cmd+A selects every row. A click or Enter opens a file,
// calling open, or opens and closes a folder. The right-click menu acts on
// the selection; a row dropped on a folder would move there. What writes
// files shows the preview's notice.
func (pr *Project) tree(c *ui.Context, own bool, open func(path string)) {
	v := &pr.Files
	p := theme.Of(c)
	pr.filesHeader(c)
	if v.searchOpen {
		pr.fileSearchView(c)
		return
	}
	openFile := func(relative string) {
		if pr.Actions != nil && pr.Actions.OpenFile != nil && pr.Actions.OpenFile(relative) {
			return
		}
		open(relative)
	}
	if pr.Actions != nil && pr.Data.Files != nil && len(pr.Data.Files.Children) == 0 {
		state := v.directories[""]
		// A folder read before stays as it is while it is read again: the
		// loader is for the first read alone, or every refresh would flash.
		if state.Loaded && !state.Failed {
			kit.Result(c, kit.ResultProps{Icon: icons.Folder, Title: i18n.T(c, "files_directory_empty")}).Grow(1).Justify(ui.Center)
			pr.directoryTail(c, "")
			return
		}
		if state.Loading {
			ui.Column(c.Key("files-loading")).Grow(1).Center().Children(func() { kit.Loader(c, 20) })
			return
		}
	}
	visible := pr.visible()
	box := ui.Scroll(c.Key("scroll")).TrackScroll(scroll(&v.tree, own)).Grow(1).Padding(8)
	box.ContextMenu(func(m *ui.Menu) { pr.fileMenu(c, m, visible, "") })
	box.Children(func() {
		tree := ui.TreeBase(c)
		pr.treeKeys(c, tree.Root, visible)
		tree.Root.Children(func() {
			var node func(n *preview.FileNode)
			node = func(n *preview.FileNode) {
				c.Key(n.Path)
				var folder *bool
				if n.Children != nil {
					if v.folders[n.Path] == nil {
						o := pr.Actions == nil || n.Path == ""
						v.folders[n.Path] = &o
					}
					folder = v.folders[n.Path]
					if *folder && pr.Actions != nil && pr.Actions.ReadDirectory != nil {
						pr.Actions.ReadDirectory(n.Path)
					}
				}
				item := tree.Item(folder)
				row := item.Row.Height(rowHeight).PaddingStart(8 + 16*float32(item.Depth())).PaddingEnd(8).Gap(8).
					Radius(p.Radius).Cursor(ui.CursorPointer).Label(n.Name)
				// The root's path is "": the request is its own flag, or the
				// root's row would take the focus from the editor every frame.
				if v.focusing && v.focus == n.Path {
					row.Focus()
					v.focusing = false
				}
				switch {
				case row.Clicked():
					mods := row.ClickModifiers()
					shift, add := mods&ui.Shift != 0, mods&(ui.Super|ui.Ctrl) != 0
					v.sel.pick(visible, n.Path, shift, add)
					switch {
					case shift || add:
					case folder != nil:
						*folder = !*folder
					default:
						openFile(n.Path)
					}
				case row.FocusVisible() && v.sel.current != n.Path:
					v.sel.pick(visible, n.Path, false, false)
				}
				row.ContextMenu(func(m *ui.Menu) {
					v.sel.context(visible, n.Path)
					pr.fileMenu(c, m, visible, n.Path)
				})
				row.Drag(fileDrag{n.Path})
				// Where a drop on the row goes (directory, of the node at hand:
				// looking each row up by its path would walk the tree a row).
				dir := parentDir(n.Path)
				if n.Children != nil {
					dir = n.Path
				}
				movable := func(d fileDrag) bool { return parentDir(d.path) != dir && !within(dir, d.path) }
				if d, ok := ui.Drop[fileDrag](row); ok && movable(d) {
					pr.fileAction(c, FileAction{Kind: "files_move", Paths: []string{d.path}, Directory: dir})
				}
				text := p.MutedForeground
				if d, ok := ui.DragOver[fileDrag](row); ok && movable(d) {
					row.Background(p.Hover)
				}
				switch {
				case v.sel.selected[n.Path]:
					row.Background(p.Hover).AccessSelected(true)
					text = p.HoverForeground
				case row.Hovered():
					text = p.Foreground
				}
				row.TextColor(text).Children(func() {
					icon := icons.File
					switch {
					case folder != nil && *folder:
						icon = icons.FolderOpen
					case folder != nil:
						icon = icons.Folder
					}
					ui.Icon(c, icon).FontSize(16).TextColor(p.MutedForeground)
					ui.Text(c, n.Name).FontSize(14).SingleLine().Shrink(1)
				})
				item.Group(func() {
					for _, kid := range n.Children {
						node(kid)
					}
					if folder != nil && *folder {
						pr.directoryTail(c, n.Path)
					}
				})
			}
			if pr.Data.Files != nil {
				node(pr.Data.Files)
			}
		})
	})
}

// treeKeys are the tree's own keys: Shift with Up or Down extends the
// selection, Cmd+A selects every row; F2 renames, Cmd+C, Cmd+X and Cmd+V
// copy, cut and paste, which would write files.
func (pr *Project) treeKeys(c *ui.Context, root ui.Element, visible []string) {
	v := &pr.Files
	extend := func(step int) {
		i := max(slices.Index(visible, v.sel.current), 0)
		next := visible[min(max(i+step, 0), len(visible)-1)]
		v.sel.pick(visible, next, true, false)
		v.focus, v.focusing = next, true
	}
	targets := v.sel.targets(visible, v.sel.current)
	switch {
	case root.Shortcut(ui.Shift, ui.KeyDown):
		extend(1)
	case root.Shortcut(ui.Shift, ui.KeyUp):
		extend(-1)
	case root.Shortcut(ui.Cmd, ui.KeyA):
		v.sel.all(visible)
	case root.Shortcut(0, ui.KeyF2) && len(targets) == 1:
		pr.fileAction(c, FileAction{Kind: "files_rename", Paths: targets})
	case root.Shortcut(ui.Cmd, ui.KeyC) && len(targets) > 0:
		pr.fileAction(c, FileAction{Kind: "files_copy", Paths: targets})
	case root.Shortcut(ui.Cmd, ui.KeyX) && len(targets) > 0:
		pr.fileAction(c, FileAction{Kind: "files_cut", Paths: targets})
	case root.Shortcut(ui.Cmd, ui.KeyV):
		pr.fileAction(c, FileAction{Kind: "files_paste", Directory: pr.directory(v.sel.current)})
	case root.Shortcut(0, ui.KeyDelete) && len(targets) > 0:
		pr.fileAction(c, FileAction{Kind: "files_trash", Paths: targets})
	}
}

// fileMenu is a row's right-click menu, or the tree's for path "" (the
// files plugin's actions.js tree): it acts on the selection when the row
// is in it.
func (pr *Project) fileMenu(c *ui.Context, m *ui.Menu, visible []string, path string) {
	t := func(key string) string { return i18n.T(c, key) }
	paths := pr.Files.sel.targets(visible, path)
	item := func(key string) {
		if m.Item(t(key)).Chosen() {
			pr.fileAction(c, FileAction{Kind: key, Paths: paths, Directory: pr.directory(path)})
		}
	}
	// The demo's files are on no disk: nothing opens, reveals or downloads
	// them, and its conversation takes no file from here.
	demo := pr.Actions == nil && kit.Demo(c)
	if len(paths) > 0 {
		if !demo {
			item("files_open_system")
			m.Separator()
		}
		item("files_copy")
		item("files_cut")
	}
	// Nothing was copied in the preview, so nothing pastes.
	paste := pr.Actions != nil && pr.Actions.PasteReady != nil && pr.Actions.PasteReady() || demo && len(pr.Files.sampleClip) > 0
	if m.Item(t("files_paste")).Disabled(!paste).Chosen() {
		pr.fileAction(c, FileAction{Kind: "files_paste", Directory: pr.directory(path)})
	}
	if len(paths) > 0 {
		if m.Item(t("files_copy_path")).Chosen() {
			c.WriteClipboard(strings.Join(paths, "\n"))
		}
		if !demo {
			item("files_insert_conversation")
		}
		m.Separator()
		if len(paths) == 1 {
			reveal := "files_reveal_manager"
			switch runtime.GOOS {
			case "darwin":
				reveal = "files_reveal_finder"
			case "windows":
				reveal = "files_reveal_explorer"
			}
			if !demo {
				item(reveal)
			}
			item("files_rename")
			if f := findFile(pr.Data.Files, paths[0]); !demo && f != nil && f.Children == nil {
				item("files_download")
			}
		}
		item("files_trash")
		m.Separator()
	}
	item("files_new_file")
	item("files_new_directory")
}
