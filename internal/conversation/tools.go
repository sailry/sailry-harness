package conversation

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

// Row metrics of disclosure.rs.
const (
	rowHeight  = 24
	rowIcon    = 16
	caretSize  = 12
	detailMax  = 480 // mono details
	outputMax  = 100 // 5 lines of 20 DIPs
	diffRow    = 13 * 1.6
	railMargin = 7.5  // 8 - 0.5: the rail's line sits under the icon's middle
	railPad    = 15.5 // 8 + 8 - 0.5: the body starts where the label does
)

// opened returns the open flag of a disclosure, closed at first.
func (s *State) opened(key string) *bool {
	if b, ok := s.open[key]; ok {
		return b
	}
	b := new(bool)
	s.open[key] = b
	return b
}

// rowColor is the color of a row's icon and label, first match winning.
func rowColor(p *theme.Palette, call Call) ui.Color {
	switch {
	case call.Failed:
		return p.Danger
	case call.Pending || call.Warning:
		return p.Warning
	case call.Running:
		return p.MutedForeground.Mix(p.Foreground, 0.35)
	}
	return p.MutedForeground
}

// stateText is the state a row names after its detail; pending and failed
// rows say it by color instead.
func stateText(c *ui.Context, st RowState) string {
	switch st {
	case Denied:
		return i18n.T(c, "approval_rejected")
	case StateInterrupted:
		return i18n.T(c, "tool_interrupted")
	case NotExecuted:
		return i18n.T(c, "tool_not_executed")
	}
	return ""
}

// summary builds a row's content: icon, label, detail, state and caret.
// A running row shimmers its label and detail with one band across the
// row, sw's; the icon has none.
func summary(c *ui.Context, call Call, hovered, caret, open bool, sw *kit.Sweep) {
	p := theme.Of(c)
	color := rowColor(p, call)
	quiet := !call.Failed && !call.Pending && !call.Warning
	if hovered && quiet {
		color = p.Foreground
	}
	icon := icons.ByName(call.Icon)
	if call.Pending {
		icon = icons.Help
	} else if icon == nil {
		icon = icons.Settings2
	}
	ui.Icon(c, icon).FontSize(rowIcon).Shrink(0).TextColor(color)
	label := call.Label
	if label == "" && call.LabelKey != "" {
		label = i18n.T(c, call.LabelKey)
	}
	shimmer := call.Running && quiet
	switch {
	case shimmer && label != "":
		sw.Text(c, ui.Text(c, label).FontSize(14).SingleLine().Shrink(0).TextColor(color), ui.Span{Text: label, Size: 14, Color: color})
	case call.wrap:
		ui.Text(c, label).FontSize(14).Shrink(1).MinWidth(0).TextColor(color)
	case label != "":
		ui.Text(c, label).FontSize(14).SingleLine().Shrink(0).TextColor(color)
	}
	detail, code := call.Detail, call.Code
	if (call.Failed || call.Warning) && call.State != Denied && call.Error != "" {
		detail, code = call.Error, false
	}
	if detail != "" {
		dc := p.MutedForeground.Alpha(0.65)
		if !quiet {
			dc = color
		}
		d := ui.Text(c, detail).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0).TextColor(dc)
		span := ui.Span{Text: detail, Size: 14, Color: dc}
		if code {
			d.Font("monospace").FontSize(13).MaxWidth(detailMax)
			span.Font, span.Size = "monospace", 13
		}
		if shimmer {
			sw.Text(c, d, span)
		}
	}
	st := stateText(c, call.State)
	if call.note != "" {
		st = call.note
	}
	if st != "" {
		ui.Text(c, st).FontSize(14).SingleLine().Shrink(0).TextColor(color)
	}
	if caret {
		chevron := icons.ChevronRight
		if open {
			chevron = icons.ChevronDown
		}
		ch := ui.Icon(c, chevron).FontSize(caretSize).Shrink(0).TextColor(color)
		if !hovered {
			ch.Opacity(0)
		}
	}
}

