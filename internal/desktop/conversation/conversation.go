// Package conversation is the transcript of a conversation: a virtualized
// list of turns, each a user message and the assistant's reply with its work
// region, tool calls and Markdown answer. The view models and actions are in
// model.go.
package conversation

import (
	"fmt"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Layout metrics.
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
	lists     map[string]*ui.ListState
	open      map[string]*bool
	work      map[string]bool  // work regions the user opened or closed
	built     map[string]bool  // lists built once, so laid out
	md        markdown.Options // the replies' (kit.Markdown fills the rest)
	light     float32          // the welcome wordmark's beam, in window DIPs
	lit       bool             // the pointer has moved over the welcome page
	view      Viewer           // the image viewer
	rewind    string           // the turn whose rewind waits to be confirmed
	reading   string           // the turn read aloud
	find      search           // the search bar
	wantOlder bool             // the start came into view this frame
	nav       navigation       // the message navigation rail
	// Split is whether the transcripts show in the panes of a split, which have
	// no navigation rail.
	Split bool
}

func New() *State {
	return &State{lists: map[string]*ui.ListState{}, open: map[string]*bool{}, work: map[string]bool{}, built: map[string]bool{}}
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
// follows its end, the session's states around them, the search bar and
// the jump button over it, and the image viewer and rewind confirmation
// over the window. sess may be nil.
func View(c *ui.Context, s *State, id string, turns []Turn, sess *Session, actions Actions) {
	s.md.ShareSelection = true
	if sess != nil && sess.Loading {
		loading(c)
		return
	}
	l := s.List(id)
	l.Key = func(i int) any { return turns[i].ID }
	n := len(turns)
	s.wantOlder = false
	banner(c, sess, actions)
	// The theme's conversation effects lie under the turns.
	kit.Draws(ui.Box(c.Key(id)).Grow(1), kit.Backdrop(c, "conversation")).Children(func() {
		ui.List(c, l, n, func(i int) {
			s.turn(c, i, &turns[i], n, sess, actions)
		}).Fill().Role(ui.RoleLog)
		// A delegated session's transcript, the child panel's (no session), is
		// read-only and has no navigation.
		if sess != nil && n > 0 && !s.Split {
			s.nav.view(c, l, turns)
		}
		jump(c, l, n, !s.built[id])
		s.built[id] = true
		s.searchBar(c, id, turns)
	})
	if s.wantOlder && !sess.LoadingOlder {
		actions.LoadOlder()
	}
	s.viewer(c, actions)
	s.confirmRewind(c, actions)
}

func (s *State) turn(c *ui.Context, i int, t *Turn, n int, sess *Session, actions Actions) {
	first, last := i == 0, i == n-1
	top, bottom := float32(0), float32(turnBottom)
	if first {
		top = 24
	}
	if last {
		bottom = 24
	}
	q, current, marked := s.highlight(i, t)
	col := ui.Column(c).FillWidth().MaxWidth(theme.ContentWidth).Margin(0, ui.Auto).Padding(top, padX, bottom, padX).Gap(turnGap).Selectable()
	col.Children(func() {
		if first {
			s.older(c, sess, actions)
		}
		selection(c, col, actions)
		if t.Compaction != nil {
			s.compaction(c, t)
			return
		}
		if t.User != "" || len(t.Images) > 0 || len(t.Files) > 0 {
			ui.Column(c.Key("user")).Gap(8).Children(func() {
				if t.User != "" {
					var hl []ui.Span
					if marked {
						hl = spans(t.User, q, current, theme.Of(c))
					}
					userMessage(c, t.User, hl)
				}
				if len(t.Images) > 0 {
					s.thumbnails(c, "user-images", t.Images, true)
				}
				if len(t.Files) > 0 {
					ui.Row(c).Justify(ui.End).Children(func() {
						ui.Column(c).Width(bubbleMax).Children(func() { files(c, "user-files", t.Files, actions) })
					})
				}
				userFooter(c, t, actions)
			})
		}
		if len(t.Notices) > 0 {
			notices(c, t.Notices)
		}
		if !t.Reply.Notice || len(t.Reply.Blocks) > 0 {
			mark := ""
			if marked {
				mark = q
			}
			s.reply(c, t, mark, actions)
		}
		if last && sess != nil {
			diagnostic(c, sess.Diagnostic)
		}
	})
}

// userMessage is the muted bubble on the right, in the package's user message
// colors, border and radius, with its corner art; marks, when set, are its
// text with the search's matches marked.
func userMessage(c *ui.Context, text string, marks []ui.Span) {
	p := theme.Of(c)
	ui.Row(c).Justify(ui.End).Children(func() {
		var b ui.Element
		if marks != nil {
			b = ui.RichText(c, marks...)
		} else {
			b = ui.Text(c, text)
		}
		b.FontSize(14).LineHeight(1.6).Padding(9, 14).Radius(p.Radius).Background(kit.Veil(c, p.Muted)).TextColor(p.Foreground).MaxWidth(bubbleMax)
		v := p.Variant
		if v == nil {
			return
		}
		if m, ok := v.Messages["user"]; ok {
			b.Background(kit.Veil(c, m.Background.Or(p.Muted))).TextColor(m.Foreground.Or(p.Foreground))
			if m.Radius != nil {
				b.Radius(*m.Radius)
			}
			if m.BorderWidth > 0 {
				b.Border(m.BorderWidth, m.Border.Or(p.Border))
			}
		}
		if v.Asset("message.user.top_left") != nil || v.Asset("message.user.bottom_right") != nil {
			b.Draw(func(pt *ui.Painter, r ui.Rect) { kit.Corners(pt, r, v, "message.user") })
		}
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
// separator, the work region folded under it, the answer, and the footer once
// the run ends.
func (s *State) reply(c *ui.Context, t *Turn, mark string, actions Actions) {
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
			answer := blocks[from:]
			if len(r.Citations) > 0 {
				answer = cited(answer, r.Citations)
			}
			if mark != "" {
				answer = markBlocks(answer, mark)
			}
			ui.Column(c.Key("answer")).Gap(workGap).Children(func() {
				s.blocks(c, t.ID, answer, active, actions)
			})
		}
		if !active && r.Changes != nil {
			s.changes(c, t.ID, r.Changes, actions)
		}
		if active && showPhase(r, blocks) {
			phaseRow(c, r)
		}
		if !active {
			s.footer(c, t, blocks, actions)
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
				kit.Markdown(c, b.Text, kit.MarkdownProps{Variant: kit.Chat, Animated: continuing && last, Options: &s.md})
			case ReasoningBlock:
				head := Call{Label: i18n.T(c, "turn_reasoning"), Icon: "bot", Running: continuing && last}
				s.disclosure(c, "reasoning-"+key, func(hovered, open bool, sw *kit.Sweep) { summary(c, head, hovered, true, open, sw) }, 4, 8, func() {
					ui.Scroll(c).MaxHeight(160).Children(func() {
						ui.Text(c, b.Text).FontSize(14).LineHeight(22.0 / 14).TextColor(p.MutedForeground)
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
			case ImagesBlock:
				s.thumbnails(c, "images", b.Images, false)
			case FilesBlock:
				files(c, "files", b.Files, actions)
			case SearchBlock:
				s.grounding(c, key, b)
			case RetryBlock:
				s.retry(c, key, b.Retry, continuing && last)
			case VisualBlock:
				visual(c, "visual", b.Visual)
			case ErrorBlock:
				head := Call{Label: b.Label, Icon: "triangle-alert", Failed: true}
				s.disclosure(c, "error-"+key, func(hovered, open bool, sw *kit.Sweep) { summary(c, head, hovered, true, open, sw) }, 4, 8, func() {
					ui.Scroll(c).MaxHeight(160).Children(func() {
						ui.Text(c, b.Text).FontSize(14).TextColor(p.Danger)
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
	// One band across the row, past the indicator.
	sw := &kit.Sweep{}
	row := ui.Row(c.Key("phase")).Gap(8).Height(rowHeight)
	row.Children(func() {
		kit.Indicator(c, Indicator(r.Phase), 16)
		sw.Text(c, ui.Text(c, label).FontSize(14).SingleLine().TextColor(p.MutedForeground),
			ui.Span{Text: label, Size: 14, Color: p.MutedForeground})
	})
	sw.Frame(row)
}

// Indicator is the thinking indicator's figure for a phase (ui/thinking
// state): searching while it thinks or waits, composing while it writes,
// working while children run, a still ring otherwise.
func Indicator(ph Phase) kit.Thinking {
	switch ph {
	case Thinking, AwaitingResponse, PhaseNone:
		return kit.Searching
	case Generating:
		return kit.Composing
	case WaitingChildren:
		return kit.Working
	}
	return kit.Breathing
}
