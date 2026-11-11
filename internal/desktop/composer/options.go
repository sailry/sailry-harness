package composer

import (
	"fmt"
	"hash/fnv"

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

// Modes are the modes' copy, icons and tints, in the menu's order: Code
// neutral, Plan blue, Flagship violet.
var Modes = []choice{
	{"composer_mode_code", "composer_mode_code_description", icons.Terminal, [2]ui.Color{}},
	{"composer_mode_plan", "composer_mode_plan_description", icons.Map, theme.Tints[0]},
	{"composer_mode_flagship", "composer_mode_flagship_description", icons.Bot, theme.Tints[1]},
}

// Permission is what the agent may do without asking.
type Permission int

const (
	AskFirst Permission = iota
	ProjectAccess
	FullAccess
)

// Permissions are the permission modes' copy, icons and tints: asking
// neutral, project access teal, full access amber.
var Permissions = []choice{
	{"composer_permission_ask", "composer_permission_ask_description", icons.CircleUser, [2]ui.Color{}},
	{"composer_permission_project", "composer_permission_project_description", icons.CircleCheck, theme.Tints[2]},
	{"composer_permission_full", "composer_permission_full_description", icons.TriangleAlert, theme.Tints[3]},
}

// choice is a label over a description, with the icon and the tint of its
// row (kit.Choice); the zero tint is Kit's neutral one.
type choice struct {
	label, description string
	icon               *ui.SVG
	tint               [2]ui.Color
}

// modeTint is the tint of a package's mode, which declares none: one of the
// colored theme.Tints (the last is the neutral gray, the built-in Code
// mode's) by the FNV hash of the mode's key, as kit.Tint chooses an emblem's,
// so a mode keeps its color and the packages' modes tell apart.
func modeTint(key string) [2]ui.Color {
	h := fnv.New32a()
	h.Write([]byte(key))
	return theme.Tints[h.Sum32()%uint32(len(theme.Tints)-1)]
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

// modeKeys are the built-in modes' keys, as Context.SelectMode takes them.
var modeKeys = []string{"code", "plan", "flagship"}

// activeMode is the package's mode the conversation runs in, nil while a
// built-in one does.
func activeMode(ctx *Context) *PluginMode {
	for i := range ctx.PluginModes {
		if ctx.PluginModes[i].Selected {
			return &ctx.PluginModes[i]
		}
	}
	return nil
}

// modeLook is the current mode's icon and label: the package's while one of
// its modes is chosen.
func (s *State) modeLook(c *ui.Context, ctx *Context) (*ui.SVG, string) {
	if m := activeMode(ctx); m != nil {
		icon := m.Icon
		if icon == nil {
			icon = Modes[s.Mode].icon
		}
		return icon, m.Label
	}
	m := Modes[s.Mode]
	return m.icon, i18n.T(c, m.label)
}

// selectMode chooses a built-in mode, which ends a package's.
func (s *State) selectMode(ctx *Context, m Mode) {
	s.Mode = m
	if ctx.SelectMode != nil {
		ctx.SelectMode(modeKeys[m])
	}
}

// cycleMode is Shift+Tab (CycleMode): the next of the built-in modes and
// then the packages' modes that can be chosen, round.
func (s *State) cycleMode(ctx *Context) {
	var plugins []PluginMode
	if ctx.SelectMode != nil {
		for _, m := range ctx.PluginModes {
			if !m.Disabled {
				plugins = append(plugins, m)
			}
		}
	}
	index := int(s.Mode)
	for i, m := range plugins {
		if m.Selected {
			index = len(Modes) + i
		}
	}
	next := (index + 1) % (len(Modes) + len(plugins))
	if next < len(Modes) {
		s.selectMode(ctx, Mode(next))
		return
	}
	ctx.SelectMode(plugins[next-len(Modes)].Key)
}

// modeMenu is the mode's icon and label, a menu of the modes as Kit's choice
// rows: the icon in its tinted tile, the description, a check on the chosen
// one. The packages' modes follow the built-in ones, each in its key's tint,
// a mode that cannot be chosen disabled (live/mode.rs).
func (s *State) modeMenu(c *ui.Context, ctx *Context) {
	icon, label := s.modeLook(c, ctx)
	active := activeMode(ctx)
	kit.MenuButton(c.Key("mode"), s.menuID("mode"), func(b ui.Element) {
		subtle(c, b.Disabled(ctx.Readonly), icon, label, theme.Of(c).Foreground)
	}, func(menu *kit.Menu) {
		for i, m := range Modes {
			if menu.Item(i18n.T(c, m.label)).Description(i18n.T(c, m.description)).Icon(m.icon).Tint(m.tint).
				Checked(active == nil && Mode(i) == s.Mode).Chosen() {
				s.selectMode(ctx, Mode(i))
			}
		}
		for _, m := range ctx.PluginModes {
			icon := m.Icon
			if icon == nil {
				icon = icons.Bot
			}
			item := menu.Item(m.Label).Icon(icon).Tint(modeTint(m.Key)).Checked(m.Selected).Disabled(m.Disabled || ctx.SelectMode == nil)
			if m.Description != "" {
				item.Description(m.Description)
			}
			if item.Chosen() {
				ctx.SelectMode(m.Key)
			}
		}
	}, kit.MenuOptions{At: ui.AnchorTopStart, Self: ui.AnchorBottomStart, CheckRight: true})
}

// permissionMenu is the permission's icon and label, in the warning color
// unless it asks every time, over a menu of the permissions as Kit's choice
// rows; Full access has its description in the warning color.
func (s *State) permissionMenu(c *ui.Context, ctx *Context) {
	m := Permissions[s.Permission]
	label := i18n.T(c, m.label)
	kit.MenuButton(c.Key("permission"), s.menuID("permission"), func(b ui.Element) {
		kit.Tooltip(c, subtle(c, b.Disabled(ctx.Readonly), m.icon, label, permissionColor(theme.Of(c), s.Permission)), label)
	}, func(menu *kit.Menu) {
		for i, m := range Permissions {
			item := menu.Item(i18n.T(c, m.label)).Description(i18n.T(c, m.description)).Icon(m.icon).Tint(m.tint).
				Checked(Permission(i) == s.Permission)
			if Permission(i) == FullAccess {
				item.Warning()
			}
			if item.Chosen() {
				s.Permission = Permission(i)
			}
		}
	}, kit.MenuOptions{At: ui.AnchorTopStart, Self: ui.AnchorBottomStart, CheckRight: true})
}

// permissionColor is the permission control's color: the warning color for
// a permission that changes files without asking. The danger color stays
// for errors and destructive actions.
func permissionColor(p *theme.Palette, m Permission) ui.Color {
	if m == AskFirst {
		return p.Foreground
	}
	return p.Warning
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

// ContextButton is a control on the context bar: Kit's small round ghost
// button in the bar's muted color, 24 high, 8 at each end, a 14 icon and the
// label 4 apart, at most 192 wide; caret adds a menu's caret. The packages'
// context controls wear it too.
func ContextButton(c *ui.Context, b ui.Element, icon *ui.SVG, label string, selected, caret bool) ui.Element {
	return kit.GhostLook(c, b, selected).Label(label).Height(24).PaddingX(8).Gap(4).Radius(12).MaxWidth(192).MinWidth(0).Shrink(1).
		TextColor(theme.Of(c).MutedForeground).Children(func() {
		ui.Icon(c, icon).FontSize(14).Shrink(0)
		ui.Text(c, label).FontSize(14).SingleLine().Shrink(1).MinWidth(0)
		if caret {
			ui.Icon(c, icons.ChevronDown).FontSize(12).Shrink(0)
		}
	})
}

// contextButton is a location control: on the context bar a ContextButton
// with a caret, in the settings popover a menu row.
func contextButton(c *ui.Context, b ui.Element, icon *ui.SVG, label string, form controlForm) ui.Element {
	if form == menuForm {
		return MenuRow(c, kit.GhostLook(c, b, false).Label(label), icon, label, theme.Of(c).Foreground, icons.ChevronRight)
	}
	return ContextButton(c, b, icon, label, false, true)
}

// hostLabel is a host's name: its key's text for the local and the preview
// hosts.
func hostLabel(c *ui.Context, h Host) string {
	if h.Key {
		return i18n.T(c, h.Name)
	}
	return h.Name
}

// hostMenu chooses the host a new conversation runs on (live/context.rs
// host_control): the host's name, a menu of the hosts with a check on the
// current one. It shows only where there is another host to choose, before
// the project; a session stays on its host, so there it names the host and
// is disabled. A draft whose host went away is read-only and may still move
// to another. Another host has its own projects and models: the draft goes
// to that host's chats and takes its default model.
func (s *State) hostMenu(c *ui.Context, ctx *Context, form controlForm) {
	if len(ctx.Hosts) < 2 {
		return
	}
	current := ctx.Hosts[0]
	for _, h := range ctx.Hosts {
		if h.ID == s.Host || s.Host == "" && h.Local {
			current = h
		}
	}
	label := hostLabel(c, current)
	kit.MenuButton(c.Key("host"), s.menuID("host"), func(b ui.Element) {
		contextButton(c, b.Disabled(ctx.LocationLocked), icons.Cpu, label, form)
	}, func(m *kit.Menu) {
		for _, h := range ctx.Hosts {
			if m.Item(hostLabel(c, h)).Checked(h.ID == current.ID).Chosen() && h.ID != current.ID {
				s.Host, s.Project, s.Worktree = h.ID, "", ""
				s.Model, s.Team, s.MemberModels = "", "", nil
			}
		}
	}, kit.MenuOptions{At: ui.AnchorTopStart, Self: ui.AnchorBottomStart})
}

// projectButton is the location, the one project control of the bar: a
// package's control of the same choice is not drawn beside it (live
// plugin_controls.go). It reads the project's name, or Chats for none,
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
			// With a host control the list is the chosen host's.
			if len(ctx.Hosts) > 1 && pr.Host != "" && s.Host != "" && pr.Host != s.Host {
				continue
			}
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
// check, then New worktree. It is the bar's second control and shows only
// where there is a branch or worktree to choose: the project's folder is a
// Git repository. A chat without a project, a plain folder and a folder not
// yet inspected have none, and where a package's context control chooses the
// branch (Context.PackageBranch) that one shows instead.
func (s *State) worktreeMenu(c *ui.Context, ctx *Context, actions Actions, form controlForm) {
	pr := s.project(ctx)
	if pr == nil || ctx.Repository.Kind != Git || ctx.PackageBranch {
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
		m.Separator()
		if m.Item(i18n.T(c, "location_create")).Icon(icons.Plus).Chosen() {
			actions.CreateWorktree()
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
	modeIcon, modeLabel := s.modeLook(c, ctx)
	hint := i18n.T(c, "composer_settings") + " · " + modeLabel + " · " + i18n.T(c, Permissions[s.Permission].label)
	b := kit.IconButton(c.Key("settings"), icons.Settings2, hint, 32, s.settings).Radius(16).Disabled(ctx.Readonly)
	b.HasPopup(ui.PopupDialog).Expanded(s.settings).OnClick(func() {
		s.settings, s.page, s.layer = !s.settings, settingsMain, layerMain
	})
	panel := kit.Popover(c, b, &s.settings, func() {
		// row_text: a 16 icon, the label, and a 16 chevron, 8 apart, in a
		// small ghost button 32 high.
		row := func(key string, icon *ui.SVG, label string, color ui.Color, page settingsPage) {
			MenuRow(c, kit.Ghost(c.Key(key), label, false), icon, label, color, icons.ChevronRight).
				OnClick(func() { s.page = page })
		}
		back := func() {
			label := i18n.T(c, "composer_back")
			MenuRow(c, kit.Ghost(c.Key("back"), label, false), icons.ChevronLeft, label, p.Foreground, nil).
				OnClick(func() { s.page = settingsMain })
		}
		// option is a choice of a page, the row the mode and permission
		// menus have (kit.ChoiceRow), selected while it is the one chosen.
		option := func(key string, o kit.Choice, pick func()) {
			b := ui.ButtonBase(c.Key(key)).Label(o.Label).FillWidth().AccessSelected(o.Checked).Disabled(o.Disabled)
			o.Highlighted = b.Hovered() || b.Pressed()
			kit.ChoiceRow(c, b, o).OnClick(func() { pick(); s.page = settingsMain })
		}
		choices := func(list []choice, chosen int, warning func(int) bool, pick func(int)) {
			for i, m := range list {
				option(m.label, kit.Choice{Label: i18n.T(c, m.label), Description: i18n.T(c, m.description), Icon: m.icon,
					Tint: m.tint, Checked: i == chosen, Warning: warning(i)}, func() { pick(i) })
			}
		}
		switch s.page {
		case settingsMode:
			back()
			builtin := int(s.Mode)
			if activeMode(ctx) != nil {
				builtin = -1
			}
			choices(Modes, builtin, func(int) bool { return false }, func(i int) { s.selectMode(ctx, Mode(i)) })
			for _, m := range ctx.PluginModes {
				icon := m.Icon
				if icon == nil {
					icon = icons.Bot
				}
				option("plugin-"+m.Key, kit.Choice{Label: m.Label, Description: m.Description, Icon: icon, Tint: modeTint(m.Key),
					Checked: m.Selected, Disabled: m.Disabled || ctx.SelectMode == nil}, func() { ctx.SelectMode(m.Key) })
			}
		case settingsPermission:
			back()
			choices(Permissions, int(s.Permission), func(i int) bool { return Permission(i) == FullAccess },
				func(i int) { s.Permission = Permission(i) })
		default:
			pm := Permissions[s.Permission]
			if len(ctx.Models) == 0 {
				// model_settings without a model: Select model, to the settings.
				kit.Ghost(c.Key("select-model"), i18n.T(c, "composer_select_model"), false).FillWidth().Height(32).
					Disabled(ctx.SelectModel == nil).
					Children(func() { ui.Text(c, i18n.T(c, "composer_select_model")).FontSize(14).SingleLine() }).
					OnClick(func() { ctx.SelectModel() })
			} else {
				s.controls(c, ctx, actions)
			}
			row("mode-page", modeIcon, modeLabel, p.Foreground, settingsMode)
			row("permission-page", pm.icon, i18n.T(c, pm.label), permissionColor(p, s.Permission), settingsPermission)
			s.renderPluginControls(c, "composer", ControlMenu)
			if narrow {
				s.hostMenu(c, ctx, menuForm)
				s.projectButton(c, ctx, actions, menuForm)
				s.worktreeMenu(c, ctx, actions, menuForm)
			}
		}
	})
	if panel != (ui.Element{}) {
		// As wide as the controls, which the model picker widens; clipped
		// while it eases to their width.
		width := float32(panelWidth)
		if s.page == settingsMain && len(ctx.Models) > 0 {
			width = s.controlsWidth(c)
		}
		panel.AttachTo(b, ui.AnchorTopStart, ui.AnchorBottomStart).Margin(0, 0, 8, 0).Padding(8).Width(width + 16).Gap(4).Clip()
	}
}

// MenuRow is a control as a row of the settings popover (Form::Menu): a
// small ghost button 32 high and as wide as the panel, a 16 icon, the label
// and a 16 slot for the chevron, 8 apart.
func MenuRow(c *ui.Context, b ui.Element, icon *ui.SVG, label string, color ui.Color, chevron *ui.SVG) ui.Element {
	return b.FillWidth().MinWidth(0).Height(32).PaddingX(8).Gap(8).Justify(ui.Start).TextColor(color).Children(func() {
		ui.Icon(c, icon).FontSize(16).Shrink(0)
		ui.Text(c, label).FontSize(14).SingleLine().Grow(1).MinWidth(0)
		slot := ui.Box(c).Size(16, 16).Shrink(0)
		if chevron != nil {
			slot.Children(func() { ui.Icon(c, chevron).FontSize(16) })
		}
	})
}
