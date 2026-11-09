// Package conversation is the transcript of a conversation: a virtualized
// list of turns, each a user message and the assistant's reply with its
// work region, tool calls and Markdown answer (sailry-public
// conversation/live.rs, live/messages.rs, live/tools.rs, disclosure.rs,
// surface/*). The view models and actions are in model.go.
package conversation

import (
	"fmt"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

// Metrics of live/messages.rs and layout.rs.
const (
	padX       = 24
	turnGap    = 10 // user message to reply
	turnBottom = 28 // between turns
	bubbleMax  = 440
	workGap    = 16 // between the blocks of the work region
)

// State is the transcript's UI state: a list state per conversation, so
// each keeps its place, and the disclosures opened. It holds no
// conversation data.
type State struct {
	lists  map[string]*ui.ListState
	open   map[string]*bool
	work   map[string]bool // work regions the user opened or closed
	built  map[string]bool // lists built once, so laid out
	md     markdown.Options
	themes map[bool]*markdown.Theme
}

func New() *State {
	return &State{lists: map[string]*ui.ListState{}, open: map[string]*bool{}, work: map[string]bool{}, built: map[string]bool{}, themes: map[bool]*markdown.Theme{}}
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

// View builds the transcript of conversation id: its turns in a list that
// follows its end, and the jump button over it.
func View(c *ui.Context, s *State, id string, turns []Turn, actions Actions) {
	s.md.Theme = s.markdownTheme(theme.Of(c))
	l := s.List(id)
	l.Key = func(i int) any { return turns[i].ID }
	n := len(turns)
	ui.Box(c.Key(id)).Grow(1).Children(func() {
		ui.List(c, l, n, func(i int) {
			s.turn(c, &turns[i], i == 0, i == n-1, actions)
		}).Fill().Role(ui.RoleLog)
		jump(c, l, n, !s.built[id])
		s.built[id] = true
	})
}

func (s *State) turn(c *ui.Context, t *Turn, first, last bool, actions Actions) {
	top, bottom := float32(0), float32(turnBottom)
	if first {
		top = 24
	}
	if last {
		bottom = 24
	}
	ui.Column(c).FillWidth().MaxWidth(theme.ContentWidth).Margin(0, ui.Auto).Padding(top, padX, bottom, padX).Gap(turnGap).Children(func() {
		if t.User != "" {
			userMessage(c, t.User)
		}
		s.reply(c, t, actions)
	})
}

// userMessage is the muted bubble on the right (theme/messages.rs).
func userMessage(c *ui.Context, text string) {
	p := theme.Of(c)
	ui.Row(c).Justify(ui.End).Children(func() {
		ui.Text(c, text).Selectable().FontSize(14).LineHeight(1.6).Padding(9, 14).Radius(6).
			Background(p.Muted).TextColor(p.Foreground).MaxWidth(bubbleMax)
	})
}

// statusKey is the run's status label (frame::status).
func statusKey(st RunStatus) string {
	switch st {
	case Queued:
		return "turn_queued"
	case Running:
		return "chat_running"
	case Stopping:
		return "chat_stopping"
	case Continued:
		return "turn_continued"
	case Cancelled:
		return "turn_cancelled"
	case Interrupted:
		return "chat_interrupted"
	case Failed:
		return "turn_failed"
	case Unsynced:
		return "chat_status_unsynced"
	}
	return "turn_completed"
}

// Elapsed formats a run's time as the reference does: 9s, 1m05s, 1h02m05s.
func Elapsed(d time.Duration) string {
	s := int(d / time.Second)
	switch {
	case s < 60:
		return fmt.Sprintf("%ds", s)
	case s < 3600:
		return fmt.Sprintf("%dm%02ds", s/60, s%60)
	}
	return fmt.Sprintf("%dh%02dm%02ds", s/3600, s/60%60, s%60)
}

// reply is the assistant's message: a header with the run's status over a
// separator, the work region folded under it, the answer, and the footer
// once the run ends (messages.rs render_turn).
func (s *State) reply(c *ui.Context, t *Turn, actions Actions) {
	p := theme.Of(c)
	r := &t.Reply
	blocks := Compact(r.Blocks)
	from := AnswerFrom(blocks)
	active := r.Status.Active()
	ui.Column(c.Key("reply")).Gap(8).Children(func() {
		// The header: the status, its time, and the work region's trigger.
		ui.Column(c).Gap(8).Children(func() {
			key := "work-" + t.ID
			workOpen, chosen := s.work[key]
			if !chosen {
				workOpen = active // open while the run goes on, until the user chooses
			}
			label := i18n.T(c, statusKey(r.Status))
			status := func() {
				row := ui.Row(c).Gap(8).Grow(1).MinWidth(0)
				if r.Status == Failed || r.Status == Interrupted {
					row.TextColor(p.Danger)
				}
				row.Children(func() {
					ui.Text(c, label).FontSize(14).SingleLine()
					if r.Elapsed > 0 {
						ui.Text(c, Elapsed(r.Elapsed)).FontSize(14).SingleLine()
					}
				})
			}
			if from == 0 {
				ui.Row(c).Height(rowHeight).TextColor(p.MutedForeground).Children(status)
			} else {
				b := ui.ButtonBase(c.Key("work")).Label(label).Height(rowHeight).FillWidth().Gap(8).
					Justify(ui.Start).Expanded(workOpen).Cursor(ui.CursorPointer).TextColor(p.MutedForeground)
				hovered := b.Hovered() || b.FocusVisible()
				b.Children(func() {
					status()
					chevron := icons.ChevronRight
					if workOpen {
						chevron = icons.ChevronDown
					}
					ch := ui.Icon(c, chevron).FontSize(16).Shrink(0)
					if !hovered {
						ch.Opacity(0)
					}
				}).OnClick(func() { s.work[key] = !workOpen })
			}
			ui.Divider(c).Background(p.Border)
			if from > 0 && workOpen {
				ui.Column(c.Key("work-body")).Gap(workGap).Children(func() {
					s.blocks(c, t.ID, blocks[:from], active && from == len(blocks), actions)
				})
			}
		})
		if from < len(blocks) {
			ui.Column(c.Key("answer")).Gap(workGap).Children(func() {
				s.blocks(c, t.ID, blocks[from:], active, actions)
			})
		}
		if active && showPhase(r, blocks) {
			phaseRow(c, r)
		}
		if !active {
			footer(c, t, blocks, actions)
		}
	})
}

// blocks builds blocks in order. continuing marks the latest tool row
// running while the turn goes on.
func (s *State) blocks(c *ui.Context, turn string, blocks []Block, continuing bool, actions Actions) {
	p := theme.Of(c)
	for i, b := range blocks {
		key := fmt.Sprintf("%s-%d", turn, i)
		last := i == len(blocks)-1
		ui.Column(c.Key(key)).Children(func() {
			switch b.Kind {
			case TextBlock:
				markdown.View(c, b.Text, &s.md)
			case ReasoningBlock:
				head := Call{Label: i18n.T(c, "turn_reasoning"), Icon: "bot", Running: continuing && last}
				s.disclosure(c, "reasoning-"+key, func(hovered, open bool) { summary(c, head, hovered, true, open) }, 4, 8, func() {
					ui.Scroll(c).MaxHeight(160).Children(func() {
						ui.Text(c, b.Text).Selectable().FontSize(14).LineHeight(22.0 / 14).TextColor(p.MutedForeground)
					})
				})
			case ToolsBlock:
				s.tools(c, b.Calls, continuing && last)
			case QuestionBlock:
				s.question(c, b.Question)
			case ChildrenBlock:
				s.children(c, b.Children, actions)
			case ProgressBlock:
				s.steps(c, key, b.Steps, continuing)
			case ErrorBlock:
				head := Call{Label: b.Label, Icon: "triangle-alert", Failed: true}
				s.disclosure(c, "error-"+key, func(hovered, open bool) { summary(c, head, hovered, true, open) }, 4, 8, func() {
					ui.Scroll(c).MaxHeight(160).Children(func() {
						ui.Text(c, b.Text).Selectable().FontSize(14).TextColor(p.Danger)
					})
				})
			}
		})
	}
}

// showPhase hides the phase row while the run waits on the user or a
// call or child runs: their rows already say what goes on.
func showPhase(r *Reply, blocks []Block) bool {
	for _, b := range blocks {
		for _, call := range b.Calls {
			if call.Pending || call.Running {
				return false
			}
		}
		for _, k := range b.Children {
			if k.Status == ChildRunning {
				return false
			}
		}
		if b.Question != nil && b.Question.State == QuestionPending {
			return false
		}
	}
	return true
}

// phaseRow is the active run's last row: the orb and what it does.
func phaseRow(c *ui.Context, r *Reply) {
	p := theme.Of(c)
	var label string
	switch r.Phase {
	case Generating:
		label = i18n.T(c, "turn_generating")
	case AwaitingResponse:
		label = i18n.T(c, "turn_awaiting_response")
	case Compacting:
		label = i18n.T(c, "turn_compacting")
	case WaitingChildren:
		if r.Waiting == 1 {
			label = i18n.T(c, "turn_waiting_child")
		} else {
			label = i18n.T(c, "turn_waiting_children", "count", r.Waiting)
		}
	default:
		label = i18n.T(c, "turn_thinking")
	}
	ui.Row(c.Key("phase")).Gap(8).Height(rowHeight).Children(func() {
		kit.Orb(c, 16)
		kit.Shimmer(c, label, 14, p.MutedForeground)
	})
}

// footer is the ended run's actions: copy the answer, and retry a run
// that failed, stopped or broke off.
func footer(c *ui.Context, t *Turn, blocks []Block, actions Actions) {
	p := theme.Of(c)
	var parts []string
	for _, b := range blocks[AnswerFrom(blocks):] {
		if b.Kind == TextBlock {
			parts = append(parts, b.Text)
		}
	}
	text := strings.Join(parts, "\n\n")
	ui.Row(c.Key("footer")).Gap(8).Children(func() {
		if text != "" {
			kit.IconButton(c.Key("copy"), icons.Copy, i18n.T(c, "turn_copy"), 28, false).TextColor(p.MutedForeground).
				OnClick(func() {
					c.WriteClipboard(text)
					c.Toast(i18n.T(c, "content_copied"))
				})
		}
		switch t.Reply.Status {
		case Failed, Interrupted, Cancelled:
			kit.IconButton(c.Key("retry"), icons.RotateCw, i18n.T(c, "chat_retry"), 28, false).TextColor(p.MutedForeground).
				OnClick(func() { actions.Retry(t.ID) })
		}
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