// plain is a row without details: 24 DIPs, no caret.
func plain(c *ui.Context, key any, call Call) {
	row := ui.Row(c.Key(key)).Height(rowHeight).Gap(8).MaxWidthPercent(100)
	hovered := row.Hovered()
	sw := &kit.Sweep{}
	row.Children(func() { summary(c, call, hovered, false, false, sw) })
	sw.Frame(row)
}

// disclosure is a row whose body shows below it on a rail while open.
// Opening is instant, as the reference's (Kit Collapsible without a
// motion_id).
// head builds the row's content; texts it gives the row's sweep shimmer
// as one band across the row.
func (s *State) disclosure(c *ui.Context, key string, head func(hovered, open bool, sw *kit.Sweep), top, bottom float32, body func()) {
	open := s.opened(key)
	ui.Column(c.Key(key)).Children(func() {
		t := ui.ButtonBase(c).Height(rowHeight).Gap(8).AlignSelf(ui.Start).MaxWidthPercent(100).
			Justify(ui.Start).Expanded(*open).Cursor(ui.CursorPointer)
		hovered := t.Hovered() || t.FocusVisible()
		sw := &kit.Sweep{}
		t.Children(func() { head(hovered, *open, sw) }).OnClick(func() { *open = !*open })
		sw.Frame(t)
		if *open {
			Rail(c).Padding(top, 0, bottom, railPad).Children(body)
		}
	})
}

// Rail is the indented body of a disclosure: a 1 DIP line under the icon.
func Rail(c *ui.Context) ui.Element {
	return ui.Column(c).MarginStart(railMargin).BorderWidth(0, 0, 0, 1).BorderColor(theme.Of(c).Border).Gap(8)
}

// tools builds a tool block. continuing marks the last call running while
// the turn goes on, so the shimmer stays on the latest activity.
func (s *State) tools(c *ui.Context, calls []Call, continuing bool) {
	if continuing {
		calls = append([]Call(nil), calls...)
		last := &calls[len(calls)-1]
		if !last.Pending && !last.Failed && !last.Warning {
			last.Running = true
		}
	}
	if Sequence(calls) {
		head := Summary(calls)
		s.disclosure(c, "tool-sequence-"+calls[0].ID, func(hovered, open bool, sw *kit.Sweep) {
			summary(c, head, hovered, true, open, sw)
		}, 8, 8, func() { s.groups(c, calls) })
		return
	}
	s.groups(c, calls)
}

// groups builds runs of calls of the same name, 8 DIPs apart.
func (s *State) groups(c *ui.Context, calls []Call) {
	ui.Column(c).Gap(8).Children(func() {
		for _, run := range Runs(calls) {
			if len(run) == 1 && !pathList(run[0]) {
				s.row(c, run[0])
				continue
			}
			s.group(c, run)
		}
	})
}

// group is calls of one name under the last one's summary.
func (s *State) group(c *ui.Context, run []Call) {
	head := Summary(run)
	if len(run) == 1 && run[0].Output == nil {
		plain(c, "tool-group-"+run[0].ID, head)
		return
	}
	s.disclosure(c, "tool-group-"+run[0].ID, func(hovered, open bool, sw *kit.Sweep) {
		summary(c, head, hovered, true, open, sw)
	}, 8, 8, func() {
		if paths := uniquePaths(run); len(paths) > 0 && pathList(run[len(run)-1]) {
			for _, path := range paths {
				plain(c, path, Call{Label: "", Icon: "file-text", Detail: path, Code: true})
			}
			for _, call := range run {
				if call.Output != nil && call.Output.Failure != "" {
					notice(c, call.Output.Failure, failureTone(c, call.Output))
				}
			}
			return
		}
		for _, call := range run {
			s.row(c, call)
		}
	})
}

