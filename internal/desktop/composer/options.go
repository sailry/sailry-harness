package composer

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The draft's options: the mode and permission menus, the host, project and
// worktree of the context bar, and the settings popover a narrow composer
// folds them into.

// Mode is how the agent works (WorkMode).
type Mode int

const (
	Code Mode = iota
	Plan
	Flagship
)

// Modes are the modes' copy and icons, in the menu's order.
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

// Permissions are the permission modes' copy and icons.
var Permissions = []choice{
	{"composer_permission_ask", "composer_permission_ask_description", icons.CircleUser},
	{"composer_permission_project", "composer_permission_project_description", icons.CircleCheck},
	{"composer_permission_full", "composer_permission_full_description", icons.TriangleAlert},
}

// choice is a label over a description.
type choice struct {
	label, description string
	icon               *ui.SVG
}

// subtle is crate::theme::subtle_button on a round trigger: a ghost
// button of Kit's default size, 32 high, 10 at each end, with an icon 8
// before its label.
func subtle(c *ui.Context, b ui.Element, icon *ui.SVG, label string, color ui.Color) ui.Element {
	return kit.GhostLook(c, b, false).Label(label).Height(32).PaddingX(10).Gap(8).Radius(16).TextColor(color).
		Children(func() {
			ui.Icon(c, icon).FontSize(16)
			ui.Text(c, label).FontSize(14).SingleLine()
		})
}

// menuID names one of this composer's menus. Kit keeps a menu's open state
// by its ID for the whole window, and a split shows several composers: a
// name shared by them would open the menu in each.
func (s *State) menuID(name string) string {
	id, ok := s.menus[name]
	if !ok {
		if s.menus == nil {
			s.menus = map[string]string{}
		}
		id = fmt.Sprintf("composer-%p-%s", s, name)
		s.menus[name] = id
	}
	return id
}

