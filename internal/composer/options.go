package composer

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

// The draft's options (conversation/mode.rs, permission.rs, context.rs,
// live/mode.rs, permission.rs, location.rs, repository.rs and
// composer/compact.rs): the mode and permission menus, the host, project
// and worktree of the context bar, and the settings popover a narrow
// composer folds them into.

// Mode is how the agent works (WorkMode).
type Mode int

const (
	Code Mode = iota
	Plan
	Flagship
)

// Modes are the modes' copy and icons, in the menu's order (mode.rs).
var Modes = []choice{
	{"composer_mode_code", "composer_mode_code_description", icons.Terminal},
	{"composer_mode_plan", "composer_mode_plan_description", icons.Map},
	{"composer_mode_flagship", "composer_mode_flagship_description", icons.Bot},
}

// Permission is what the agent may do without asking.
type Permission int

const (
	AskFirst Permission = iota
	ProjectAccess
	FullAccess
)

// Permissions are the permission modes' copy and icons (permission.rs).
var Permissions = []choice{
	{"composer_permission_ask", "composer_permission_ask_description", icons.CircleUser},
	{"composer_permission_project", "composer_permission_project_description", icons.CircleCheck},
	{"composer_permission_full", "composer_permission_full_description", icons.TriangleAlert},
}

// choice is a menu row of choice.rs: a label over a description.
type choice struct {
	label, description string
	icon               *ui.SVG
}

// subtle is crate::theme::subtle_button on a round trigger: a ghost
// button 32 high with an icon and a label.
func subtle(c *ui.Context, b ui.Element, icon *ui.SVG, label string, color ui.Color) ui.Element {
	return kit.GhostLook(c, b, false).Label(label).Height(32).PaddingX(12).Gap(6).Radius(16).TextColor(color).
		Children(func() {
			ui.Icon(c, icon).FontSize(16)
			ui.Text(c, label).FontSize(14).SingleLine()
		})
}

// modeMenu is live/mode.rs mode_menu: the mode's icon and label, a menu
// of the modes with their descriptions, the check on the right.
func (s *State) modeMenu(c *ui.Context, ctx *Context) {
	m := Modes[s.Mode]
	kit.MenuButton(c.Key("mode"), "composer-mode", func(b ui.Element) {
		subtle(c, b.Disabled(ctx.Readonly), m.icon, i18n.T(c, m.label), theme.Of(c).Foreground)
	}, func(menu *kit.Menu) {
		for i, m := range Modes {
			if menu.Item(i18n.T(c, m.label)).Description(i18n.T(c, m.description)).Icon(m.icon).
				Checked(Mode(i) == s.Mode).Chosen() {
				s.Mode = Mode(i)
			}
		}
	}, kit.MenuOptions{At: ui.AnchorTopStart, Self: ui.AnchorBottomStart, CheckRight: true})
}

// permissionMenu is live/permission.rs permission_menu: Full access in
// the danger color, project access in the warning color.
func (s *State) permissionMenu(c *ui.Context, ctx *Context) {
	m := Permissions[s.Permission]
	label := i18n.T(c, m.label)
	kit.MenuButton(c.Key("permission"), "composer-permission", func(b ui.Element) {
		subtle(c, b.Disabled(ctx.Readonly), m.icon, label, permissionColor(theme.Of(c), s.Permission)).Tooltip(label)
	}, func(menu *kit.Menu) {
		for i, m := range Permissions {
			if menu.Item(i18n.T(c, m.label)).Description(i18n.T(c, m.description)).Icon(m.icon).
				Checked(Permission(i) == s.Permission).Chosen() {
				s.Permission = Permission(i)
			}
		}
	}, kit.MenuOptions{At: ui.AnchorTopStart, Self: ui.AnchorBottomStart, CheckRight: true})
}

func permissionColor(p *theme.Palette, m Permission) ui.Color {
	switch m {
	case FullAccess:
		return p.Danger
	case ProjectAccess:
		return p.Warning
	}
	return p.Foreground
}

// host is the draft's host, the first when unknown.
func (s *State) host(ctx *Context) (Host, bool) {
	for _, h := range ctx.Hosts {
		if h.ID == s.Host {
			return h, true
		}
	}
	if len(ctx.Hosts) > 0 {
		return ctx.Hosts[0], true
	}
	return Host{}, false
}

