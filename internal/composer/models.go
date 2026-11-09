package composer

import (
	"fmt"
	"strings"
	"unicode"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

// The model chip and its controls (live/model_controls/*, model_picker.rs,
// model_picker/*): the chip names the provider, the model, its effort and
// its features, or the team; its popover holds the team row, the
// features, the effort and its track, and pages for the model picker and
// the team list.

// Chip widths (model_controls/mod.rs CHIP_WIDTH and the narrow one).
const (
	chipWidth   = 260
	narrowChip  = 140
	panelWidth  = 296 // model_picker.rs panel_size: 320 less Kit's popover padding
	panelHeight = 320
	member      = 24 // team.rs MEMBER
	teamRow     = 40 // team.rs ROW_HEIGHT
)

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

// Badge is a provider's initials (model_controls/badge.rs): the first
// letter of each word, of camel-case humps too, up to three; a single
// word gives its first two letters; CJK names keep two characters.
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
	chip := kit.GhostLook(c, ui.ButtonBase(c.Key("model")), s.picker).Label(label).Height(32).PaddingX(12).Gap(8).Radius(16).
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
		panel.AttachTo(chip, ui.AnchorTopEnd, ui.AnchorBottomEnd).Margin(0, 0, 8, 0).Padding(12).Width(panelWidth + 24)
	}
}

// featureBadge is a chosen feature on the chip: Fast's bolt, an option's
// icon, or its name in the emphasis color.
type featureBadge struct {
	label string
	icon  *ui.SVG
}

// chosenFeatures are the features set for the model, Fast first
// (features.rs selected).
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

// controls is the popover's body (model_controls/mod.rs render): its
// height moves between the main page and the picker's and team list's.
func (s *State) controls(c *ui.Context, ctx *Context, actions Actions) {
	h := float32(108 + teamRow)
	if s.layer != layerMain {
		h = panelHeight
	}
	body := ui.Column(c.Key("model-controls")).Width(panelWidth).Height(h).Clip().AlignItems(ui.Center).Gap(8).
		Transition(ui.ElementTransition{Height: true, Duration: 200e6})
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
	m := s.Chosen(ctx.Models)
	list := efforts(m)
	s.teamRow(c, ctx)
	effort := s.EffortName(c, m)
	highest := len(list) > 1 && int(s.Effort) == len(list)-1
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
				resettable := int(s.Effort) != defaultEffort(m) || len(s.Features) > 0
				kit.IconButton(c.Key("reset"), icons.Restore, i18n.T(c, "composer_effort_reset"), 28, false).Radius(14).
					Disabled(!resettable).OnClick(func() {
					s.Effort = float64(defaultEffort(m))
					s.Features = map[string]string{}
				})
			})
		})
		kit.Ghost(c.Key("choose"), m.Name, false).Height(28).PaddingX(8).Gap(4).Radius(14).MaxWidth(panelWidth).Children(func() {
			ui.Text(c, m.Name).FontSize(14).SingleLine().Shrink(1).MinWidth(0)
			ui.Icon(c, icons.ChevronRight).FontSize(12).TextColor(p.MutedForeground)
		}).OnClick(func() {
			s.layer, s.family = layerModels, m.Family
			s.open = map[string]bool{m.Provider: true}
		})
	})
	track(c, &s.Effort, len(list), highest, len(list) < 2)
}

// defaultEffort is the level a model starts at: medium, or the middle.
func defaultEffort(m Model) int {
	list := efforts(m)
	for i, e := range list {
		if e == "medium" {
			return i
		}
	}
	return len(list) / 2
}