// row is one call: its body is its output.
func (s *State) row(c *ui.Context, call Call) {
	if call.Output == nil {
		plain(c, "tools-"+call.ID, call)
		return
	}
	s.disclosure(c, "tools-"+call.ID, func(hovered, open bool, sw *kit.Sweep) {
		summary(c, call, hovered, true, open, sw)
	}, 4, 8, func() { output(c, call.Output) })
}

func failureTone(c *ui.Context, o *Output) ui.Color {
	if o.ToolFailure {
		return theme.Of(c).Warning
	}
	return theme.Of(c).Danger
}

// notice is a line of the output's notes (surface.rs notice style).
func notice(c *ui.Context, text string, tone ui.Color) {
	ui.Text(c, text).FontSize(14).Padding(8, 12).TextColor(tone).Selectable()
}

// output builds a call's result: failure, table, diff, text, notices, with
// a copy button showing while the pointer is over it.
func output(c *ui.Context, o *Output) {
	p := theme.Of(c)
	box := ui.Column(c).Gap(4)
	hovered := box.Hovered()
	box.Children(func() {
		if o.Failure != "" {
			notice(c, o.Failure, failureTone(c, o))
		}
		if o.Table != nil {
			table(c, o.Table)
		}
		if o.Diff != nil {
			diff(c, o.Diff)
		}
		if o.Text != "" {
			text, hidden := o.Text, 0
			if o.Command {
				text, hidden = lastLines(text)
			}
			if hidden > 0 {
				ui.Text(c, i18n.T(c, "tool_output_earlier", "count", hidden)).FontSize(14).
					Padding(0, 12, 8, 12).TextColor(p.MutedForeground)
			}
			ui.ScrollBoth(c).MaxHeight(outputMax).Children(func() {
				ui.Text(c, text).Font("monospace").FontSize(14).LineHeight(20.0 / 14).NoWrap().
					Selectable().TextColor(p.MutedForeground).PaddingEnd(28)
			})
		}
		for _, n := range o.Notices {
			notice(c, n, p.MutedForeground)
		}
		if o.Partial {
			notice(c, i18n.T(c, "tool_partial"), p.MutedForeground)
		}
		copyText := copyOf(o)
		b := kit.IconButton(c.Key("copy"), icons.Copy, i18n.T(c, "tool_copy"), 20, false).
			TextColor(p.MutedForeground).Absolute().Top(4).Right(4)
		if !hovered && !b.FocusVisible() {
			b.Opacity(0)
		}
		b.OnClick(func() { c.WriteClipboard(copyText) })
	})
}

// copyOf is what a call's copy button copies.
func copyOf(o *Output) string {
	var parts []string
	for _, s := range []string{o.Failure, o.Text} {
		if s != "" {
			parts = append(parts, s)
		}
	}
	if o.Diff != nil {
		for _, l := range o.Diff.Lines {
			parts = append(parts, string(o.Diff.Op(l))+l.Text)
		}
	}
	return strings.Join(parts, "\n")
}

// Op returns a diff line's marker.
func (*Diff) Op(l DiffLine) byte {
	if l.Op == 0 {
		return ' '
	}
	return l.Op
}

// diff is a file change preview (surface/diff.rs): an outlined box, its
// header the file name and +N −M, its body up to five rows of the diff.
func diff(c *ui.Context, d *Diff) {
	p := theme.Of(c)
	name := d.Path
	if i := strings.LastIndexByte(name, '/'); i >= 0 {
		name = name[i+1:]
	}
	ui.Column(c).Border(1, p.Border).Radius(p.Radius2xl()).Background(p.Background).Clip().Children(func() {
		ui.Row(c).Height(32).PaddingX(12).Gap(8).BorderWidth(0, 0, 1, 0).BorderColor(p.Border).
			TextColor(p.MutedForeground).Children(func() {
			kit.Tooltip(c, ui.Text(c, name).FontSize(14).SingleLine().Grow(1).Shrink(1), d.Path)
			ui.Row(c).Gap(6).Shrink(0).Children(func() {
				if d.Added > 0 || d.Removed == 0 {
					ui.Textf(c, "+%d", d.Added).Font("monospace").FontSize(12).TextColor(p.Success)
				}
				if d.Removed > 0 {
					ui.Textf(c, "−%d", d.Removed).Font("monospace").FontSize(12).TextColor(p.Danger)
				}
			})
		})
		// MyGo's diff viewer, unified, at most five rows in view.
		rows := min(max(len(d.Lines), 1), 5)
		old, new := d.Texts()
		box := ui.Scroll(c.Key("diff")).MaxHeight(float32(rows) * diffRow)
		kit.Diff(c, box, old, new, kit.DiffOptions{Language: editor.LanguageFor(d.Path), Fit: true, NoNumbers: true})
	})
}