// project is the draft's project, nil for a chat without one.
func (s *State) project(ctx *Context) *Project {
	for i := range ctx.Projects {
		if ctx.Projects[i].ID == s.Project {
			return &ctx.Projects[i]
		}
	}
	return nil
}

// contextButton is a context bar button: round, ghost, muted, an icon and
// a label (the label alone goes in a narrow composer), and a chevron for
// a menu.
func contextButton(c *ui.Context, b ui.Element, icon *ui.SVG, label string, chevron, iconOnly bool) ui.Element {
	p := theme.Of(c)
	kit.GhostLook(c, b, false).Label(label).Height(28).PaddingX(10).Gap(6).Radius(14).MaxWidth(224).
		TextColor(p.MutedForeground)
	if iconOnly {
		b.Width(28).PaddingX(0).Tooltip(label)
	}
	return b.Children(func() {
		ui.Icon(c, icon).FontSize(14)
		if !iconOnly {
			ui.Text(c, label).FontSize(14).SingleLine().Shrink(1).MinWidth(0)
			if chevron {
				ui.Icon(c, icons.ChevronDown).FontSize(12).Shrink(0)
			}
		}
	})
}

// hostMenu is context.rs's host button: Local device or Remote, its hint
// naming the host, and a menu of the hosts.
func (s *State) hostMenu(c *ui.Context, ctx *Context, iconOnly bool) {
	h, ok := s.host(ctx)
	if !ok {
		return
	}
	icon, label := icons.Cpu, i18n.T(c, "composer_host_local")
	if !h.Local {
		icon, label = icons.Globe, i18n.T(c, "composer_host_remote")
	}
	kit.MenuButton(c.Key("host"), "composer-host", func(b ui.Element) {
		contextButton(c, b, icon, label, true, iconOnly).Tooltip(i18n.T(c, "composer_host_hint", "name", i18n.T(c, h.Name)))
	}, func(m *kit.Menu) {
		for _, x := range ctx.Hosts {
			if m.Item(i18n.T(c, x.Name)).Checked(x.ID == h.ID).Chosen() {
				s.Host = x.ID
			}
		}
	}, kit.MenuOptions{At: ui.AnchorTopStart, Self: ui.AnchorBottomStart})
}

// projectButton is the location: the project's name, or Chats for none,
// opening the location picker (location.rs choose_location).
func (s *State) projectButton(c *ui.Context, ctx *Context, iconOnly bool) {
	icon, label := icons.Inbox, i18n.T(c, "composer_no_project")
	if pr := s.project(ctx); pr != nil {
		icon, label = icons.Folder, pr.Name
	}
	b := ui.ButtonBase(c.Key("project")).Disabled(ctx.Readonly)
	contextButton(c, b, icon, label, false, iconOnly).OnClick(func() {
		s.place = placeState{open: true}
	})
}

// branchLabel is the worktree's branch as the inspection reads it
// (repository.rs): the branch, the short HEAD, or No branch.
func branchLabel(c *ui.Context, ctx *Context, worktree string) string {
	switch {
	case ctx.Repository.Branch != "":
		return ctx.Repository.Branch
	case ctx.Repository.Kind == Git:
		return i18n.T(c, "composer_branch_none")
	}
	return worktree
}

// worktreeMenu chooses the worktree of the project: its checkouts with a
// check, then New worktree where the folder is a Git repository.
func (s *State) worktreeMenu(c *ui.Context, ctx *Context, actions Actions, iconOnly bool) {
	pr := s.project(ctx)
	if pr == nil || ctx.Repository.Kind == Unavailable {
		return
	}
	main := true
	for _, w := range pr.Worktrees {
		if w.Branch == s.Worktree {
			main = w.Main
		}
	}
	icon := icons.Folder
	if !main {
		icon = icons.Network
	}
	label := branchLabel(c, ctx, s.Worktree)
	kit.MenuButton(c.Key("worktree"), "composer-worktree", func(b ui.Element) {
		contextButton(c, b.Disabled(ctx.Readonly), icon, label, false, iconOnly)
	}, func(m *kit.Menu) {
		for _, w := range pr.Worktrees {
			name := w.Branch
			if w.Main {
				name = pr.Name
			}
			if m.Item(name).Checked(w.Branch == s.Worktree).Chosen() {
				s.Worktree = w.Branch
			}
		}
		if ctx.Repository.Kind == Git {
			m.Separator()
			if m.Item(i18n.T(c, "location_create")).Icon(icons.Plus).Chosen() {
				actions.CreateWorktree()
			}
		}
	}, kit.MenuOptions{At: ui.AnchorTopStart, Self: ui.AnchorBottomStart})
}

