// Package composer is the message composer under the transcript: the input,
// the attachment and dictation buttons, the model picker with its reasoning
// effort, send and stop, and the context bar with the branch menu
// (sailry-public conversation/live/composer.rs, model_controls, theme/composer.rs).
package composer

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

// Efforts are the reasoning levels the picker steps through. The reference
// shows their raw names, untranslated (reasoning.rs).
var Efforts = []string{"minimal", "low", "medium", "high", "xhigh"}

// Branches are the preview's branches (conversation/context.rs).
var Branches = []string{"composer_branch_main", "composer_branch_preview"}

// Width is the composer's maximum width, the transcript's too.
const Width = theme.ContentWidth

// Model is a model the picker offers, as the backend lists it.
type Model struct {
	ID, Name, Provider string
	Reasoning          bool // it takes a reasoning effort
}

// State is the composer's part of the app state.
type State struct {
	Draft   string
	Model   string  // the chosen Model's ID
	Effort  float64 // index into Efforts
	Branch  int     // index into Branches
	Running bool    // a reply is running: Stop replaces Send

	picker bool // the model picker is open
	list   bool // it shows the model list rather than its main panel
	in     interaction
}

func New(model string) *State {
	return &State{Model: model, Effort: 2, in: interaction{reasons: map[string]*string{}}}
}

// EffortName is the effort shown for the chosen model.
func (s *State) EffortName(c *ui.Context, m Model) string {
	if !m.Reasoning {
		return i18n.T(c, "composer_effort_unavailable")
	}
	return Efforts[int(s.Effort)]
}

// View builds the composer: the capsules of what the run waits on, the
// input and its controls, and the context bar. Send gets the trimmed draft.
func View(c *ui.Context, s *State, models []Model, in Interactions, actions Actions) {
	send, stop := actions.Send, actions.Stop
	p := theme.Of(c)
	submit := func() {
		if text := strings.TrimSpace(s.Draft); text != "" && !s.Running {
			s.Draft = ""
			send(text)
		}
	}
	ui.Column(c.Key("composer")).FillWidth().MaxWidth(Width).Margin(0, ui.Auto).Padding(8, 24, 16, 24).Gap(8).Children(func() {
		s.interactions(c, in, actions)
		// theme/composer.rs surface: padding 12, radius_2xl (2.5 x the
		// radius), muted, the package's border width in the input border
		// color, and its corner art under the controls.
		surface := ui.Column(c).Padding(12).Gap(8).Radius(p.Radius2xl()).Background(p.Muted)
		if v := p.Variant; v != nil {
			if w := v.Composer.BorderWidth; w > 0 {
				surface.Border(w, p.Input)
			}
			if v.Asset("composer.top_left") != nil || v.Asset("composer.bottom_right") != nil {
				surface.Draw(func(pt *ui.Painter, r ui.Rect) { kit.Corners(pt, r, v, "composer") })
			}
		}
		surface.Children(func() {
			ui.TextAreaBase(c.Key("draft"), &s.Draft).Label(i18n.T(c, "composer")).Placeholder(i18n.T(c, "composer")).
				PlaceholderColor(p.MutedForeground).Lines(2, 6).FontSize(14).LineHeight(1.5).TextColor(p.Foreground).
				OnTextKey(func(k ui.TextKey) bool {
					if k.Key == ui.KeyEnter && k.Mods == 0 {
						if text := strings.TrimSpace(k.Text()); text != "" && !s.Running {
							k.SetText("")
							s.Draft = ""
							send(text)
						}
						return true
					}
					return false // Shift+Enter breaks the line
				})
			ui.Row(c).MinHeight(32).Gap(4).Children(func() {
				kit.IconButton(c.Key("attach"), icons.Plus, i18n.T(c, "composer_attachment"), 32, false).Radius(16)
				ui.Spacer(c)
				s.modelChip(c, models)
				kit.IconButton(c.Key("dictation"), icons.Microphone, i18n.T(c, "dictation_start"), 32, false).Radius(16)
				s.sendButton(c, submit, stop)
			})
		})
		ui.Row(c).MinHeight(32).MarginX(12).Padding(4, 8).Gap(4).TextColor(p.MutedForeground).Children(func() {
			s.branchMenu(c)
		})
	})
}

func (s *State) sendButton(c *ui.Context, submit, stop func()) {
	v := theme.Of(c).Variant
	if s.Running {
		b := kit.Primary(c.Key("stop"), i18n.T(c, "chat_stop"), false).Size(32, 32).PaddingX(0).Radius(16)
		if v != nil {
			action(b, v.Composer.Stop, false)
		}
		b.Tooltip(i18n.T(c, "chat_stop")).Children(func() { ui.Icon(c, icons.Stop).FontSize(16) }).OnClick(stop)
		return
	}
	empty := strings.TrimSpace(s.Draft) == ""
	b := kit.Primary(c.Key("send"), i18n.T(c, "send"), empty).Size(32, 32).PaddingX(0).Radius(16)
	if v != nil {
		action(b, v.Composer.Send, empty)
	}
	b.Tooltip(i18n.T(c, "send")).Children(func() { ui.Icon(c, icons.ArrowUp).FontSize(16) }).OnClick(submit)
}

