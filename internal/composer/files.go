package composer

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

// The file picker (file_picker.rs, file_picker/render*): the execution
// host's file chooser, here choosing files to attach. A toolbar with
// Back, Forward and Up, the folder's breadcrumbs and a filter; the
// locations beside the folder's entries in MyGo's Table (name, size,
// kind), several chosen with Cmd and Shift; the counts under them; the
// chosen paths, Cancel and Open in the footer. Double click or Enter
// opens a folder.

// fileState is the picker's state: open, the folder shown, the history
// Back and Forward walk, the filter and the files chosen.
type fileState struct {
	open     bool
	dir      *Entry
	back     []*Entry
	forward  []*Entry
	filter   string
	list     ui.ListState
	row      int
	selected ui.Selection[string]
}

// openFiles opens the picker at the host's root folder.
func (s *State) openFiles(ctx *Context) {
	if ctx.Readonly || ctx.Files == nil {
		return
	}
	s.files = fileState{open: true, dir: ctx.Files, row: -1}
}

// go_ shows a folder, remembering the one it leaves.
func (f *fileState) visit(dir *Entry) {
	if dir == nil || dir == f.dir {
		return
	}
	f.back, f.forward = append(f.back, f.dir), nil
	f.dir, f.filter, f.row = dir, "", -1
	f.selected.Clear()
}

// parent is the folder holding dir under root, nil at the root.
func parent(root, dir *Entry) *Entry {
	for _, e := range root.Children {
		if e == dir {
			return root
		}
		if e.Children != nil {
			if p := parent(e, dir); p != nil {
				return p
			}
		}
	}
	return nil
}

// trail is the folders from root to dir, for the breadcrumbs.
func trail(root, dir *Entry) []*Entry {
	out := []*Entry{dir}
	for d := dir; d != root; {
		d = parent(root, d)
		if d == nil {
			break
		}
		out = append([]*Entry{d}, out...)
	}
	return out
}

// entries are the folder's entries the filter matches, folders first,
// then by name (picker order_visible).
func (f *fileState) entries() []*Entry {
	q := strings.ToLower(f.filter)
	var out []*Entry
	for _, e := range f.dir.Children {
		if strings.Contains(strings.ToLower(e.Name), q) {
			out = append(out, e)
		}
	}
	slices.SortStableFunc(out, func(a, b *Entry) int {
		if (a.Children != nil) != (b.Children != nil) {
			if a.Children != nil {
				return -1
			}
			return 1
		}
		return strings.Compare(strings.ToLower(a.Name), strings.ToLower(b.Name))
	})
	return out
}

// filePicker is the picker's dialog, at most 900 wide and 480 high.
func (s *State) filePicker(c *ui.Context, ctx *Context, actions Actions) {
	f := &s.files
	if !f.open || ctx.Files == nil {
		return
	}
	p := theme.Of(c)
	w, h := c.Size()
	width := min(w-48, 900) - 48
	height := min(max(h-300, 120), 480)
	rows := f.entries()
	chosen := func() []*Entry {
		var out []*Entry
		for _, e := range f.dir.Children {
			if e.Children == nil && f.selected.Has(e.Path) {
				out = append(out, e)
			}
		}
		return out
	}
	open := func() {
		list := chosen()
		if len(list) == 0 {
			return
		}
		var add []*Attachment
		for _, e := range list {
			add = append(add, &Attachment{Name: e.Name, Path: e.Path, Mime: e.Mime, Size: e.Size})
		}
		f.open = false
		s.add(c, add, actions)
	}
	kit.Dialog(c.Key("file-picker"), &f.open, func() {
		ui.Text(c, i18n.T(c, "composer_attachment")).FontSize(16).FontWeight(600)
		ui.Text(c, i18n.T(c, "directory_preview")).FontSize(14).TextColor(p.MutedForeground)
		s.fileToolbar(c, ctx)
		ui.Row(c.Key("body")).Width(width).Height(height).Radius(p.RadiusLg).Border(1, p.Border).Clip().
			AlignItems(ui.Stretch).Children(func() {
			s.fileSidebar(c, ctx)
			ui.Column(c.Key("content")).Grow(1).MinWidth(0).Children(func() {
				f.list.Key = func(i int) any { return rows[i].Path }
				f.list.Selected, f.list.Selection = &f.row, &f.selected
				f.list.Label = func(i int) string { return rows[i].Name }
				cols := []ui.TableColumn{
					{Title: i18n.T(c, "directory_name"), ID: "name"},
					{Title: i18n.T(c, "directory_size"), ID: "size", Width: 96, Align: ui.End},
					{Title: i18n.T(c, "directory_kind"), ID: "kind", Width: 120},
				}
				if len(rows) == 0 {
					key := "files_directory_empty"
					if f.filter != "" {
						key = "directory_no_matches"
					}
					kit.Result(c.Key("empty"), kit.ResultProps{Icon: icons.FolderOpen, Title: i18n.T(c, key)}).Grow(1)
				} else {
					t := ui.Table(c.Key(f.dir.Path), &f.list, cols, len(rows), func(row, col int) {
						e := rows[row]
						switch col {
						case 0:
							ui.Row(c).Gap(8).MinWidth(0).AlignItems(ui.Center).Children(func() {
								icon := fileIcon(e.Mime)
								if e.Children != nil {
									icon = icons.Folder
								}
								ui.Icon(c, icon).FontSize(16).Shrink(0).TextColor(p.MutedForeground)
								ui.Text(c, e.Name).FontSize(14).SingleLine().Shrink(1).MinWidth(0)
							})
						case 1:
							if e.Children == nil {
								ui.Text(c, size(c, e.Size)).FontSize(13).SingleLine().TextColor(p.MutedForeground)
							}
						case 2:
							kind := i18n.T(c, "directory_file_kind")
							if e.Children != nil {
								kind = i18n.T(c, "directory_folder_kind")
							}
							ui.Text(c, kind).FontSize(13).SingleLine().TextColor(p.MutedForeground)
						}
					}).Grow(1).Padding(0, 12)
					if t.Submitted() && f.row >= 0 && f.row < len(rows) {
						if e := rows[f.row]; e.Children != nil {
							f.visit(e)
						} else {
							open()
						}
					}
				}
				dirs, files := 0, 0
				for _, e := range f.dir.Children {
					if e.Children != nil {
						dirs++
					} else {
						files++
					}
				}
				ui.Row(c.Key("status")).Height(28).Shrink(0).PaddingX(12).Gap(12).AlignItems(ui.Center).
					BorderWidth(1, 0, 0, 0).BorderColor(p.Border).TextColor(p.MutedForeground).Children(func() {
					ui.Text(c, i18n.T(c, "directory_count_directories", "count", dirs)).FontSize(12)
					ui.Text(c, i18n.T(c, "directory_count_files", "count", files)).FontSize(12)
				})
			})
		})
		var paths []string
		for _, e := range chosen() {
			paths = append(paths, e.Path)
		}
		ui.Row(c.Key("footer")).FillWidth().Gap(12).AlignItems(ui.Center).Children(func() {
			text := strings.Join(paths, ", ")
			kit.Tooltip(c, ui.Text(c, text).FontSize(13).SingleLine().Grow(1).Shrink(1).MinWidth(0).TextColor(p.MutedForeground), text)
			kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).OnClick(func() { f.open = false })
			kit.Primary(c.Key("open"), i18n.T(c, "directory_open"), len(paths) == 0).
				Children(func() { ui.Text(c, i18n.T(c, "directory_open")).FontSize(14) }).OnClick(open)
		})
	}).Label(i18n.T(c, "composer_attachment"))
}