// placeState is the location picker's: open, its query and selected row.
type placeState struct {
	open  bool
	query string
	sel   int
}

// locationDialog is location.rs's picker (command_picker.rs dialog): a
// searchable list 480 wide of Chats, the host's projects and Add project,
// rows 36 high with a check on the current one.
func (s *State) locationDialog(c *ui.Context, ctx *Context, actions Actions) {
	st := &s.place
	if !st.open {
		return
	}
	type row struct {
		id, label string
		icon      *ui.SVG
		checked   bool
	}
	all := []row{{"", i18n.T(c, "composer_no_project"), icons.Inbox, s.Project == ""}}
	for _, pr := range ctx.Projects {
		all = append(all, row{pr.ID, pr.Name, icons.Folder, pr.ID == s.Project})
	}
	all = append(all, row{"+", i18n.T(c, "project_add"), icons.Plus, false})
	var rows []row
	q := strings.ToLower(st.query)
	for _, r := range all {
		if strings.Contains(strings.ToLower(r.label), q) {
			rows = append(rows, r)
		}
	}
	choose := func(r row) {
		st.open = false
		switch r.id {
		case "+":
			actions.AddProject()
		case s.Project:
		default:
			s.Project, s.Worktree = r.id, ""
			if pr := s.project(ctx); pr != nil && len(pr.Worktrees) > 0 {
				s.Worktree = pr.Worktrees[0].Branch
			}
		}
	}
	list(c.Key("location-picker"), &st.open, &st.query, &st.sel, len(rows), i18n.T(c, "location_project"),
		func(i int) { choose(rows[i]) }, func(i int, selected bool) {
			r := rows[i]
			ui.Icon(c, r.icon).FontSize(14)
			ui.Text(c, r.label).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
			if r.checked {
				ui.Icon(c, icons.Check).FontSize(14)
			}
		})
}

// list is command_picker.rs's searchable list dialog: 480 wide, the query
// over rows 36 high, Up and Down moving round, Enter choosing, Escape
// clearing the query and then closing.
func list(c *ui.Context, open *bool, query *string, sel *int, n int, placeholder string, choose func(int), row func(i int, selected bool)) {
	p := theme.Of(c)
	if *sel >= n {
		*sel = 0
	}
	w, _ := c.Size()
	kit.Dialog(c, open, func() {
		ui.Row(c.Key("query")).Height(45).PaddingX(12).Gap(8).Shrink(0).BorderWidth(0, 0, 1, 0).BorderColor(p.Border).Children(func() {
			ui.Icon(c, icons.Search).FontSize(16).TextColor(p.MutedForeground)
			in := ui.TextInputBase(c.Key("input"), query).Grow(1).FontSize(14).TextColor(p.PopoverForeground).
				Placeholder(placeholder).Label(placeholder).AutoFocus()
			if in.Changed() {
				*sel = 0
			}
			if in.Shortcut(0, ui.KeyDown) {
				*sel = next(n, *sel, false)
			}
			if in.Shortcut(0, ui.KeyUp) {
				*sel = next(n, *sel, true)
			}
			in.OnSubmit(func() {
				if *sel >= 0 && *sel < n {
					choose(*sel)
				}
			})
			if in.Shortcut(0, ui.KeyEscape) {
				if *query != "" {
					*query, *sel = "", 0
				} else {
					*open = false
				}
			}
		})
		ui.Scroll(c.Key("rows")).MaxHeight(344).Padding(8).Children(func() {
			if n == 0 {
				ui.Text(c, i18n.T(c, "git_no_matches")).FontSize(14).Padding(12).TextColor(p.MutedForeground)
				return
			}
			for i := range n {
				b := ui.ButtonBase(c.Key(i)).Height(36).PaddingX(8).Gap(8).Radius(p.Radius).Shrink(0).
					TextColor(p.PopoverForeground).AlignItems(ui.Center)
				if b.Hovered() {
					*sel = i
				}
				if i == *sel {
					b.Background(p.Hover).TextColor(p.HoverForeground).ScrollIntoView()
				}
				b.Children(func() { row(i, i == *sel) }).OnClick(func() { choose(i) })
			}
		})
	}).Width(min(480, w-40)).MaxHeightPercent(60).Padding(0).Gap(0).Label(placeholder).Clip()
}