// action paints send or stop in the package's colors (theme/composer.rs);
// disabled, the button keeps Kit's look.
func action(b ui.Element, a *theme.Action, disabled bool) {
	if a == nil || disabled {
		return
	}
	bg := a.Background.Or(ui.Transparent)
	switch {
	case b.Pressed():
		bg = a.Active.Or(bg)
	case b.Hovered():
		bg = a.Hover.Or(bg)
	}
	b.Background(bg)
	if a.Foreground.OK {
		b.TextColor(a.Foreground.V)
	}
}

// Chosen returns the model chosen, the first one when the ID is unknown.
func (s *State) Chosen(models []Model) Model {
	for _, m := range models {
		if m.ID == s.Model {
			return m
		}
	}
	return models[0]
}

// modelChip is the round chip naming the provider, the model and the
// effort, which opens the picker above it (model_controls/mod.rs).
func (s *State) modelChip(c *ui.Context, models []Model) {
	p := theme.Of(c)
	m := s.Chosen(models)
	chip := kit.Ghost(c.Key("model"), i18n.T(c, "composer_select_model"), s.picker).Height(32).PaddingX(10).Gap(8).Radius(16).MaxWidth(260)
	chip.HasPopup(ui.PopupDialog).Expanded(s.picker).Children(func() {
		kit.Tag(c, m.Provider).Shrink(0)
		ui.Text(c, m.Name).FontSize(14).SingleLine().Shrink(1)
		ui.Text(c, s.EffortName(c, m)).FontSize(14).SingleLine().Shrink(0).TextColor(p.MutedForeground)
	}).OnClick(func() { s.picker, s.list = !s.picker, false })
	// The reference's popover: 296 wide, above the chip, right-aligned.
	panel := ui.Popover(c, chip, &s.picker, func() {
		if s.list {
			s.modelList(c, models)
		} else {
			s.mainPanel(c, m)
		}
	})
	if panel != (ui.Element{}) {
		panel.AttachTo(chip, ui.AnchorTopEnd, ui.AnchorBottomEnd).Margin(0, 0, 6, 0).Width(296).Gap(8)
	}
}

// mainPanel shows the effort, the model, and the effort slider.
func (s *State) mainPanel(c *ui.Context, m Model) {
	p := theme.Of(c)
	effort := ui.Text(c, s.EffortName(c, m)).FontSize(16).Height(28).TextAlign(ui.Center).TextColor(p.Primary)
	if m.Reasoning && int(s.Effort) == len(Efforts)-1 {
		effort.TextColor(p.Warning)
	}
	kit.Ghost(c.Key("models"), m.Name, false).Height(28).PaddingX(8).Gap(4).Justify(ui.SpaceBetween).Children(func() {
		ui.Text(c, m.Name).FontSize(14).SingleLine()
		ui.Icon(c, icons.ChevronRight).FontSize(12).TextColor(p.MutedForeground)
	}).OnClick(func() { s.list = true })
	ui.StepSlider(c.Key("effort"), &s.Effort, 0, float64(len(Efforts)-1), 1).Label(i18n.T(c, "composer_effort")).
		FillWidth().Disabled(!m.Reasoning)
}

// modelList lists the models by provider; choosing one goes back.
func (s *State) modelList(c *ui.Context, models []Model) {
	p := theme.Of(c)
	ui.Scroll(c).MaxHeight(320).Gap(2).Children(func() {
		last := ""
		for _, m := range models {
			if m.Provider != last {
				last = m.Provider
				ui.Text(c, m.Provider).FontSize(14).TextColor(p.MutedForeground).Padding(6, 14, 6, 8)
			}
			selected := m.ID == s.Model
			row := kit.Ghost(c.Key(m.ID), m.Name, false).Height(32).Padding(0, 8).Radius(p.RadiusLg).Justify(ui.Start).Gap(8)
			if selected {
				row.Background(p.Primary.Alpha(0.12)).AccessSelected(true)
			}
			row.Children(func() {
				ui.Text(c, m.Name).FontSize(14).SingleLine().Grow(1)
				if selected {
					ui.Icon(c, icons.Check).FontSize(14).TextColor(p.Primary)
				}
			}).OnClick(func() { s.Model, s.list = m.ID, false })
		}
	})
}

// branchMenu is the round branch button of the context bar, a menu drawn
// in the window with a check on the branch (conversation/context.rs).
func (s *State) branchMenu(c *ui.Context) {
	icon := icons.Folder
	if s.Branch != 0 {
		icon = icons.Network
	}
	label := i18n.T(c, Branches[s.Branch])
	kit.GhostLook(c, ui.ButtonBase(c.Key("branch")), false).Label(label).Height(28).PaddingX(10).Gap(6).Radius(14).MaxWidth(224).
		TextColor(theme.Of(c).MutedForeground).Children(func() {
		ui.Icon(c, icon).FontSize(14)
		ui.Text(c, label).FontSize(14).SingleLine()
	}).PopoverMenuButton(func(m *ui.Menu) {
		for i, key := range Branches {
			if m.Item(i18n.T(c, key)).Checked(i == s.Branch).Chosen() {
				s.Branch = i
			}
		}
	})
}