// fileToolbar is render/toolbar.rs: Back, Forward and Up, the breadcrumbs
// and the filter.
func (s *State) fileToolbar(c *ui.Context, ctx *Context) {
	p := theme.Of(c)
	f := &s.files
	up := parent(ctx.Files, f.dir)
	ui.Row(c.Key("toolbar")).FillWidth().Gap(4).AlignItems(ui.Center).Children(func() {
		kit.IconButton(c.Key("back"), icons.ArrowLeft, i18n.T(c, "directory_back"), 28, false).Disabled(len(f.back) == 0).
			OnClick(func() {
				f.forward = append(f.forward, f.dir)
				f.dir, f.back = f.back[len(f.back)-1], f.back[:len(f.back)-1]
				f.filter, f.row = "", -1
				f.selected.Clear()
			})
		kit.IconButton(c.Key("forward"), icons.ArrowRight, i18n.T(c, "directory_forward"), 28, false).
			Disabled(len(f.forward) == 0).OnClick(func() {
			f.back = append(f.back, f.dir)
			f.dir, f.forward = f.forward[len(f.forward)-1], f.forward[:len(f.forward)-1]
			f.filter, f.row = "", -1
			f.selected.Clear()
		})
		kit.IconButton(c.Key("up"), icons.ArrowUp, i18n.T(c, "directory_up"), 28, false).Disabled(up == nil).
			OnClick(func() { f.visit(up) })
		ui.Row(c.Key("crumbs")).Grow(1).MinWidth(0).Gap(2).AlignItems(ui.Center).Clip().Children(func() {
			for i, d := range trail(ctx.Files, f.dir) {
				if i > 0 {
					ui.Icon(c, icons.ChevronRight).FontSize(12).Shrink(0).TextColor(p.MutedForeground)
				}
				kit.Ghost(c.Key(d.Path), d.Name, d == f.dir).Height(28).PaddingX(6).Shrink(1).MinWidth(0).
					Children(func() { ui.Text(c, d.Name).FontSize(13).SingleLine() }).OnClick(func() { f.visit(d) })
			}
		})
		ui.Row(c.Key("filter")).Width(180).Height(28).PaddingX(8).Gap(6).Radius(p.Radius).Border(1, p.Input).
			AlignItems(ui.Center).Children(func() {
			ui.Icon(c, icons.Search).FontSize(14).TextColor(p.MutedForeground)
			ui.TextInputBase(c.Key("input"), &f.filter).Grow(1).FontSize(13).Placeholder(i18n.T(c, "directory_filter")).
				Label(i18n.T(c, "directory_filter")).PlaceholderColor(p.MutedForeground)
		})
	})
}

// fileSidebar is render/sidebar.rs: the locations, 188 wide.
func (s *State) fileSidebar(c *ui.Context, ctx *Context) {
	p := theme.Of(c)
	f := &s.files
	ui.Scroll(c.Key("locations")).Width(188).Shrink(0).Padding(8).Gap(2).Background(p.Sidebar).
		BorderWidth(0, 1, 0, 0).BorderColor(p.Border).Children(func() {
		ui.Text(c, i18n.T(c, "directory_locations")).FontSize(12).Padding(4, 8).TextColor(p.MutedForeground)
		place := func(key string, icon *ui.SVG, label string, dir *Entry) {
			kit.Ghost(c.Key(key), label, f.dir == dir).FillWidth().Height(28).PaddingX(8).Gap(8).Justify(ui.Start).
				Children(func() {
					ui.Icon(c, icon).FontSize(14)
					ui.Text(c, label).FontSize(13).SingleLine()
				}).OnClick(func() { f.visit(dir) })
		}
		place("home", icons.House, i18n.T(c, "directory_home"), ctx.Files)
		for _, e := range ctx.Files.Children {
			if e.Children != nil {
				place(e.Path, icons.Folder, e.Name, e)
			}
		}
	})
}