// table is tabular output: 180 DIP columns, at most five rows in view.
func table(c *ui.Context, t *Table) {
	p := theme.Of(c)
	rows := min(max(len(t.Rows), 1), 5)
	ui.ScrollBoth(c).MaxHeight(float32(rows+1) * 28).Children(func() {
		cells := func(values []string, head bool) {
			ui.Row(c).Height(28).BorderWidth(0, 0, 1, 0).BorderColor(p.Border).Children(func() {
				for _, v := range values {
					cell := ui.Text(c, v).FontSize(14).SingleLine().Width(180).PaddingX(8)
					kit.Tooltip(c, cell, v)
					if head {
						cell.FontWeight(500).TextColor(p.MutedForeground)
					}
				}
			})
		}
		cells(t.Columns, true)
		for _, r := range t.Rows {
			cells(r, false)
		}
	})
}

// steps is a task's progress (tools/progress.rs, ui/status_list.rs).
func (s *State) steps(c *ui.Context, key string, steps []Step, active bool) {
	p := theme.Of(c)
	done := 0
	for _, st := range steps {
		if st.State == StepCompleted || st.State == StepSkipped {
			done++
		}
	}
	head := Call{Label: i18n.T(c, "task_progress"), Icon: "circle-check", Detail: fmt.Sprintf("%d/%d", done, len(steps)), Running: active}
	s.disclosure(c, "progress-"+key, func(hovered, open bool, sw *kit.Sweep) { summary(c, head, hovered, true, open, sw) }, 4, 8, func() {
		ui.Scroll(c).MaxHeight(320).PaddingEnd(12).Children(func() {
			for i, st := range steps {
				icon, color := icons.CircleCheck, p.MutedForeground
				switch st.State {
				case StepInProgress:
					icon = icons.Loader
				case StepCompleted:
					color = p.Success
				case StepSkipped:
					icon = icons.Minus
				case StepPaused:
					icon, color = icons.Pause, p.Warning
				case StepBlocked:
					icon, color = icons.TriangleAlert, p.Warning
				case StepFailed:
					icon, color = icons.CircleX, p.Danger
				}
				ui.Row(c.Key(i)).Height(32).Padding(4, 8, 4, 0).Gap(8).Children(func() {
					ui.Icon(c, icon).FontSize(16).Shrink(0).TextColor(color)
					switch {
					case st.State == StepInProgress && active:
						kit.Shimmer(c, st.Text, 14, p.Foreground)
					case st.State == StepCompleted || st.State == StepSkipped:
						ui.Text(c, st.Text).FontSize(14).SingleLine().Strikethrough().TextColor(p.MutedForeground)
					default:
						ui.Text(c, st.Text).FontSize(14).SingleLine().TextColor(p.Foreground)
					}
				})
			}
		})
	})
}