// modeMenu is the mode's icon and label, a menu of the modes with their
// descriptions, the check on the right.
func (s *State) modeMenu(c *ui.Context, ctx *Context) {
	m := Modes[s.Mode]
	kit.MenuButton(c.Key("mode"), s.menuID("mode"), func(b ui.Element) {
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

// permissionMenu is Full access in the danger color, project access in the
// warning color.
func (s *State) permissionMenu(c *ui.Context, ctx *Context) {
	m := Permissions[s.Permission]
	label := i18n.T(c, m.label)
	kit.MenuButton(c.Key("permission"), s.menuID("permission"), func(b ui.Element) {
		kit.Tooltip(c, subtle(c, b.Disabled(ctx.Readonly), m.icon, label, permissionColor(theme.Of(c), s.Permission)), label)
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

// project is the draft's project, nil for a chat without one.
func (s *State) project(ctx *Context) *Project {
	for i := range ctx.Projects {
		if ctx.Projects[i].ID == s.Project {
			return &ctx.Projects[i]
		}
	}
	return nil
}

func (s *State) OpenProjectPicker() {
	s.projectPicker = true
}

// controlForm is where a location control shows (contributions Form): on
// the context bar, or as a row of the compact settings popover, which takes
// them below the icon width.
type controlForm int

const (
	toolbarForm controlForm = iota
	menuForm
)

// contextButton is a location control. On the context bar it is Kit's small
// round ghost button in the bar's muted color: 24 high, 8 at each end, a 14
// icon, the label and the menu's caret 4 apart, at most 192 wide. In the
// settings popover it is a menu row.
func contextButton(c *ui.Context, b ui.Element, icon *ui.SVG, label string, form controlForm) ui.Element {
	p := theme.Of(c)
	kit.GhostLook(c, b, false).Label(label)
	if form == menuForm {
		return menuRow(c, b, icon, label, p.Foreground, icons.ChevronRight)
	}
	return b.Height(24).PaddingX(8).Gap(4).Radius(12).MaxWidth(192).MinWidth(0).Shrink(1).TextColor(p.MutedForeground).
		Children(func() {
			ui.Icon(c, icon).FontSize(14).Shrink(0)
			ui.Text(c, label).FontSize(14).SingleLine().Shrink(1).MinWidth(0)
			ui.Icon(c, icons.ChevronDown).FontSize(12).Shrink(0)
		})
}

// projectButton is the location: the project's name, or Chats for none,
// and a menu of Chats, the host's projects with a check on the current one,
// then Add project.
func (s *State) projectButton(c *ui.Context, ctx *Context, actions Actions, form controlForm) {
	icon, label := icons.Inbox, i18n.T(c, "composer_no_project")
	if pr := s.project(ctx); pr != nil {
		icon, label = icons.Folder, pr.Name
	}
	if s.projectPicker {
		s.projectPicker = false
		if !ctx.Readonly && !ctx.LocationLocked {
			kit.OpenMenu(c, s.menuID("project"), true)
		}
	}
	kit.MenuButton(c.Key("project"), s.menuID("project"), func(b ui.Element) {
		contextButton(c, b.Disabled(ctx.Readonly || ctx.LocationLocked), icon, label, form)
	}, func(m *kit.Menu) {
		choose := func(id string) {
			if id == s.Project {
				return
			}
			s.Project, s.Worktree = id, ""
			if pr := s.project(ctx); pr != nil && len(pr.Worktrees) > 0 {
				if pr.Host != "" && pr.Host != s.Host {
					s.Host, s.Model, s.Team, s.MemberModels = pr.Host, "", "", nil
				}
				s.Worktree = pr.Worktrees[0].Branch
			}
		}
		if m.Item(i18n.T(c, "composer_no_project")).Icon(icons.Inbox).Checked(s.Project == "").Chosen() {
			choose("")
		}
		for _, pr := range ctx.Projects {
			if m.Item(pr.Name).Icon(icons.Folder).Checked(pr.ID == s.Project).Chosen() {
				choose(pr.ID)
			}
		}
		m.Separator()
		if m.Item(i18n.T(c, "project_add")).Icon(icons.Plus).Chosen() {
			actions.AddProject()
		}
	}, kit.MenuOptions{At: ui.AnchorTopStart, Self: ui.AnchorBottomStart})
}

// branchLabel is the worktree's branch as the inspection reads it: the
// branch, the short HEAD, or No branch.
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
func (s *State) worktreeMenu(c *ui.Context, ctx *Context, actions Actions, form controlForm) {
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
	disabled := ctx.Readonly || ctx.LocationLocked
	if s.ActivityCallbacks != nil && s.ActivityCallbacks.WorktreeEnabled != nil {
		disabled = ctx.Readonly || !s.ActivityCallbacks.WorktreeEnabled()
	}
	kit.MenuButton(c.Key("worktree"), s.menuID("worktree"), func(b ui.Element) {
		contextButton(c, b.Disabled(disabled), icon, label, form)
	}, func(m *kit.Menu) {
		for _, w := range pr.Worktrees {
			name := w.Branch
			if s.ActivityCallbacks != nil && s.ActivityCallbacks.WorktreeLabel != nil {
				name = s.ActivityCallbacks.WorktreeLabel(w.Branch)
			}
			if w.Main {
				name = pr.Name
			}
			if m.Item(name).Checked(w.Branch == s.Worktree).Chosen() {
				s.Worktree = w.Branch
				if s.ActivityCallbacks != nil && s.ActivityCallbacks.Worktree != nil {
					s.ActivityCallbacks.Worktree(w.Branch)
				}
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

// next moves a selection among n rows, round at either end; -1 for none.
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

// settingsButton is a round settings button whose popover holds the model
// controls over the mode and permission, each a row opening its page, and
// below the icon width the project and the worktree (composer/compact.rs:
// the model settings, then the composer's and the context's controls as
// menu rows).
func (s *State) settingsButton(c *ui.Context, ctx *Context, actions Actions) {
	p := theme.Of(c)
	narrow := s.width > 0 && s.width < iconWidth
	hint := i18n.T(c, "composer_settings") + " · " + i18n.T(c, Modes[s.Mode].label) + " · " + i18n.T(c, Permissions[s.Permission].label)
	b := kit.IconButton(c.Key("settings"), icons.Settings2, hint, 32, s.settings).Radius(16).Disabled(ctx.Readonly)
	b.HasPopup(ui.PopupDialog).Expanded(s.settings).OnClick(func() {
		s.settings, s.page, s.layer = !s.settings, settingsMain, layerMain
	})
	panel := kit.Popover(c, b, &s.settings, func() {
		// row_text: a 16 icon, the label, and a 16 chevron, 8 apart, in a
		// small ghost button 32 high.
		row := func(key string, icon *ui.SVG, label string, color ui.Color, page settingsPage) {
			menuRow(c, kit.Ghost(c.Key(key), label, false), icon, label, color, icons.ChevronRight).
				OnClick(func() { s.page = page })
		}
		back := func() {
			label := i18n.T(c, "composer_back")
			menuRow(c, kit.Ghost(c.Key("back"), label, false), icons.ChevronLeft, label, p.Foreground, nil).
				OnClick(func() { s.page = settingsMain })
		}
		choices := func(list []choice, chosen int, color func(int) ui.Color, pick func(int)) {
			for i, m := range list {
				b := kit.Ghost(c.Key(m.label), i18n.T(c, m.label), i == chosen).FillWidth().Padding(8, 10).Gap(8).
					Justify(ui.Start).AccessSelected(i == chosen).TextColor(color(i))
				b.Children(func() {
					ui.Icon(c, m.icon).FontSize(16).Shrink(0)
					ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
						ui.Text(c, i18n.T(c, m.label)).FontSize(14).LineHeight(1.25)
						ui.Text(c, i18n.T(c, m.description)).FontSize(12).LineHeight(1.25).TextColor(p.MutedForeground)
					})
				}).OnClick(func() { pick(i); s.page = settingsMain })
			}
		}
		// Full access is the danger color, on its row and its choice.
		danger := func(m Permission) ui.Color {
			if m == FullAccess {
				return p.Danger
			}
			return p.Foreground
		}
		switch s.page {
		case settingsMode:
			back()
			choices(Modes, int(s.Mode), func(int) ui.Color { return p.Foreground }, func(i int) { s.Mode = Mode(i) })
		case settingsPermission:
			back()
			choices(Permissions, int(s.Permission), func(i int) ui.Color { return danger(Permission(i)) },
				func(i int) { s.Permission = Permission(i) })
		default:
			m, pm := Modes[s.Mode], Permissions[s.Permission]
			if len(ctx.Models) == 0 {
				// model_settings without a model: Select model, to the settings.
				kit.Ghost(c.Key("select-model"), i18n.T(c, "composer_select_model"), false).FillWidth().Height(32).
					Disabled(ctx.SelectModel == nil).
					Children(func() { ui.Text(c, i18n.T(c, "composer_select_model")).FontSize(14).SingleLine() }).
					OnClick(func() { ctx.SelectModel() })
			} else {
				s.controls(c, ctx, actions)
			}
			row("mode-page", m.icon, i18n.T(c, m.label), p.Foreground, settingsMode)
			row("permission-page", pm.icon, i18n.T(c, pm.label), danger(s.Permission), settingsPermission)
			if narrow {
				s.projectButton(c, ctx, actions, menuForm)
				s.worktreeMenu(c, ctx, actions, menuForm)
			}
		}
	})
	if panel != (ui.Element{}) {
		panel.AttachTo(b, ui.AnchorTopStart, ui.AnchorBottomStart).Margin(0, 0, 8, 0).Padding(8).Width(312).Gap(4)
	}
}

// menuRow is a control as a row of the settings popover (Form::Menu): a
// small ghost button 32 high and as wide as the panel, a 16 icon, the label
// and a 16 slot for the chevron, 8 apart.
func menuRow(c *ui.Context, b ui.Element, icon *ui.SVG, label string, color ui.Color, chevron *ui.SVG) ui.Element {
	return b.FillWidth().MinWidth(0).Height(32).PaddingX(8).Gap(8).Justify(ui.Start).TextColor(color).Children(func() {
		ui.Icon(c, icon).FontSize(16).Shrink(0)
		ui.Text(c, label).FontSize(14).SingleLine().Grow(1).MinWidth(0)
		slot := ui.Box(c).Size(16, 16).Shrink(0)
		if chevron != nil {
			slot.Children(func() { ui.Icon(c, chevron).FontSize(16) })
		}
	})
}
