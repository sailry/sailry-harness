package composer

import (
	"fmt"
	"maps"
	"math"
	"slices"
	"strings"
	"unicode"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The model chip and its controls: the chip names the provider, the model,
// its effort and its features, or the team; its popover holds the team row,
// the features, the effort and its track, and pages for the model picker and
// the team list.

// Chip widths.
const (
	chipWidth   = 260
	narrowChip  = 140
	panelWidth  = 296
	panelHeight = 320
	member      = 24
	teamRow     = 40
)

// The model picker's columns: the rail, the list and the efforts, a hairline
// in a gap of 8 on each side between them. The list is what the page has
// beside the others: listWidth where the window has room for it. The rail
// is its tiles, Back's and a family's alike, and railPad on each side.
const (
	railTile    = 32
	railIcon    = 18
	railPad     = 6
	railWidth   = railTile + 2*railPad
	listWidth   = 236
	effortWidth = 132
	columnGap   = 8
	pickerWidth = railWidth + listWidth + effortWidth + 2*(2*columnGap+1)
	// pickerReserve is what the picker's page leaves of the window's height:
	// the composer under the popover, the popover's own edges and the rows
	// the compact settings popover has under the page.
	pickerReserve = 320
	// windowInset is how far a popover stays from the window's sides.
	windowInset = 8
)

// effortState is the model picker's effort column: the model whose efforts
// it lists and how the pointer and the keys reach it.
type effortState struct {
	// model is the model whose efforts show: the one whose row the pointer
	// or the keys were on last, the chosen one until then.
	model string
	// focused is the model row that had the keyboard focus last frame, so
	// the focus takes the column once, as it comes to a row.
	focused string
	// pointer is where the pointer was in the window in the last frame, and
	// seen tells that it is known: a row takes the column from a pointer
	// that moved since, so one at rest takes nothing from the keys, nor
	// does a row the list brings under it.
	pointer [2]int
	seen    bool
	// from is where the pointer last was on the column's model's row, or
	// since then when its way was read, and direct tells that it has stayed
	// on model rows since: on its way to the column it keeps that model.
	from   [2]int
	direct bool
	// enter gives the chosen model's row the focus as the picker opens, and
	// back gives the shown model's row the focus the column had.
	enter, back bool
}

// layer is the page the model popover shows.
type layer int

const (
	layerMain layer = iota
	layerModels
	layerTeams
)

// team returns the team the draft runs as, nil for the model alone.
func (s *State) team(ctx *Context) *Team {
	for i := range ctx.Teams {
		if ctx.Teams[i].ID == s.Team {
			return &ctx.Teams[i]
		}
	}
	return nil
}

// Badge is a provider's initials: the first letter of each word, of camel-
// case humps too, up to three; a single word gives its first two letters; CJK
// names keep two characters.
func Badge(name string) string {
	var words [][]rune
	var cur []rune
	flush := func() {
		if len(cur) > 0 {
			words = append(words, cur)
			cur = nil
		}
	}
	rs := []rune(name)
	for i, r := range rs {
		switch {
		case !unicode.IsLetter(r) && !unicode.IsDigit(r):
			flush()
		case unicode.IsUpper(r) && i > 0 && unicode.IsLower(rs[i-1]):
			flush()
			cur = append(cur, r)
		default:
			cur = append(cur, r)
		}
	}
	flush()
	switch len(words) {
	case 0:
		return ""
	case 1:
		w := words[0]
		return strings.ToUpper(string(w[:min(2, len(w))]))
	}
	var b []rune
	for _, w := range words[:min(3, len(words))] {
		b = append(b, unicode.ToUpper(w[0]))
	}
	return string(b)
}

// modelChip is model_menu: the round chip that opens the model controls
// above it, right-aligned (Anchor::BottomRight, bottom_2, p_3).
func (s *State) modelChip(c *ui.Context, ctx *Context, actions Actions) {
	p := theme.Of(c)
	m := s.Chosen(ctx.Models)
	narrow := s.width > 0 && s.width < iconWidth
	width := float32(chipWidth)
	if narrow {
		width = narrowChip
	}
	if len(ctx.Models) == 0 {
		// model_label without a configuration: the chip reads Select model
		// and opens the Node's model settings.
		label := i18n.T(c, "composer_select_model")
		kit.GhostLook(c, ui.ButtonBase(c.Key("model")), false).Label(label).Height(32).PaddingX(10).Gap(8).Radius(16).
			MaxWidth(width).MinWidth(0).Shrink(1).Disabled(ctx.Readonly || ctx.SelectModel == nil).
			Children(func() { ui.Text(c, label).FontSize(14).LineHeight(20.0 / 14).SingleLine().Shrink(1).MinWidth(0) }).
			OnClick(func() { ctx.SelectModel() })
		return
	}
	team := s.team(ctx)
	effort := ""
	if len(efforts(m)) > 0 {
		effort = s.EffortName(c, m)
	}
	lead := m.Name
	if effort != "" {
		lead += " · " + effort
	}
	label := i18n.T(c, "composer_model_settings") + ": " + m.Name
	if team != nil {
		label = i18n.T(c, "composer_model_settings") + ": " + team.Name + " · " + lead
	}
	fast, other := s.chosenFeatures(c, m)
	for _, f := range append(fast, other...) {
		label += " · " + f.label
	}
	chip := kit.GhostLook(c, ui.ButtonBase(c.Key("model")), s.picker).Label(label).Height(32).PaddingX(10).Gap(8).Radius(16).
		MaxWidth(width).MinWidth(0).Shrink(1).Disabled(ctx.Readonly)
	if team != nil {
		kit.Tooltip(c, chip, lead)
	}
	badge := func(f featureBadge) {
		if f.icon != nil {
			kit.Tooltip(c, ui.Icon(c, f.icon).FontSize(16).Shrink(0).TextColor(p.Foreground), f.label)
		} else {
			kit.Tooltip(c, ui.Text(c, f.label).FontSize(14).SingleLine().Shrink(0).TextColor(p.Primary), f.label)
		}
	}
	chip.HasPopup(ui.PopupDialog).Expanded(s.picker).Children(func() {
		if team == nil && !narrow {
			kit.Tooltip(c, kit.Tag(c, Badge(m.Provider)).Shrink(0), m.Provider)
		}
		for _, f := range fast {
			badge(f)
		}
		if team != nil {
			avatar(c, icons.Users, kit.AgentColor(c, team.ID), 20, true, false)
			ui.Text(c, team.Name).FontSize(14).SingleLine().Shrink(1).MinWidth(0)
		} else {
			ui.Text(c, m.Name).FontSize(14).SingleLine().Shrink(1).MinWidth(0)
			if effort != "" {
				if narrow {
					kit.Tooltip(c, ui.Icon(c, icons.Brain).FontSize(16).Shrink(0).TextColor(p.MutedForeground), effort)
				} else {
					ui.Text(c, effort).FontSize(14).SingleLine().Shrink(0).TextColor(p.MutedForeground)
				}
			}
		}
		for _, f := range other {
			if !narrow || f.icon != nil {
				badge(f)
			}
		}
	}).OnClick(func() { s.picker, s.layer = !s.picker, layerMain })
	panel := kit.Popover(c, chip, &s.picker, func() { s.controls(c, ctx, actions) })
	if panel != (ui.Element{}) {
		panel.AttachTo(chip, ui.AnchorTopEnd, ui.AnchorBottomEnd).Margin(0, 0, 8, 0).Padding(12).Width(s.popoverWidth(c, 12))
		s.sizePopover(panel)
	}
}

// sizePopover gives a popover of the controls the model picker's size in
// the frame the picker shows: a popover easing to it, as it eases between
// its other pages and back from this one, would have the picker's columns
// and rows past its edge on the way, where a press closes the popover.
func (s *State) sizePopover(panel ui.Element) {
	if s.layer == layerModels {
		kit.Snapped(panel)
	}
}

// popoverWidth is how wide a popover of the controls is, pad inside its
// edge: the controls' page and the padding, and for the model picker its
// three columns, the padding and the edge, as far as the window has room.
// The picker's page fills the popover (controls). In a window too narrow
// for the three columns the rail and the efforts keep their widths and the
// list between them gives up its own.
func (s *State) popoverWidth(c *ui.Context, pad float32) float32 {
	width := panelWidth + 2*pad
	if s.layer != layerModels {
		return width
	}
	w, _ := c.Size()
	return max(width, min(pickerWidth+2*(pad+1), w-2*windowInset))
}

// featureBadge is a chosen feature on the chip: Fast's bolt, an option's
// icon, or its name in the emphasis color.
type featureBadge struct {
	label string
	icon  *ui.SVG
}

// chosenFeatures are the features set for the model, Fast first.
func (s *State) chosenFeatures(c *ui.Context, m Model) (fast, other []featureBadge) {
	for _, f := range m.Features {
		id, ok := s.Features[f.ID]
		if !ok {
			continue
		}
		for _, o := range f.Options {
			if o.ID != id {
				continue
			}
			if f.ID == "speed" && o.ID == "fast" {
				fast = append(fast, featureBadge{i18n.T(c, "composer_feature_fast"), icons.Zap})
				continue
			}
			other = append(other, featureBadge{o.Name, o.Icon})
		}
	}
	return fast, other
}

// controls is the popover's body: its height animates between the main
// page's and the team list's own (up to the picker's); the picker's page
// has its own at once.
func (s *State) controls(c *ui.Context, ctx *Context, actions Actions) {
	// The main page has the team row's divider (1, and a gap of 8); the
	// team list is as tall as its rows, the model picker the full height.
	h := float32(108 + teamRow + 9)
	switch s.layer {
	case layerModels:
		// As tall as the efforts' heading and nine rows, less in a window
		// too short for it, where the models and the efforts scroll.
		_, wh := c.Size()
		h = max(panelHeight/2, min(panelHeight, wh-pickerReserve))
	case layerTeams:
		// Back 28, the rows 32 each, the divider 1 + 8, Manage 32, 4 apart.
		rows := 1 + len(ctx.Teams)
		h = min(float32(28+32*rows+9+32+4*(rows+2)), panelHeight)
	}
	body := ui.Column(c.Key("model-controls")).Height(h).Clip().AlignItems(ui.Center).Gap(8).
		Disabled(ctx.Readonly || s.TeamLocked)
	if s.layer == layerModels {
		// The picker's page fills its popover and has its height at once,
		// as the popover does (sizePopover).
		body.FillWidth().MinWidth(0)
	} else {
		body.Width(panelWidth).Transition(ui.ElementTransition{Height: true, Duration: 200e6})
	}
	body.Children(func() {
		switch s.layer {
		case layerModels:
			s.modelPicker(c, ctx)
		case layerTeams:
			s.teamList(c, ctx, actions)
		default:
			s.mainPanel(c, ctx)
		}
	})
}

// mainPanel is the controls' first page: the team row, then the
// features, the effort with its score and reset, the model to choose
// another, and the effort track.
func (s *State) mainPanel(c *ui.Context, ctx *Context) {
	p := theme.Of(c)
	m := s.controlModel(s.controlModels(ctx))
	effortIndex := s.controlEffort()
	list := efforts(m)
	s.teamRow(c, ctx)
	effort := s.controlEffortName(c, m)
	highest := len(list) > 1 && int(*effortIndex) == len(list)-1
	ui.Column(c.Key("controls")).FillWidth().Shrink(0).Gap(4).AlignItems(ui.Center).Children(func() {
		ui.Box(c.Key("effort-row")).FillWidth().Height(28).Children(func() {
			ui.Row(c.Key("features")).Absolute().Left(0).Top(0).Height(28).Gap(4).AlignItems(ui.Center).Children(func() {
				s.features(c, m)
			})
			e := ui.Text(c.Key("effort"), effort).FontSize(16).Height(28).FillWidth().TextAlign(ui.Center).TextColor(p.Primary)
			if highest {
				e.TextColor(p.Warning)
			}
			if m.Score != nil {
				scoreTip(c, e, m.Score)
			}
			ui.Row(c.Key("reset-row")).Absolute().Right(0).Top(0).Height(28).AlignItems(ui.Center).Children(func() {
				resettable := int(*effortIndex) != defaultEffort(m) || len(s.controlFeatures()) > 0
				kit.IconButton(c.Key("reset"), icons.Restore, i18n.T(c, "composer_effort_reset"), 28, false).Radius(14).
					Disabled(!resettable).OnClick(func() {
					*effortIndex = float64(defaultEffort(m))
					s.setControlFeatures(map[string]string{})
					s.markMemberOverride()
				})
			})
		})
		kit.Ghost(c.Key("choose"), m.Name, false).Height(28).PaddingX(8).Gap(4).Radius(14).MaxWidth(panelWidth).Children(func() {
			ui.Text(c, m.Name).FontSize(14).SingleLine().Shrink(1).MinWidth(0)
			ui.Icon(c, icons.ChevronRight).FontSize(12).TextColor(p.MutedForeground)
		}).OnClick(func() {
			s.layer, s.family = layerModels, m.Family
			// The picker opens on the chosen model's group, whatever was
			// folded the time before.
			s.open = map[string]bool{m.Provider: true}
			s.efforts = effortState{enter: true}
		})
	})
	before := *effortIndex
	track(c, effortIndex, len(list), highest, len(list) < 2)
	if before != *effortIndex {
		s.markMemberOverride()
	}
}

// defaultEffort is the level a model starts at: medium, or the middle.
func defaultEffort(m Model) int {
	list := efforts(m)
	if m.DefaultEffort != "" {
		for i, e := range list {
			if e == m.DefaultEffort {
				return i
			}
		}
	}
	for i, e := range list {
		if e == "medium" {
			return i
		}
	}
	return len(list) / 2
}

// features are Fast toggles, a one-option feature toggles, others choose from
// a menu with Off. A model without a speed feature shows Fast off, its
// tooltip saying it does not take it.
func (s *State) features(c *ui.Context, m Model) {
	speed := false
	for _, f := range m.Features {
		speed = speed || f.ID == "speed"
	}
	if !speed {
		kit.Tooltip(c, kit.IconButton(c.Key("fast-unsupported"), icons.Zap, i18n.T(c, "composer_feature_fast"), 28, false).Radius(14).
			Disabled(true), i18n.T(c, "composer_feature_fast_unsupported"))
	}
	for _, f := range m.Features {
		features := s.controlFeatures()
		current, set := features[f.ID]
		var opt *FeatureOption
		for i := range f.Options {
			if f.Options[i].ID == current {
				opt = &f.Options[i]
			}
		}
		cycles := f.ID == "speed" || len(f.Options) == 1
		name := func(o FeatureOption) string {
			if f.ID == "speed" && o.ID == "fast" {
				return i18n.T(c, "composer_feature_fast")
			}
			if o.Name != "" {
				return o.Name
			}
			return o.ID
		}
		label := f.Name
		icon := f.Icon
		if cycles {
			switch {
			case opt != nil:
				label = name(*opt)
			case len(f.Options) > 0:
				label = name(f.Options[0])
			}
			if f.ID == "speed" {
				icon = icons.Zap
			} else if opt != nil && opt.Icon != nil {
				icon = opt.Icon
			}
		}
		if icon == nil {
			icon = icons.Settings2
		}
		tip := label + " · " + i18n.T(c, "composer_feature_description")
		if cycles && len(f.Options) > 1 && !set {
			tip = i18n.T(c, "composer_feature_off") + " · " + label
		}
		if cycles {
			b := kit.IconButton(c.Key(f.ID), icon, label, 28, set).Radius(14)
			kit.Tooltip(c, b, tip)
			b.AccessSelected(set).OnClick(func() {
				s.markMemberFeatureOverride()
				// off, then each option in turn.
				i := 0
				if set {
					i = len(f.Options)
					for j, o := range f.Options {
						if o.ID == current {
							i = j + 1
						}
					}
				}
				if i < len(f.Options) {
					features[f.ID] = f.Options[i].ID
				} else {
					delete(features, f.ID)
				}
			})
			continue
		}
		kit.MenuButton(c.Key(f.ID), s.menuID("feature-"+f.ID), func(b ui.Element) {
			kit.Tooltip(c, kit.IconLook(c, b, icon, label, "", 28, set).Radius(14), tip)
		}, func(menu *kit.Menu) {
			if menu.Item(i18n.T(c, "composer_feature_off")).Checked(!set).Chosen() {
				delete(features, f.ID)
				s.markMemberFeatureOverride()
			}
			for _, o := range f.Options {
				if menu.Item(name(o)).Checked(set && o.ID == current).Chosen() {
					features[f.ID] = o.ID
					s.markMemberFeatureOverride()
				}
			}
		})
	}
}

// teamRow is the execution's avatar opening the team list, and, right-
// aligned, the lead and the team's members, the one whose model the controls
// set full, the others faded.
func (s *State) teamRow(c *ui.Context, ctx *Context) {
	p := theme.Of(c)
	team := s.team(ctx)
	name, icon, fill := i18n.T(c, "composer_execution_model"), icons.Bot, p.Primary
	if team != nil {
		name, icon, fill = team.Name, icons.Users, kit.AgentColor(c, team.ID)
	}
	ui.Row(c.Key("team-row")).FillWidth().MinWidth(0).Height(32).Shrink(0).Gap(8).AlignItems(ui.Center).Children(func() {
		b := kit.GhostLook(c, ui.ButtonBase(c.Key("team")), false).Label(name)
		kit.Tooltip(c, b, name)
		b.Height(member).PaddingStart(0).PaddingEnd(4).Gap(2).Radius(member / 2).Children(func() {
			avatar(c, icon, fill, member, true, false)
			ui.Icon(c, icons.ChevronRight).FontSize(12)
		}).OnClick(func() { s.layer = layerTeams })
		ui.Spacer(c)
		if team == nil {
			return
		}
		lead := i18n.T(c, "composer_team_lead")
		memberButton(c, "lead", lead, icons.Crown, p.Primary, false, s.member == "", func() { s.member = "" })
		for _, m := range team.Members {
			memberButton(c, m.ID, m.Name, icons.Bot, kit.AgentColor(c, m.ID), m.Overridden, s.member == m.ID,
				func() { s.member = m.ID })
		}
	})
	// A divider under the row, inside the panel's padding, so it stops
	// short of the panel's edges.
	ui.Box(c.Key("team-divider")).FillWidth().Height(1).Shrink(0).Background(p.Border)
}

func memberButton(c *ui.Context, key, name string, icon *ui.SVG, fill ui.Color, overridden, selected bool, pick func()) {
	b := ui.ButtonBase(c.Key("member-" + key)).Label(name)
	kit.Tooltip(c, b, name)
	b.Size(member, member).Radius(member / 2).Cursor(ui.CursorPointer).AccessSelected(selected)
	b.Children(func() { avatar(c, icon, fill, member, selected, overridden) }).OnClick(pick)
}

// avatar is the identity's icon on its fill, faded unless selected, with a
// dot where the model was set apart.
func avatar(c *ui.Context, icon *ui.SVG, fill ui.Color, size float32, selected, overridden bool) {
	p := theme.Of(c)
	a := float32(1)
	if !selected {
		a = 0.35
	}
	ui.Box(c).Size(size, size).Shrink(0).Radius(size / 2).Background(fill.Alpha(a)).Center().Children(func() {
		glyph := ui.Icon(c, icon).FontSize(size * 0.58).TextColor(p.PrimaryForeground)
		if !selected {
			glyph.Opacity(0.6)
		}
		if overridden {
			ui.Box(c).Absolute().Top(-1).Right(-1).Size(8, 8).Radius(4).Background(p.Primary).Border(1, p.Floating())
		}
	})
}

// teamList is Model only, the default team and the teams with their member
// counts and a check, then Manage teams.
func (s *State) teamList(c *ui.Context, ctx *Context, actions Actions) {
	p := theme.Of(c)
	row := func(key, name string, icon *ui.SVG, fill ui.Color, count int, selected bool, pick func()) {
		kit.Ghost(c.Key(key), name, selected).FillWidth().Height(32).PaddingX(8).Gap(8).Justify(ui.Start).
			AccessSelected(selected).Children(func() {
			avatar(c, icon, fill, member, true, false)
			ui.Text(c, name).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
			if count >= 0 {
				ui.Textf(c, "%d", count).FontSize(12).Shrink(0).TextColor(p.MutedForeground)
			}
			ui.Box(c).Size(16, 16).Shrink(0).Children(func() {
				if selected {
					ui.Icon(c, icons.Check).FontSize(16)
				}
			})
		}).OnClick(pick)
	}
	ui.Scroll(c.Key("teams")).FillWidth().Grow(1).Gap(4).Children(func() {
		kit.Ghost(c.Key("back"), i18n.T(c, "composer_back"), false).Height(28).PaddingX(8).Gap(6).Justify(ui.Start).
			Children(func() {
				ui.Icon(c, icons.ArrowLeft).FontSize(14)
				ui.Text(c, i18n.T(c, "composer_back")).FontSize(14)
			}).OnClick(func() { s.layer = layerMain })
		row("model-only", i18n.T(c, "composer_execution_model"), icons.Bot, p.Primary, -1, s.Team == "", func() {
			if s.SelectTeam != nil && !s.SelectTeam("") {
				return
			}
			s.Team, s.member, s.layer = "", "", layerMain
			s.MemberModels = nil
		})
		for _, t := range ctx.Teams {
			row("team-"+t.ID, t.Name, icons.Users, kit.AgentColor(c, t.ID), len(t.Members), s.Team == t.ID, func() {
				if s.SelectTeam != nil {
					if !s.SelectTeam(t.ID) {
						return
					}
				} else if s.Team != t.ID {
					s.MemberModels = nil
				}
				s.Team, s.member, s.layer = t.ID, "", layerMain
			})
		}
		ui.Box(c).FillWidth().Height(1).Margin(4, 0).Background(p.Border).Shrink(0)
		kit.Ghost(c.Key("manage"), i18n.T(c, "composer_team_manage"), false).FillWidth().Height(32).PaddingX(8).Gap(8).
			Justify(ui.Start).TextColor(p.MutedForeground).Children(func() {
			ui.Box(c).Size(member, member).Shrink(0).Center().Children(func() { ui.Icon(c, icons.Settings).FontSize(16) })
			ui.Text(c, i18n.T(c, "composer_team_manage")).FontSize(14).SingleLine()
		}).OnClick(actions.ManageTeams)
	})
}

// modelPicker is three columns, a hairline between them: the families' rail
// under Back to the controls, a tile for each; the providers' groups of the
// rail's family, each folding its models under its name; and the efforts of
// the model the pointer or the keys are on, of the chosen one until then. A
// model's row chooses it at the effort its column checks, an effort's row at
// that effort. Up and Down move in a column, Right or Tab from the models to
// the efforts, Left or Shift+Tab back. The fill alone marks what is chosen
// (Palette.Selected); the rail filters by family only and a score opens
// nothing (docs/removed-ui.md).
func (s *State) modelPicker(c *ui.Context, ctx *Context) {
	p := theme.Of(c)
	e := &s.efforts
	if s.open == nil {
		s.open = map[string]bool{}
	}
	models := s.controlModels(ctx)
	shown := func(m Model) bool { return s.family == "" || m.Family == s.family }
	var providers []string
	seen := map[string]bool{}
	// scored tells that a listed model has a score: every row then keeps
	// the score's place, so the capability marks line up (modelRow).
	scored := false
	for _, m := range models {
		if !shown(m) {
			continue
		}
		scored = scored || m.Score != nil
		if !seen[m.Provider] {
			seen[m.Provider] = true
			providers = append(providers, m.Provider)
		}
	}
	// open_first: a list whose groups were neither opened nor folded shows
	// its first one open. A group the user folded has its entry, false, so
	// it stays folded, here and when the rail comes back to its family; the
	// reference opens the first group as the list is revealed and as the
	// family changes, never under a fold.
	if len(providers) > 0 && !slices.ContainsFunc(providers, func(pr string) bool { _, set := s.open[pr]; return set }) {
		s.open[providers[0]] = true
	}
	// The column's model: the one a row gave it, while that row is in the
	// list, else the chosen one.
	chosen := s.controlModel(models)
	column := chosen
	for _, m := range models {
		if m.ID == e.model && shown(m) && s.open[m.Provider] {
			column = m
		}
	}
	if e.model != column.ID {
		// The column is the chosen model's again: where the pointer was
		// headed from another's row tells nothing now.
		e.model, e.direct = column.ID, false
	}
	line := func(key string) {
		ui.Box(c.Key(key)).Width(1).Shrink(0).Background(p.Border)
	}
	picker := ui.Row(c.Key("model-picker")).FillWidth().Grow(1).MinHeight(0).Gap(columnGap).AlignItems(ui.Stretch)
	picker.Children(func() {
		ui.Scroll(c.Key("families")).Width(railWidth).Shrink(0).Padding(railPad).Gap(4).Radius(p.Radius2xl()).Background(p.Secondary).
			Children(func() {
				kit.IconButton(c.Key("back"), icons.ArrowLeft, i18n.T(c, "composer_model_settings"), railTile, false).Radius(p.RadiusLg).
					OnClick(func() { s.layer = layerMain })
				for _, f := range ctx.Families {
					// A family's tile: its mark in a rounded square the
					// size of Back, filled as a chosen row while it is the
					// rail's family.
					selected, label := s.family == f.ID, i18n.T(c, f.Label)
					b := ui.ButtonBase(c.Key("family-"+f.ID)).Label(label).Size(railTile, railTile).Radius(p.RadiusLg).Center().
						Cursor(ui.CursorPointer).AccessSelected(selected).TextColor(p.MutedForeground)
					kit.Tooltip(c, b, label)
					switch {
					case selected || b.Pressed():
						b.Background(p.Selected()).TextColor(p.Foreground)
					case b.Hovered():
						b.Background(p.Hover)
					}
					// The folds stay as the family changes: a group folded
					// in one is folded when the rail comes back to it.
					b.Children(func() { ui.Icon(c, f.Icon).FontSize(railIcon) }).
						OnClick(func() { s.family, e.model = f.ID, "" })
				}
			})
		line("rail-line")
		rows := map[string]ui.Element{}
		var first ui.Element
		list := ui.Scroll(c.Key("groups")).Grow(1).MinWidth(0).MaxHeight(panelHeight).FocusGroup(ui.Vertical)
		list.Children(func() {
			if len(providers) == 0 {
				ui.Text(c, i18n.T(c, "composer_model_empty")).FontSize(14).Padding(8).TextColor(p.MutedForeground)
				return
			}
			for i, pr := range providers {
				open := s.open[pr]
				// A group's name folds and unfolds its models, by a click,
				// Enter or Space: a disclosure, expanded while they show,
				// its chevron down then and to the right while folded. The
				// fold is kept in s.open, false for a folded group.
				group := ui.ButtonBase(c.Key("group-"+pr)).Role(ui.RoleDisclosure).Label(pr).FillWidth().Padding(6, 14, 6, 8).Gap(4).
					Justify(ui.SpaceBetween).Expanded(open).Cursor(ui.CursorPointer).TextColor(p.MutedForeground).Children(func() {
					ui.Text(c, pr).FontSize(14).SingleLine()
					chevron := icons.ChevronRight
					if open {
						chevron = icons.ChevronDown
					}
					ui.Icon(c, chevron).FontSize(12)
				}).OnClick(func() { s.open[pr], e.model = !open, "" })
				if i == 0 {
					first = group
				}
				if !open {
					continue
				}
				for _, m := range models {
					if m.Provider == pr && shown(m) {
						rows[m.ID] = s.modelRow(c, m, chosen, scored)
					}
				}
			}
		})
		// The picker opens with the focus on the chosen model's row, so the
		// keys move from it.
		if row, ok := rows[chosen.ID]; ok && e.enter {
			row.Focus()
		}
		e.enter = false
		// The focus takes the column as it comes to a row, the pointer as it
		// moves onto one or on it. The picker reads the pointer wherever it
		// is, so the first move onto a row is known for one.
		x, y, _ := picker.PointerPosition()
		box := picker.Bounds()
		at := [2]int{int(math.Round(float64(box.X + x))), int(math.Round(float64(box.Y + y)))}
		moved := e.seen && at != e.pointer
		e.pointer, e.seen = at, true
		focused, hovered := "", false
		for id, row := range rows {
			if row.Focused() {
				if focused = id; e.focused != id {
					e.model, e.direct = id, false
				}
			}
		}
		e.focused = focused
		for id, row := range rows {
			if row.Hovered() {
				hovered = true
				if e.aim(id, at, moved) {
					e.model = id
				}
			}
		}
		if !hovered {
			e.direct = false
		}
		for _, m := range models {
			if m.ID == e.model {
				column = m
			}
		}
		levels := efforts(column)
		// Right and Tab go to the efforts, where there are any; Left comes
		// back to the model they are of.
		right, tab := false, false
		if len(levels) > 0 {
			right, tab = list.Shortcut(0, ui.KeyRight), list.Shortcut(0, ui.KeyTab)
		}
		row, listed := rows[column.ID]
		// The column's model stays marked while the pointer or the focus is
		// in the column.
		if listed && column.ID != chosen.ID && !row.Pressed() {
			row.Background(p.Hover)
		}
		switch {
		case !e.back:
		case listed:
			row.Focus()
		case len(providers) > 0:
			// The chosen model is not in the list: its first group.
			first.Focus()
		}
		e.back = false
		line("effort-line")
		s.effortColumn(c, column, chosen, right || tab)
	})
}

// aimStep is how far the pointer goes before its way tells where it is
// headed.
const aimStep = 6

// aim follows the pointer to at, in the window, on the row of the model id,
// and reports whether the row takes the effort column: it does once the
// pointer moved. From the column's own row, a pointer headed for the
// column, more to the right than up or down, crosses the rows between
// without giving them the column.
func (e *effortState) aim(id string, at [2]int, moved bool) bool {
	switch {
	case id == e.model:
		e.from, e.direct = at, true
		return false
	case !moved:
		return false
	}
	if dx, dy := at[0]-e.from[0], at[1]-e.from[1]; e.direct {
		if dx*dx+dy*dy < aimStep*aimStep {
			return false
		}
		if dx > max(dy, -dy) {
			e.from = at
			return false
		}
	}
	e.from, e.direct = at, true
	return true
}

// rowPad is a picker row's padding at each end: its score ends there.
const rowPad = 8

// scoreWidth is the place a model's row keeps for its score, a digit, the
// point and a decimal at 14 in the monospace font, ending at the row's
// padding: a row without a score keeps it empty, so its capability marks
// stand where the other rows' do.
const scoreWidth = 28

// rowLook is a row of the picker, a model's or an effort's: 32 high, its
// content from rowPad. The chosen one is told by its fill alone, the
// neutral Palette.Selected, with no check; assistive technology has it as
// selected.
func rowLook(c *ui.Context, b ui.Element, selected bool) ui.Element {
	p := theme.Of(c)
	b.FillWidth().MinWidth(0).Height(32).Shrink(0).PaddingX(rowPad).Gap(8).Radius(p.RadiusLg).Justify(ui.Start).
		Cursor(ui.CursorPointer).AccessSelected(selected).TextColor(p.PopoverForeground)
	if b.Pressed() || selected {
		b.Background(p.Selected())
	}
	return b
}

// score is a row's last child: the score in the monospace font, muted, its
// digits ending at the row's padding. It is text alone: nothing opens
// over it (docs/removed-ui.md).
func score(c *ui.Context, sc *Score) ui.Element {
	return ui.Text(c, fmt.Sprintf("%.1f", sc.Overall)).Font("monospace").FontSize(14).SingleLine().Shrink(0).
		TextColor(theme.Of(c).MutedForeground)
}

// modelRow is a model of a group: its name, then at the row's end its
// capabilities and, last, its score. scored tells that a model of the list
// has a score: a row without one then keeps the score's place under its
// capabilities. It chooses the model at the effort its column checks.
func (s *State) modelRow(c *ui.Context, m, chosen Model, scored bool) ui.Element {
	p := theme.Of(c)
	b := rowLook(c, ui.ButtonBase(c.Key(m.ID)).Label(m.Name), m.ID == chosen.ID)
	b.Children(func() {
		ui.Text(c, m.Name).FontSize(14).LineHeight(20.0 / 14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
		for _, capability := range m.Capabilities {
			label := i18n.T(c, capability.Label)
			kit.Tooltip(c, ui.Icon(c.Key(capability.ID), capability.Icon).FontSize(12).Shrink(0).TextColor(p.MutedForeground).Label(label), label)
		}
		switch {
		case m.Score != nil:
			score(c, m.Score).MinWidth(scoreWidth).TextAlign(ui.End)
		case scored && len(m.Capabilities) > 0:
			ui.Box(c.Key("no-score")).Width(scoreWidth).Shrink(0)
		}
	}).OnClick(func() { s.pick(m, s.carried(m, chosen)) })
	return b
}

// carried is the effort a model is chosen at from its row, the one its
// column checks: the effort chosen now where the model has one of its name,
// which the chosen model does, else the model's default.
func (s *State) carried(m, chosen Model) int {
	list, now := efforts(m), efforts(chosen)
	if len(list) == 0 {
		return 0
	}
	if len(now) > 0 {
		if i := slices.Index(list, now[min(max(int(*s.controlEffort()), 0), len(now)-1)]); i >= 0 {
			return i
		}
	}
	return defaultEffort(m)
}

// effortColumn is the efforts of a model under their heading, rows as the
// models' are: the level and, at the row's end, the score measured at it.
// The one the model's row chooses is filled and selected. A model without
// efforts has a muted line for them, so the column keeps its place. focus
// gives the selected row the keyboard focus.
func (s *State) effortColumn(c *ui.Context, m, chosen Model, focus bool) {
	p := theme.Of(c)
	name := i18n.T(c, "composer_effort")
	list := efforts(m)
	current := s.carried(m, chosen)
	// It scrolls, as the models do: a model declares as many levels as it
	// has.
	column := ui.Scroll(c.Key("efforts")).Width(effortWidth).Shrink(0).MaxHeight(panelHeight).Role(ui.RoleListBox).Label(name).
		FocusGroup(ui.Vertical)
	column.Children(func() {
		// As tall as a group's name, so the rows line up with the models'.
		ui.Row(c.Key("heading")).FillWidth().Height(32).Shrink(0).PaddingX(8).AlignItems(ui.Center).Children(func() {
			ui.Text(c, name).FontSize(12).SingleLine().Shrink(1).MinWidth(0).TextColor(p.MutedForeground)
		})
		if len(list) == 0 {
			ui.Text(c.Key("none"), i18n.T(c, "composer_effort_unavailable")).FontSize(14).LineHeight(20.0/14).
				Padding(6, 8).TextColor(p.MutedForeground)
			return
		}
		for i, level := range list {
			selected := i == current
			b := rowLook(c, ui.ButtonBase(c.Key("effort-"+level)).Label(level).Role(ui.RoleOption), selected)
			if b.Hovered() && !selected && !b.Pressed() {
				b.Background(p.Hover)
			}
			if focus && selected {
				b.Focus()
			}
			b.Children(func() {
				ui.Text(c, level).FontSize(14).LineHeight(20.0 / 14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
				measured := m.Score
				if at, ok := m.Scores[level]; ok {
					measured = at
				}
				if measured != nil && measured.Tested == level {
					score(c, measured)
				}
			}).OnClick(func() { s.pick(m, i) })
		}
	})
	if column.Shortcut(0, ui.KeyLeft) {
		s.efforts.back = true
	}
}

// pick chooses a model at an effort and goes back to the controls.
func (s *State) pick(m Model, effort int) {
	if member := s.memberSelection(); member != nil {
		features := maps.Clone(member.Features)
		if member.InheritFeatures {
			features = maps.Clone(s.Features)
		}
		for id, selected := range features {
			if !slices.ContainsFunc(m.Features, func(feature Feature) bool {
				return feature.ID == id && slices.ContainsFunc(feature.Options, func(option FeatureOption) bool { return option.ID == selected })
			}) {
				delete(features, id)
			}
		}
		member.Model, member.Effort, member.Features, member.Overridden = m.ID, float64(effort), features, true
	} else {
		s.Model, s.Effort, s.Features = m.ID, float64(effort), map[string]string{}
	}
	s.layer, s.efforts = layerMain, effortState{}
}

// scoreTip is the overall score, the five axes and the effort they were
// measured at, over the controls' effort. The picker's scores have none
// (docs/removed-ui.md).
func scoreTip(c *ui.Context, anchor ui.Element, sc *Score) {
	p := theme.Of(c)
	axes := []string{"model_score_axis_backend", "model_score_axis_frontend", "model_score_axis_knowledge",
		"model_score_axis_aesthetic", "model_score_axis_maintainability"}
	if kit.TipsHidden(c) {
		return
	}
	ui.TooltipBase(c, anchor, func(tip ui.Element) {
		kit.Anchored(tip, anchor, kit.TipLayer)
		kit.Attach(kit.Floating(c, tip), anchor, kit.Top, 6).Padding(8, 10).Gap(4).Radius(p.Radius).Width(200)
		tip.Children(func() {
			ui.Row(c).FillWidth().Justify(ui.SpaceBetween).Children(func() {
				ui.Text(c, i18n.T(c, "model_score_overall")).FontSize(12).TextColor(p.MutedForeground)
				ui.Text(c, fmt.Sprintf("%.1f", sc.Overall)).Font("monospace").FontSize(14).FontWeight(600)
			})
			for i, a := range axes {
				ui.Row(c.Key(a)).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
					ui.Text(c, i18n.T(c, a)).FontSize(12).Width(96).SingleLine()
					ui.Box(c).Grow(1).Height(4).Radius(2).Background(p.Border).Children(func() {
						ui.Box(c).WidthPercent(float32(sc.Axes[i] * 10)).FillHeight().Radius(2).Background(p.Primary)
					})
					ui.Text(c, fmt.Sprintf("%.1f", sc.Axes[i])).Font("monospace").FontSize(12).Width(28).TextAlign(ui.End)
				})
			}
			if sc.Tested != "" {
				ui.Text(c, i18n.T(c, "model_score_tested", "effort", sc.Tested)).FontSize(12).TextColor(p.MutedForeground)
			}
			if !kit.Demo(c) {
				ui.Text(c, i18n.T(c, "model_score_preview")).FontSize(12).TextColor(p.MutedForeground)
			}
		})
	})
}