// question is the history row of a question the model asked
// (questions/history.rs): its prompt, the state, and the answers below.
func (s *State) question(c *ui.Context, q *Question) {
	p := theme.Of(c)
	head := Call{Label: q.Prompt, Icon: "circle-check", Pending: q.State == QuestionPending, wrap: true}
	key := ""
	switch q.State {
	case QuestionPending:
		head.Icon, key = "bot", "question_pending"
	case QuestionAccepted:
		key = "question_accepted"
	case QuestionUnconsumed:
		key = "question_unconsumed"
	case QuestionCancelled:
		head.Icon = "circle-x" // interaction_cancelled is not shown
	case QuestionDeclined:
		key = "question_declined"
	case QuestionInterrupted:
		head.Icon, key = "triangle-alert", "question_interrupted"
	case QuestionTimedOut:
		head.Icon, key = "triangle-alert", "chat_response_timeout"
	}
	if key != "" {
		head.note = i18n.T(c, key)
	}
	if len(q.Answers) == 0 {
		plain(c, "question-"+q.CallID, head)
		return
	}
	s.disclosure(c, "question-"+q.CallID, func(hovered, open bool, sw *kit.Sweep) { summary(c, head, hovered, true, open, sw) }, 4, 8, func() {
		ui.Scroll(c).MaxHeight(outputMax).Gap(4).Children(func() {
			for _, a := range q.Answers {
				ui.Text(c, a).FontSize(14).Selectable().TextColor(p.Foreground)
			}
		})
	})
}

// children is the rows of subagent sessions: one row, or a disclosure of
// their marks and count over a row each.
func (s *State) children(c *ui.Context, kids []Child, actions Actions) {
	if len(kids) == 1 {
		child(c, kids[0], actions)
		return
	}
	p := theme.Of(c)
	done, running := 0, false
	for _, k := range kids {
		if k.Status == ChildCompleted {
			done++
		}
		running = running || k.Status == ChildRunning
	}
	label := i18n.T(c, "subagent_count", "completed", done, "total", len(kids))
	s.disclosure(c, "children-"+kids[0].ID, func(hovered, open bool, sw *kit.Sweep) {
		color := p.MutedForeground
		if hovered {
			color = p.Foreground
		}
		ui.Row(c).Gap(4).Shrink(0).Children(func() {
			for _, k := range kids {
				kit.Identicon(c.Key(k.ID), k.ID, k.Status == ChildRunning, 16)
			}
		})
		t := ui.Text(c, label).FontSize(14).SingleLine().TextColor(color)
		if running {
			sw.Text(c, t, ui.Span{Text: label, Size: 14, Color: color})
		}
		ch := ui.Icon(c, map[bool]*ui.SVG{true: icons.ChevronDown, false: icons.ChevronRight}[open]).FontSize(caretSize).TextColor(color)
		if !hovered {
			ch.Opacity(0)
		}
	}, 8, 0, func() {
		ui.Column(c).PaddingStart(16).Gap(8).Children(func() {
			for _, k := range kids {
				child(c, k, actions)
			}
		})
	})
}

// child is a subagent's row; a click opens its session.
func child(c *ui.Context, k Child, actions Actions) {
	p := theme.Of(c)
	title := k.Title
	if title == "" {
		title = i18n.T(c, "chat_child")
	}
	color := p.MutedForeground
	if k.Status == ChildFailed || k.Status == ChildInterrupted {
		color = p.Danger
	}
	b := ui.ButtonBase(c.Key("child-" + k.ID)).Label(title).Height(rowHeight).Gap(8).AlignSelf(ui.Start).Justify(ui.Start).Cursor(ui.CursorPointer)
	hovered := b.Hovered()
	if hovered && color == p.MutedForeground {
		color = p.Foreground
	}
	b.Children(func() {
		kit.Identicon(c, k.ID, k.Status == ChildRunning, 16)
		if k.Status == ChildRunning {
			kit.Shimmer(c, title, 14, color)
		} else {
			ui.Text(c, title).FontSize(14).SingleLine().TextColor(color)
		}
		ui.Text(c, strings.TrimSpace(k.Model+" "+k.Effort)).FontSize(14).SingleLine().TextColor(p.MutedForeground.Alpha(0.65))
		ch := ui.Icon(c, icons.ChevronRight).FontSize(caretSize).TextColor(color)
		if !hovered {
			ch.Opacity(0)
		}
	}).OnClick(func() { actions.OpenChild(k.ID) })
}