// features are features.rs's controls: Fast toggles, a one-option feature
// toggles, others choose from a menu with Off. A model without a speed
// feature shows Fast off, its tooltip saying it does not take it.
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
		current, set := s.Features[f.ID]
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
				// features.rs next: off, then each option in turn.
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
					s.Features[f.ID] = f.Options[i].ID
				} else {
					delete(s.Features, f.ID)
				}
			})
			continue
		}
		kit.MenuButton(c.Key(f.ID), "feature-"+f.ID, func(b ui.Element) {
			kit.Tooltip(c, kit.IconLook(c, b, icon, label, "", 28, set).Radius(14), tip)
		}, func(menu *kit.Menu) {
			if menu.Item(i18n.T(c, "composer_feature_off")).Checked(!set).Chosen() {
				delete(s.Features, f.ID)
			}
			for _, o := range f.Options {
				if menu.Item(name(o)).Checked(set && o.ID == current).Chosen() {
					s.Features[f.ID] = o.ID
				}
			}
		})
	}
}

// teamRow is team.rs team_row: the execution's avatar opening the team
// list, and, right-aligned, the lead and the team's members, the one
// whose model the controls set full, the others faded.
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
}

func memberButton(c *ui.Context, key, name string, icon *ui.SVG, fill ui.Color, overridden, selected bool, pick func()) {
	b := ui.ButtonBase(c.Key("member-" + key)).Label(name)
	kit.Tooltip(c, b, name)
	b.Size(member, member).Radius(member / 2).Cursor(ui.CursorPointer).AccessSelected(selected)
	b.Children(func() { avatar(c, icon, fill, member, selected, overridden) }).OnClick(pick)
}

// avatar is team.rs's Badge over an Avatar: the identity's icon on its
// fill, faded unless selected, with a dot where the model was set apart.
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

