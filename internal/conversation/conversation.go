// Package conversation is the transcript of a conversation: a virtualized
// list of turns, each a user message and the assistant's reply with its
// reasoning, tool calls and Markdown (sailry-public conversation/live.rs,
// live/messages.rs, disclosure.rs, theme/messages.rs).
package conversation

import (
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/sailry/sailry-go/internal/composer"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/preview"
	"github.com/sailry/sailry-go/internal/theme"
)

// Metrics of live/messages.rs and layout.rs.
const (
	padX       = 24
	turnGap    = 10 // user message to reply
	turnBottom = 28 // between turns
	bubbleMax  = 440
)

// State is the transcript's part of the app state: a list state per
// conversation, so each keeps its place, and the reasoning blocks opened.
type State struct {
	lists     map[string]*ui.ListState
	reasoning map[[2]any]*bool
	md        markdown.Options
	themes    map[bool]*markdown.Theme
}

func New() *State {
	return &State{lists: map[string]*ui.ListState{}, reasoning: map[[2]any]*bool{}, themes: map[bool]*markdown.Theme{}}
}

// List returns the list state of a conversation.
func (s *State) List(id string) *ui.ListState {
	if l, ok := s.lists[id]; ok {
		return l
	}
	l := &ui.ListState{FollowEnd: true}
	s.lists[id] = l
	return l
}

// Turns is the number of turns of a conversation: a user message and the
// reply after it.
func Turns(cv *preview.Conversation) int { return (len(cv.Messages) + 1) / 2 }

// View builds the transcript. running says the last reply is still coming.
func View(c *ui.Context, s *State, cv *preview.Conversation, running bool) {
	p := theme.Of(c)
	s.md.Theme = s.markdownTheme(p)
	l := s.List(cv.ID)
	l.Key = func(i int) any { return i }
	n := Turns(cv)
	ui.Box(c.Key(cv.ID)).Grow(1).Children(func() {
		ui.List(c, l, n, func(i int) {
			s.turn(c, cv, i, i == 0, i == n-1, running && i == n-1)
		}).Fill()
		if n > 0 && !l.AtEnd() {
			theme.Ghost(c.Key("latest"), i18n.T(c, "turn_latest"), false).Attach(ui.AnchorBottom, ui.AnchorBottom).
				Bottom(12).Height(28).PaddingX(12).Gap(6).Radius(14).Background(p.Popover).Border(1, p.Border).
				Shadow(0, 4, 12, 0, ui.RGBA(0, 0, 0, 0.12)).Children(func() {
				ui.Icon(c, icons.ArrowDown).FontSize(14)
				ui.Text(c, i18n.T(c, "turn_latest")).FontSize(14).SingleLine()
			}).OnClick(l.ScrollToEnd)
		}
	})
}

func (s *State) turn(c *ui.Context, cv *preview.Conversation, i int, first, last, running bool) {
	top, bottom := float32(0), float32(turnBottom)
	if first {
		top = 24
	}
	if last {
		bottom = 24
	}
	ui.Column(c).FillWidth().MaxWidth(composer.Width).Margin(0, ui.Auto).Padding(top, padX, bottom, padX).Gap(turnGap).Children(func() {
		user := cv.Messages[2*i]
		userMessage(c, user)
		if 2*i+1 < len(cv.Messages) {
			s.reply(c, cv.ID, &cv.Messages[2*i+1], running)
		} else if running {
			phase(c)
		}
	})
}

// userMessage is the muted bubble on the right (theme/messages.rs).
func userMessage(c *ui.Context, m preview.Message) {
	p := theme.Of(c)
	ui.Row(c).Justify(ui.End).Children(func() {
		ui.Text(c, m.Text).Selectable().FontSize(14).LineHeight(1.6).Padding(9, 14).Radius(6).
			Background(p.Muted).TextColor(p.Foreground).MaxWidth(bubbleMax)
	})
}

// reply is the assistant's message: no bubble, its work above the answer.
func (s *State) reply(c *ui.Context, conv string, m *preview.Message, running bool) {
	p := theme.Of(c)
	if m.Text == "" && m.Reasoning == "" && len(m.Tools) == 0 && !running {
		return // a reply stopped before it began
	}
	ui.Column(c).Gap(12).Children(func() {
		if m.Seconds > 0 {
			ui.Row(c).Gap(6).Children(func() {
				ui.Text(c, i18n.T(c, "turn_completed")).FontSize(14).TextColor(p.MutedForeground)
				ui.Textf(c, "%ds", m.Seconds).FontSize(14).TextColor(p.MutedForeground)
			})
			ui.Divider(c).Background(p.Border)
		}
		if m.Reasoning != "" {
			s.reasoningBlock(c, conv, m)
		}
		if len(m.Tools) > 0 {
			ui.Column(c).Gap(8).Children(func() {
				for j, t := range m.Tools {
					ui.Row(c.Key(j)).Children(func() { toolRow(c, t) })
				}
			})
		}
		if running && m.Text == "" {
			phase(c)
			return
		}
		markdown.View(c, m.Text, &s.md)
		ui.Row(c).Gap(8).Children(func() {
			theme.IconButton(c.Key("copy"), icons.Copy, i18n.T(c, "turn_copy"), 28, false).TextColor(p.MutedForeground).
				OnClick(func() {
					c.WriteClipboard(m.Text)
					c.Toast(i18n.T(c, "content_copied"))
				})
		})
	})
}