// next moves a selection among n rows, round at either end
// (command_picker.rs next); -1 for none.
func next(n, selected int, reverse bool) int {
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

// settingsPage is a page of the compact settings popover.
type settingsPage int

const (
	settingsMain settingsPage = iota
	settingsMode
	settingsPermission
)

// settingsButton is compact.rs composer_settings: a round settings button
// whose popover holds the mode and permission, each a row opening its
// page, over the model controls.
func (s *State) settingsButton(c *ui.Context, ctx *Context, actions Actions) {
	p := theme.Of(c)
	hint := i18n.T(c, "composer_settings") + " · " + i18n.T(c, Modes[s.Mode].label) + " · " + i18n.T(c, Permissions[s.Permission].label)
	b := kit.IconButton(c.Key("settings"), icons.Settings2, hint, 32, s.settings).Radius(16).Disabled(ctx.Readonly)
	b.HasPopup(ui.PopupDialog).Expanded(s.settings).OnClick(func() {
		s.settings, s.page, s.layer = !s.settings, settingsMain, layerMain
	})
	panel := ui.Popover(c, b, &s.settings, func() {
		row := func(key string, icon *ui.SVG, label string, color ui.Color, page settingsPage) {
			kit.Ghost(c.Key(key), label, false).FillWidth().Height(32).PaddingX(8).Gap(8).Justify(ui.Start).TextColor(color).
				Children(func() {
					ui.Icon(c, icon).FontSize(16)
					ui.Text(c, label).FontSize(14).SingleLine().Grow(1)
					ui.Icon(c, icons.ChevronRight).FontSize(12).TextColor(p.MutedForeground)
				}).OnClick(func() { s.page = page })
		}
		back := func() {
			kit.Ghost(c.Key("back"), i18n.T(c, "composer_back"), false).Height(28).PaddingX(8).Gap(6).Justify(ui.Start).
				Children(func() {
					ui.Icon(c, icons.ArrowLeft).FontSize(14)
					ui.Text(c, i18n.T(c, "composer_back")).FontSize(14)
				}).OnClick(func() { s.page = settingsMain })
		}
		choices := func(list []choice, chosen int, pick func(int)) {
			for i, m := range list {
				b := kit.Ghost(c.Key(m.label), i18n.T(c, m.label), i == chosen).FillWidth().Padding(8).Gap(8).
					Justify(ui.Start).AccessSelected(i == chosen)
				b.Children(func() {
					ui.Icon(c, m.icon).FontSize(16).Shrink(0)
					ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
						ui.Text(c, i18n.T(c, m.label)).FontSize(14).LineHeight(1.25)
						ui.Text(c, i18n.T(c, m.description)).FontSize(12).LineHeight(1.25).TextColor(p.MutedForeground)
					})
				}).OnClick(func() { pick(i); s.page = settingsMain })
			}
		}
		switch s.page {
		case settingsMode:
			back()
			choices(Modes, int(s.Mode), func(i int) { s.Mode = Mode(i) })
		case settingsPermission:
			back()
			choices(Permissions, int(s.Permission), func(i int) { s.Permission = Permission(i) })
		default:
			m, pm := Modes[s.Mode], Permissions[s.Permission]
			row("mode-page", m.icon, i18n.T(c, m.label), p.Foreground, settingsMode)
			row("permission-page", pm.icon, i18n.T(c, pm.label), permissionColor(p, s.Permission), settingsPermission)
			ui.Box(c).FillWidth().Height(1).Background(p.Border).Shrink(0)
			s.controls(c, ctx, actions)
		}
	})
	if panel != (ui.Element{}) {
		panel.AttachTo(b, ui.AnchorTopStart, ui.AnchorBottomStart).Margin(0, 0, 8, 0).Padding(8).Width(312).Gap(4).
			Background(p.Floating())
	}
}