// teamList is team.rs team_list: Model only, the default team and the
// teams with their member counts and a check, then Manage teams.
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
			s.Team, s.member, s.layer = "", "", layerMain
		})
		for _, t := range ctx.Teams {
			row("team-"+t.ID, t.Name, icons.Users, kit.AgentColor(c, t.ID), len(t.Members), s.Team == t.ID, func() {
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

// modelPicker is model_picker.rs: the families' rail, with Back to the
// controls, beside the providers' groups, each opening to its models.
// Hovering or clicking a model with efforts opens its efforts' menu at
// its left; a model without them is chosen at once.
func (s *State) modelPicker(c *ui.Context, ctx *Context) {
	p := theme.Of(c)
	if s.open == nil {
		s.open = map[string]bool{}
	}
	var providers []string
	seen := map[string]bool{}
	for _, m := range ctx.Models {
		if (s.family == "" || m.Family == s.family) && !seen[m.Provider] {
			seen[m.Provider] = true
			providers = append(providers, m.Provider)
		}
	}
	ui.Row(c.Key("model-picker")).Width(panelWidth).Grow(1).MinHeight(0).Gap(12).AlignItems(ui.Stretch).Children(func() {
		ui.Scroll(c.Key("families")).Width(48).Shrink(0).Padding(4).Gap(4).Radius(p.Radius2xl()).Background(p.Secondary).
			Children(func() {
				kit.IconButton(c.Key("back"), icons.ArrowLeft, i18n.T(c, "composer_model_settings"), 40, false).Radius(20).
					OnClick(func() { s.layer = layerMain })
				for _, f := range ctx.Families {
					selected := s.family == f.ID
					b := kit.IconButton(c.Key("family-"+f.ID), f.Icon, i18n.T(c, f.Label), 40, false).Radius(20).
						AccessSelected(selected).TextColor(p.MutedForeground)
					if selected {
						b.Background(p.Floating()).TextColor(p.Foreground)
					}
					b.OnClick(func() {
						s.family, s.efforts = f.ID, ""
						s.open = map[string]bool{}
					})
				}
			})
		ui.Scroll(c.Key("groups")).Grow(1).MinWidth(0).MaxHeight(panelHeight).Children(func() {
			if len(providers) == 0 {
				ui.Text(c, i18n.T(c, "composer_model_empty")).FontSize(14).Padding(8).TextColor(p.MutedForeground)
				return
			}
			// open_first: with none of the groups shown open, the first opens.
			any := false
			for _, pr := range providers {
				any = any || s.open[pr]
			}
			if !any {
				s.open[providers[0]] = true
			}
			for _, pr := range providers {
				open := s.open[pr]
				ui.ButtonBase(c.Key("group-"+pr)).Label(pr).FillWidth().Padding(6, 14, 6, 8).Gap(4).Justify(ui.SpaceBetween).
					Expanded(open).Cursor(ui.CursorPointer).TextColor(p.MutedForeground).Children(func() {
					ui.Text(c, pr).FontSize(14).SingleLine()
					chevron := icons.ChevronRight
					if open {
						chevron = icons.ChevronDown
					}
					ui.Icon(c, chevron).FontSize(12)
				}).OnClick(func() { s.open[pr], s.efforts = !open, "" })
				if !open {
					continue
				}
				for _, m := range ctx.Models {
					if m.Provider == pr && (s.family == "" || m.Family == s.family) {
						s.modelRow(c, m)
					}
				}
			}
		})
	})
}

// modelRow is a model of a group: its name and score, selected in the
// emphasis color at 12%.
func (s *State) modelRow(c *ui.Context, m Model) {
	p := theme.Of(c)
	selected := m.ID == s.Model
	look := func(b ui.Element) {
		b.Label(m.Name).FillWidth().MinWidth(0).Height(32).PaddingX(8).Gap(8).Radius(p.RadiusLg).Justify(ui.Start).
			Cursor(ui.CursorPointer).AccessSelected(selected).TextColor(p.PopoverForeground)
		switch {
		case b.Pressed() || selected:
			b.Background(p.Primary.Alpha(0.12))
		case b.Hovered():
			b.Background(p.Hover)
		}
		b.Children(func() {
			ui.Text(c, m.Name).FontSize(14).LineHeight(20.0 / 14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
			if m.Score != nil {
				t := ui.Text(c, fmt.Sprintf("%.1f", m.Score.Overall)).Font("monospace").FontSize(14).Shrink(0).
					TextColor(p.MutedForeground)
				scoreTip(c, t, m.Score)
			}
		})
	}
	list := efforts(m)
	if len(list) == 0 {
		b := ui.ButtonBase(c.Key(m.ID))
		look(b)
		b.OnClick(func() { s.pick(m, 0) })
		return
	}
	id := "efforts-" + m.ID
	b := kit.MenuButton(c.Key(m.ID), id, look, func(menu *kit.Menu) {
		current := defaultEffort(m)
		if selected {
			current = int(s.Effort)
		}
		for i, e := range list {
			it := menu.Item(e).Checked(i == current)
			if m.Score != nil && m.Score.Tested == e {
				it.Trailing(fmt.Sprintf("%.1f", m.Score.Overall))
			}
			if it.Chosen() {
				s.pick(m, i)
			}
		}
	}, kit.MenuOptions{At: ui.AnchorStart, Self: ui.AnchorEnd})
	if b.Hovered() && s.efforts != m.ID {
		if s.efforts != "" {
			kit.OpenMenu(c, "efforts-"+s.efforts, false)
		}
		s.efforts = m.ID
		kit.OpenMenu(c, id, true)
	}
}

// pick chooses a model at an effort and goes back to the controls.
func (s *State) pick(m Model, effort int) {
	s.Model, s.Effort, s.layer, s.efforts = m.ID, float64(effort), layerMain, ""
	s.Features = map[string]string{}
}

// scoreTip is scores/ring.rs's tooltip on our own TooltipBase content:
// the overall score, the five axes and the effort they were measured at.
func scoreTip(c *ui.Context, anchor ui.Element, sc *Score) {
	p := theme.Of(c)
	axes := []string{"model_score_axis_backend", "model_score_axis_frontend", "model_score_axis_knowledge",
		"model_score_axis_aesthetic", "model_score_axis_maintainability"}
	ui.TooltipBase(c, anchor, func(tip ui.Element) {
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
			ui.Text(c, i18n.T(c, "model_score_preview")).FontSize(12).TextColor(p.MutedForeground)
		})
	})
}