// reasoningBlock is the collapsed "Reasoning" disclosure; open, its text
// hangs from a rail on the left, at most 160 DIPs high (disclosure.rs).
func (s *State) reasoningBlock(c *ui.Context, conv string, m *preview.Message) {
	p := theme.Of(c)
	key := [2]any{conv, m.ID}
	open, ok := s.reasoning[key]
	if !ok {
		open = new(bool)
		s.reasoning[key] = open
	}
	ui.Column(c.Key("reasoning")).Gap(4).Children(func() {
		parts := ui.CollapsibleBase(c, open)
		hovered := parts.Trigger.Hovered()
		color := p.MutedForeground
		if hovered {
			color = p.Foreground
		}
		parts.Trigger.Gap(6).Height(24).Cursor(ui.CursorPointer).TextColor(color).Children(func() {
			ui.Icon(c, icons.Bot).FontSize(16)
			ui.Text(c, i18n.T(c, "turn_reasoning")).FontSize(14)
			chevron := ui.Icon(c, icons.ChevronRight).FontSize(12).Rotate(90 * parts.Progress())
			if !hovered {
				chevron.Opacity(0)
			}
		})
		parts.Panel(func() {
			ui.Scroll(c).MaxHeight(160).Margin(0, 0, 0, 7.5).Padding(0, 0, 0, 15.5).BorderWidth(0, 0, 0, 1).
				BorderColor(p.Border).Children(func() {
				ui.Text(c, m.Reasoning).Selectable().FontSize(14).LineHeight(22.0 / 14).TextColor(p.MutedForeground)
			})
		})
	})
}

var toolIcons = map[string]*ui.SVG{
	"Read": icons.BookOpen, "Grep": icons.Search, "Edit": icons.Writing, "Bash": icons.Terminal,
}

// toolRow is a tool call: icon, name, its arguments in mono, and its state
// (live/tools.rs, disclosure.rs colors).
func toolRow(c *ui.Context, t preview.Tool) {
	p := theme.Of(c)
	color := p.MutedForeground
	switch t.Status {
	case preview.ToolFailed:
		color = p.Danger
	case preview.ToolRunning:
		color = p.MutedForeground.Mix(p.Foreground, 0.35)
	}
	icon := toolIcons[t.Name]
	if icon == nil {
		icon = icons.Settings2
	}
	ui.Row(c).Height(24).Gap(8).TextColor(color).Children(func() {
		ui.Icon(c, icon).FontSize(16)
		ui.Text(c, t.Name).FontSize(14).SingleLine()
		ui.Text(c, t.Summary).Font("monospace").FontSize(13).SingleLine().MaxWidth(480).Shrink(1).
			TextColor(p.MutedForeground.Alpha(0.65))
		switch t.Status {
		case preview.ToolDone:
			ui.Icon(c, icons.Check).FontSize(12).TextColor(p.Success)
		case preview.ToolFailed:
			ui.Icon(c, icons.TriangleAlert).FontSize(12)
			ui.Text(c, i18n.T(c, "tool_failed")).FontSize(14).SingleLine()
		case preview.ToolRunning:
			theme.Loader(c, 12)
		}
	})
}

// phase is the running reply's row: the orb and what it is doing.
func phase(c *ui.Context) {
	p := theme.Of(c)
	ui.Row(c).Gap(8).Height(24).Children(func() {
		theme.Orb(c, 16)
		ui.Text(c, i18n.T(c, "turn_thinking")).FontSize(14).TextColor(p.MutedForeground)
	})
}

// markdownTheme is the reply's look (crates/content markdown/style.rs): 14
// DIP text on 24 DIP lines, code blocks on a 3.5% foreground tint.
func (s *State) markdownTheme(p *theme.Palette) *markdown.Theme {
	if th, ok := s.themes[p.Dark]; ok {
		return th
	}
	th := &markdown.Theme{
		FontSize:            14,
		LineHeight:          24.0 / 14,
		CodeSize:            13.0 / 14, // ems: Kit's 13 DIP mono size
		CodeLineHeight:      18 / 12.5,
		Text:                p.Foreground,
		Muted:               p.MutedForeground,
		Link:                p.Primary,
		Strong:              p.Primary, // answers emphasize in primary
		Code:                p.Foreground,
		CodeBackground:      p.Muted,
		CodeBlockBackground: p.Foreground.Alpha(0.035),
		CodeBlockBorder:     p.Border,
		CodeBlockRadius:     6,
		CodeBlockPadding:    12.0 / 14, // ems; the reference pads 12 x 10
		Rule:                p.Border,
		TableBorder:         p.Border,
		QuoteBar:            p.Border,
		QuoteText:           p.MutedForeground,
	}
	s.themes[p.Dark] = th
	return th
}
