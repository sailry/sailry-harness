package conversation

import (
	"cmp"
	"context"
	"fmt"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Layout metrics.
const (
	detailMax = 480      // mono details
	diffRow   = 13 * 1.6 // Kit's code line, as the diff viewer's
)

// textSize is the size of the transcript's content text in DIPs: the
// message text size setting (kit.MessageTextSize, 14 unless set), which the
// message bodies take too. Every text of a turn derives from it and keeps
// what it was apart from 14: a mono detail is a DIP smaller, counts and
// captions two. Controls (buttons, menus, dialogs, tooltips) keep Kit's
// sizes.
func textSize(c *ui.Context) float32 { return kit.MessageTextSize(c) }

// A row's metrics for text of size, the reference's at 14: 24 DIPs high,
// its icon 16 and its caret 12.
func rowHeight(size float32) float32 { return size + 10 }
func rowIcon(size float32) float32   { return size + 2 }
func caretSize(size float32) float32 { return size - 2 }

// outputMax is five lines of a call's output, 20 DIPs each at 14.
func outputMax(size float32) float32 { return 100 * size / 14 }

// railMargin puts the rail's 1 DIP line under the icon's middle, and
// railPad starts the body where the label does, past the icon and the
// row's gap of 8: 7.5 and 15.5 at 14.
func railMargin(size float32) float32 { return rowIcon(size)/2 - 0.5 }
func railPad(size float32) float32    { return rowIcon(size)/2 + 7.5 }

// opened reports whether a disclosure is open. They start closed, and the
// state keeps the open ones alone, so the rows a long transcript scrolls
// through leave nothing behind.
func (s *State) opened(key string) bool { return s.open[key] }

// toggle opens a closed disclosure and closes an open one.
func (s *State) toggle(key string) {
	if s.open[key] {
		delete(s.open, key)
	} else {
		s.open[key] = true
	}
}

// rowColor is the color of a row's icon and label, first match winning.
func rowColor(p *theme.Palette, call Call) ui.Color {
	switch {
	case call.Failed:
		return p.Danger
	case call.Pending || flagged(call):
		return p.Warning
	case call.Running:
		return p.MutedForeground.Mix(p.Foreground, 0.35)
	}
	return p.MutedForeground
}

// flagged gives denied, interrupted and unexecuted calls and background
// execution faults the warning color. A command exit uses the plain row.
func flagged(call Call) bool {
	return call.Warning && call.State != StateNone || backgroundFailed(call.Background)
}

// backgroundFailed reports a fault, timeout, interruption or uncertain
// outcome of a background command. An exit code records a process result.
func backgroundFailed(b *Background) bool {
	if b == nil || b.Running || b.Exit != nil {
		return false
	}
	switch b.Outcome {
	case OutcomeFailed, OutcomeTimedOut, OutcomeInterrupted, OutcomeUnknown:
		return true
	}
	return false
}

// summary builds a row's content: status icon, label, input detail and caret.
// A running row, or one awaiting the model, shimmers its label and detail
// with one band across the row, sw's; the icon has none.
func summary(c *ui.Context, call Call, hovered, caret, open bool, sw *kit.Sweep) {
	p := theme.Of(c)
	size := textSize(c)
	color := rowColor(p, call)
	quiet := !call.Failed && !call.Pending && !flagged(call)
	if hovered && quiet {
		color = p.Foreground
	}
	icon := icons.ByName(call.Icon)
	if call.Pending {
		icon = icons.Help
	} else if icon == nil {
		icon = icons.Settings2
	}
	ic := ui.Icon(c, icon).FontSize(rowIcon(size)).Shrink(0).TextColor(color)
	if call.wrap {
		ic.Margin((rowHeight(size)-rowIcon(size))/2, 0, 0, 0)
	}
	label := call.Label
	if label == "" && call.LabelKey != "" {
		label = i18n.T(c, call.LabelKey)
	}
	shimmer := call.Running && quiet || awaitingModel(call)
	switch {
	case label != "" && (shimmer || !call.wrap):
		// Laid out as the sweep draws it (kit.Sweep.Text), running or not,
		// so the label stays where it is when the row settles.
		span := ui.Span{Text: label, Size: size, Color: color}
		t := levelSpan(c, ui.RichText(c, span).SingleLine().Shrink(0), span, size, call.wrap)
		if shimmer {
			sw.Text(c, t, span)
		}
	case call.wrap:
		level(c, ui.Text(c, label).FontSize(size).Grow(1).Shrink(1).MinWidth(0).TextColor(color), size, true, label)
	}
	detail, code := call.Detail, call.Code
	if detail != "" {
		dc := p.MutedForeground.Alpha(0.65)
		if !quiet {
			dc = color
		}
		span := ui.Span{Text: detail, Size: size, Color: dc}
		if code {
			span.Font, span.Size = "monospace", size-1
		}
		// Laid out as the sweep draws it, on the label's baseline.
		d := levelSpan(c, ui.RichText(c, span).SingleLine().Grow(1).Shrink(1).MinWidth(0), span, size, call.wrap)
		if code {
			d.MaxWidth(detailMax)
		}
		if shimmer {
			sw.Text(c, d, span)
		}
	}
	if caret {
		chevron := icons.ChevronRight
		if open {
			chevron = icons.ChevronDown
		}
		ch := ui.Icon(c, chevron).FontSize(caretSize(size)).Shrink(0).TextColor(color)
		if call.wrap {
			ch.Margin((rowHeight(size)-caretSize(size))/2, 0, 0, 0)
		}
		if !hovered {
			ch.Opacity(0)
		}
	}
}

// level puts a row's text of size level with its icon (kit.Level): centred
// in the row, or on the first line of a row that wraps.
func level(c *ui.Context, e ui.Element, size float32, wrap bool, label string) ui.Element {
	if wrap {
		return kit.LevelTop(c, e, size, rowHeight(size), label)
	}
	return kit.Level(c, e, size, label)
}

// levelSpan is level for a single-line ui.RichText of span in a row whose
// text is of size (kit.LevelSpan): what a sweep crosses, and a row's
// detail.
func levelSpan(c *ui.Context, e ui.Element, span ui.Span, size float32, wrap bool) ui.Element {
	if wrap {
		return kit.LevelSpanTop(c, e, span, size, rowHeight(size))
	}
	return kit.LevelSpan(c, e, span, size)
}

// rowSize gives a row the summary's height for text of size: 24 DIPs at
// 14, or, for a label that wraps, the full width, at least that tall, with
// the icon and caret where a row of one line has them, level with
// the first line (kit.LevelTop).
func rowSize(e ui.Element, size float32, wrap bool) ui.Element {
	if wrap {
		return e.FillWidth().MinHeight(rowHeight(size)).AlignItems(ui.Start)
	}
	return e.Height(rowHeight(size))
}

// plain is a row without details: no caret.
func plain(c *ui.Context, key any, call Call) {
	row := rowSize(ui.Row(c.Key(key)), textSize(c), call.wrap).Gap(8).MaxWidthPercent(100)
	if !call.wrap {
		row.AlignSelf(ui.Start)
	}
	hovered := row.Hovered()
	sw := &kit.Sweep{}
	row.Children(func() { summary(c, call, hovered, false, false, sw) })
	sw.Frame(row)
}

// pathRow is a file a call touched, as a plain row: it opens the file as a
// link of the content does (State.follow) when the application opens files.
func (s *State) pathRow(c *ui.Context, path string) {
	call := Call{Label: "", Icon: "file-text", Detail: path, Code: true}
	if s.OpenLink == nil {
		plain(c, path, call)
		return
	}
	session := s.renderSession
	row := rowSize(ui.ButtonBase(c.Key(path)).Label(path), textSize(c), false).Gap(8).AlignSelf(ui.Start).MaxWidthPercent(100).
		Justify(ui.Start).Cursor(ui.CursorPointer)
	hovered := row.Hovered() || row.FocusVisible()
	sw := &kit.Sweep{}
	row.Children(func() { summary(c, call, hovered, false, false, sw) }).OnClick(func() { s.follow(c, session, pathLink(path)) })
	sw.Frame(row)
}

// disclosure is a row whose body shows below it on a rail while open.
// Opening is instant, as the reference's (Kit Collapsible without a
// motion_id).
// head builds the row's content; texts it gives the row's sweep shimmer
// as one band across the row.
func (s *State) disclosure(c *ui.Context, key string, head func(hovered, open bool, sw *kit.Sweep), top, bottom float32, body func()) {
	s.wrapping(c, key, false, false, head, top, bottom, func(ui.Element) { body() })
}

// wrapping is a disclosure whose row may wrap its label (rowSize).
func (s *State) wrapping(c *ui.Context, key string, wrap, defaultOpen bool, head func(hovered, open bool, sw *kit.Sweep), top, bottom float32, body func(ui.Element)) {
	open, chosen := s.open[key]
	if !chosen {
		open = defaultOpen
	}
	size := textSize(c)
	ui.Column(c.Key(key)).Children(func() {
		t := rowSize(ui.ButtonBase(c), size, wrap).Gap(8).AlignSelf(ui.Start).MaxWidthPercent(100).
			Justify(ui.Start).Expanded(open).Cursor(ui.CursorPointer)
		hovered := t.Hovered() || t.FocusVisible()
		sw := &kit.Sweep{}
		t.Children(func() { head(hovered, open, sw) }).OnClick(func() {
			if open == defaultOpen {
				s.open[key] = !open
			} else {
				delete(s.open, key)
			}
		})
		sw.Frame(t)
		if open {
			rail := Rail(c).Padding(top, 0, bottom, railPad(size))
			rail.Children(func() { body(rail) })
		}
	})
}

// railGap is between the rows on a rail.
const railGap = 8

// Rail is the indented body of a disclosure: a 1 DIP line under the icon.
func Rail(c *ui.Context) ui.Element {
	return ui.Column(c).MarginStart(railMargin(textSize(c))).BorderWidth(0, 0, 0, 1).BorderColor(theme.Of(c).Border).Gap(railGap)
}

func (s *State) tools(c *ui.Context, calls []Call, activity bool) {
	if !activity && slices.ContainsFunc(calls, func(call Call) bool { return call.Running || call.AwaitingModel }) {
		calls = slices.Clone(calls)
		for index := range calls {
			calls[index].Running, calls[index].AwaitingModel = false, false
		}
	}
	ui.Column(c).Gap(railGap).Children(func() {
		for first := 0; first < len(calls); {
			end := first + 1
			if groupable(calls[first]) {
				for end < len(calls) && groupable(calls[end]) {
					end++
				}
			}
			if end-first == 1 {
				s.row(c, calls[first], activity)
			} else {
				s.toolGroup(c, calls[first:end], activity)
			}
			first = end
		}
	})
}

// toolRowEnter brings a call that joins an open group in below the rows
// already there. MyGo shows it in place while the system asks for less motion.
// Only the entry moves: selecting Colors alone keeps later layout changes,
// as a row above opening, instant like the disclosures themselves.
var toolRowEnter = ui.ElementTransition{Duration: 180 * time.Millisecond, Ease: kit.EaseOut, Colors: true, Enter: &ui.Motion{Collapse: true}}

// A group discloses call headers. Individual rows still own their outputs.
// Every call of a run joins it whatever its state, so a call that starts,
// settles or fails stays in its place. The group is keyed by its first
// call, which a growing run keeps. While the run is active a failure does
// not open the group, so the transcript stays still as calls join it; the
// finished turn reveals the failure.
func (s *State) toolGroup(c *ui.Context, calls []Call, activity bool) {
	head := groupHead(calls)
	failed := !activity && slices.ContainsFunc(calls, func(call Call) bool {
		return (call.Failed || call.Warning) && toolErrorOpen(call) &&
			(call.Load != nil || call.Output != nil && call.Output.Failure != "")
	})
	s.wrapping(c, "tool-group-"+calls[0].ID, false, failed, func(hovered, open bool, sw *kit.Sweep) {
		summary(c, head, hovered, true, open, sw)
	}, 8, 8, func(ui.Element) {
		for _, call := range calls {
			ui.Column(c.Key("enter-" + call.ID)).Transition(toolRowEnter).Children(func() { s.row(c, call, activity) })
		}
	})
}

// groupHead is a group's header: its latest call, shown and coloured as
// that call's own row, so the run reads as the work it is doing now.
func groupHead(calls []Call) Call {
	head := calls[len(calls)-1]
	head.wrap = false
	// The header sweeps while any call in the group still runs; its label,
	// icon and colour stay the latest call's.
	head.Running = head.Running || slices.ContainsFunc(calls, func(call Call) bool { return call.Running })
	return head
}

func toolErrorOpen(call Call) bool {
	return !call.Pending && (call.Failed || call.Warning && call.State == StateNone || call.Output != nil && call.Output.Failure != "")
}

// row is one call: its body is its output. Like a group, a failed row opens
// on its own only once its run is no longer active.
func (s *State) row(c *ui.Context, call Call, activity bool) {
	if call.Name == "generate_image" && call.Output != nil && len(call.Output.Images) > 0 {
		// Generated pictures belong in the answer and output inventory. Keep the
		// original references for reply embeds without repeating the preview here.
		output := *call.Output
		output.Images = nil
		call.Output = &output
	}
	// A call whose loaded output has nothing to draw does not open: its
	// row already says all there is.
	card := s.toolCards[s.renderSession][call.ID]
	if call.Background == nil && call.BackgroundDetail == nil && (call.Output == nil || call.Load == nil && card == nil && bare(call) && !pathList(call)) {
		plain(c, "tools-"+call.ID, call)
		return
	}
	s.wrapping(c, "tools-"+call.ID, false, !activity && toolErrorOpen(call), func(hovered, open bool, sw *kit.Sweep) {
		summary(c, call, hovered, true, open, sw)
	}, 4, 8, func(rail ui.Element) {
		if call.Load != nil {
			state := ui.Local(rail, "tool-detail", func() disclosureDetail { return disclosureDetail{} })
			if output := state.read(call); output != nil {
				value := *output
				if call.Output != nil {
					value.Images = call.Output.Images
				}
				call.Output = &value
			}
		}
		var detail BackgroundDetail
		if call.BackgroundDetail != nil {
			detail = call.BackgroundDetail()
		}
		if detail.Output != nil {
			s.output(c, detail.Output, false)
		} else if b := call.Background; b != nil && b.Preview != "" {
			if call.Output == nil || call.Output.Text != b.Preview && call.Output.Failure != b.Preview {
				s.output(c, &Output{Text: b.Preview, Command: true}, false)
			}
		}
		paths := pathList(call)
		if call.Output != nil && (paths || !bare(call)) {
			s.output(c, call.Output, paths)
		}
		if card != nil {
			card(c)
		}
	})
}

// The open rail owns the formatted result. MyGo closes its local state when
// the disclosure or virtualized row leaves the tree.
type disclosureDetail struct {
	revision string
	active   *bool
	output   *Output
	cancel   func()
}

func (d *disclosureDetail) read(call Call) *Output {
	if d.active == nil || d.revision != call.DetailRevision {
		d.Close()
		active := true
		d.active, d.revision = &active, call.DetailRevision
		d.cancel = call.Load(func(output *Output) {
			if active {
				d.output = output
			}
		})
	}
	return d.output
}

func (d *disclosureDetail) Close() error {
	if d.active != nil {
		*d.active = false
	}
	if d.cancel != nil {
		d.cancel()
	}
	*d = disclosureDetail{}
	return nil
}

// bare reports an output with nothing to display below its row.
func bare(call Call) bool {
	o := call.Output
	if o.Table != nil || o.Diff != nil || o.Text != "" || o.Copy != nil || len(o.Images) > 0 || len(o.Notices) > 0 {
		return false
	}
	return o.Failure == ""
}

func failureTone(c *ui.Context, o *Output) ui.Color {
	if o.ToolFailure {
		return theme.Of(c).Warning
	}
	return theme.Of(c).Danger
}

// notice is a line of the output's notes.
func notice(c *ui.Context, text string, tone ui.Color) {
	ui.Text(c, text).FontSize(textSize(c)).Padding(8, 12).TextColor(tone).Selectable()
}

// output builds a call's result. A path list uses the file links in place
// of the aggregate preview, while retaining other parts of the result.
func (s *State) output(c *ui.Context, o *Output, paths bool) {
	p := theme.Of(c)
	size := textSize(c)
	box := ui.Column(c).Gap(4)
	hovered := box.Hovered()
	box.Children(func() {
		if paths {
			for _, path := range uniquePaths(o.Paths) {
				s.pathRow(c, path)
			}
		}
		if o.Failure != "" {
			notice(c, o.Failure, failureTone(c, o))
		}
		if o.Table != nil {
			table(c, o.Table)
		}
		if o.Diff != nil {
			s.diff(c, o.Diff)
		}
		if !paths && o.Text != "" {
			text := o.Text
			if o.Command {
				text, _ = lastLines(text)
			}
			ui.ScrollBoth(c).MaxHeight(outputMax(size)).Children(func() {
				ui.Text(c, text).Font("monospace").FontSize(size).LineHeight(20.0 / 14).NoWrap().
					Selectable().TextColor(p.MutedForeground).PaddingEnd(28)
			})
		}
		if len(o.Images) > 0 {
			s.thumbnails(c, "output-images", o.Images, false)
		}
		for _, n := range o.Notices {
			notice(c, n, p.MutedForeground)
		}
		if paths {
			return
		}
		b := kit.IconButton(c.Key("copy"), icons.Copy, i18n.T(c, "tool_copy"), 20, false).
			TextColor(p.MutedForeground).Absolute().Top(4).Right(4)
		if !hovered && !b.FocusVisible() {
			b.Opacity(0)
		}
		b.OnClick(func() {
			if o.Copy != nil {
				s.cancelCopy()
				ctx, cancel := context.WithCancel(context.Background())
				s.copyCancel = cancel
				o.Copy(ctx)
				return
			}
			s.copyText(c, copyOf(o))
		})
	})
}

func (s *State) cancelCopy() {
	if s.copyCancel != nil {
		s.copyCancel()
		s.copyCancel = nil
	}
}

func (s *State) copyText(c *ui.Context, text string) {
	s.cancelCopy()
	c.WriteClipboard(text)
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

// diff is a file change preview: an outlined box, its header the file name
// and +N −M, its body up to five rows of the diff in Kit's code size. The
// name opens the file as a link of the content does (State.follow) when the
// application opens files.
func (s *State) diff(c *ui.Context, d *Diff) {
	p := theme.Of(c)
	size := textSize(c)
	name := d.Path
	if i := strings.LastIndexByte(name, '/'); i >= 0 {
		name = name[i+1:]
	}
	ui.Column(c).Border(1, p.Border).Radius(p.Radius2xl()).Background(p.Background).Clip().Children(func() {
		ui.Row(c).Height(size+18).PaddingX(12).Gap(8).BorderWidth(0, 0, 1, 0).BorderColor(p.Border).
			TextColor(p.MutedForeground).Children(func() {
			label := ui.Text(c, name).FontSize(size).SingleLine().Shrink(1)
			if s.OpenLink != nil && d.Path != "" {
				session := s.renderSession
				label.Cursor(ui.CursorPointer).OnClick(func() { s.follow(c, session, pathLink(d.Path)) })
			}
			kit.Tooltip(c, label, d.Path)
			ui.Spacer(c)
			ui.Row(c).Gap(6).Shrink(0).Children(func() {
				if d.Added > 0 || d.Removed == 0 {
					ui.Textf(c, "+%d", d.Added).Font("monospace").FontSize(size - 2).TextColor(p.Success)
				}
				if d.Removed > 0 {
					ui.Textf(c, "−%d", d.Removed).Font("monospace").FontSize(size - 2).TextColor(p.Danger)
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

// table is tabular output: 180 DIP columns, at most five rows in view,
// each 28 DIPs at 14.
func table(c *ui.Context, t *Table) {
	p := theme.Of(c)
	size := textSize(c)
	height := size + 14
	rows := min(max(len(t.Rows), 1), 5)
	ui.ScrollBoth(c).MaxHeight(float32(rows+1) * height).Children(func() {
		cells := func(values []string, head bool) {
			ui.Row(c).Height(height).BorderWidth(0, 0, 1, 0).BorderColor(p.Border).Children(func() {
				for _, v := range values {
					cell := ui.Text(c, v).FontSize(size).SingleLine().Width(180).PaddingX(8)
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

// steps is a task's progress.
func (s *State) steps(c *ui.Context, key string, steps []Step, active bool) {
	done := 0
	for _, st := range steps {
		if st.State == StepCompleted || st.State == StepSkipped {
			done++
		}
	}
	head := Call{Label: i18n.T(c, "task_progress"), Icon: "circle-check", Detail: fmt.Sprintf("%d/%d", done, len(steps)), Running: active}
	// The plan is what the turn works through: it stays open, with no fold,
	// under its count.
	ui.Column(c.Key("progress-" + key)).Children(func() {
		plain(c, "head", head)
		Rail(c).Padding(4, 0, 8, railPad(textSize(c))).Children(func() {
			ui.Scroll(c).MaxHeight(320).PaddingEnd(12).Children(func() { stepRows(c, steps, active, textSize(c), false) })
		})
	})
}

// Steps are a task's progress rows: its state's icon and the step, settled
// ones struck through, the step in progress shimmering while active. The
// composer's task capsule shows them, in Kit's 14; the transcript shows
// them in its text size (stepRows).
func Steps(c *ui.Context, steps []Step, active bool) { stepRows(c, steps, active, 14, false) }

// QuietSteps are Steps for a summary that lists a whole plan at a glance
// (the session overview): a step to a line of 32 DIPs, cut short with its
// whole text as the tooltip, the settled ones muted but not struck through.
func QuietSteps(c *ui.Context, steps []Step, active bool) { stepRows(c, steps, active, 14, true) }

// stepRows are Steps for text of size: each row 32 DIPs at 14 for a step
// of one line, a long step wrapping whole under its first line's icon, or
// cut to its one line in a summary.
func stepRows(c *ui.Context, steps []Step, active bool, size float32, summary bool) {
	p := theme.Of(c)
	for i, st := range steps {
		// A pending step is an empty circle, so only a settled one shows a
		// check; the current step turns while the run does and is a marked
		// circle otherwise, never a spinner that stands still.
		icon, color := icons.Circle, p.MutedForeground
		switch st.State {
		case StepInProgress:
			icon, color = icons.CircleDot, p.Primary
		case StepCompleted:
			icon, color = icons.CircleCheck, p.Success
		case StepSkipped:
			icon = icons.Minus
		case StepPaused:
			icon, color = icons.Pause, p.Warning
		case StepBlocked:
			icon, color = icons.TriangleAlert, p.Warning
		case StepFailed:
			icon, color = icons.CircleX, p.Danger
		}
		row := ui.Row(c.Key(i)).Gap(8)
		if summary {
			row.FillWidth().Height(32).PaddingEnd(8).AlignItems(ui.Center)
		} else {
			rowSize(row, size, true).Padding(4, 8, 4, 0)
		}
		row.Children(func() {
			// The mark is level with the first line.
			mark := ui.Box(c.Key("mark")).Shrink(0).Center()
			if !summary {
				mark.Height(rowHeight(size))
			}
			mark.Children(func() {
				if st.State == StepInProgress && active {
					kit.BusySized(c, rowIcon(size)).Label(i18n.T(c, "chat_running"))
				} else {
					ui.Icon(c, icon).FontSize(rowIcon(size)).TextColor(color)
				}
			})
			text := ui.Text(c, st.Text).FontSize(size).Grow(1).Shrink(1).MinWidth(0).TextColor(p.Foreground)
			if st.State == StepCompleted || st.State == StepSkipped {
				text.TextColor(p.MutedForeground)
				if !summary {
					text.Strikethrough()
				}
			}
			if summary {
				level(c, text.SingleLine().Tooltip(st.Text), size, false, st.Text)
				return
			}
			level(c, text, size, true, st.Text)
		})
	}
}

// question is the history row of a question the model asked: its prompt, the
// state, and the answers below.
func (s *State) question(c *ui.Context, q *Question) {
	p := theme.Of(c)
	size := textSize(c)
	// The state shows as the icon alone, without a text.
	head := Call{Label: q.Prompt, Icon: "circle-check", Pending: q.State == QuestionPending, wrap: true}
	switch q.State {
	case QuestionPending:
		head.Icon = "bot"
	case QuestionCancelled, QuestionDeclined:
		head.Icon = "circle-x"
	case QuestionInterrupted, QuestionTimedOut:
		head.Icon = "triangle-alert"
	}
	if len(q.Answers) == 0 {
		plain(c, "question-"+q.CallID, head)
		return
	}
	s.wrapping(c, "question-"+q.CallID, true, false, func(hovered, open bool, sw *kit.Sweep) { summary(c, head, hovered, true, open, sw) }, 4, 8, func(ui.Element) {
		ui.Scroll(c).MaxHeight(outputMax(size)).Gap(4).Children(func() {
			for _, a := range q.Answers {
				ui.Text(c, a).FontSize(size).Selectable().TextColor(p.Foreground)
			}
		})
	})
}

// children is the rows of subagent sessions: one row, or a disclosure of
// their marks and count over a row each.
// childCounts counts every ended child as done, whatever its outcome. A
// continued row is superseded by the turn that follows it and is not counted.
func childCounts(kids []Child) (done, total int) {
	for _, k := range kids {
		if k.Status == ChildContinued {
			continue
		}
		total++
		if k.Status.Finished() {
			done++
		}
	}
	return done, total
}

func (s *State) children(c *ui.Context, kids []Child, actions Actions) {
	if len(kids) == 1 {
		child(c, kids[0], actions)
		return
	}
	p := theme.Of(c)
	size := textSize(c)
	done, total := childCounts(kids)
	running := slices.ContainsFunc(kids, func(k Child) bool { return !k.Unsynced && k.Status.Active() })
	label := i18n.T(c, "subagent_count", "completed", done, "total", total)
	s.disclosure(c, "children-"+cmp.Or(kids[0].Turn, kids[0].ID), func(hovered, open bool, sw *kit.Sweep) {
		color := p.MutedForeground
		if hovered {
			color = p.Foreground
		}
		ui.Row(c).Gap(4).Shrink(0).Children(func() {
			for _, k := range kids {
				kit.RoleIdenticon(c.Key(cmp.Or(k.Turn, k.ID)), k.ID, k.Avatar, !k.Unsynced && k.Status.Active(), rowIcon(size))
			}
		})
		// Laid out as the sweep draws it, running or not (summary).
		span := ui.Span{Text: label, Size: size, Color: color}
		t := levelSpan(c, ui.RichText(c, span).SingleLine(), span, size, false)
		if running {
			sw.Text(c, t, span)
		}
		ch := ui.Icon(c, map[bool]*ui.SVG{true: icons.ChevronDown, false: icons.ChevronRight}[open]).FontSize(caretSize(size)).TextColor(color)
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
	size := textSize(c)
	title := k.Title
	if title == "" {
		title = i18n.T(c, "chat_child")
	}
	color := p.MutedForeground
	if k.Status == ChildFailed || k.Status == ChildInterrupted {
		color = p.Danger
	}
	b := ui.ButtonBase(c.Key("child-" + cmp.Or(k.Turn, k.ID))).Label(title).Height(rowHeight(size)).Gap(8).AlignSelf(ui.Start).Justify(ui.Start).Cursor(ui.CursorPointer)
	hovered := b.Hovered()
	if hovered && color == p.MutedForeground {
		color = p.Foreground
	}
	b.Children(func() {
		if k.Status == ChildContinued {
			ui.Icon(c, icons.ArrowRight).FontSize(rowIcon(size)).TextColor(color)
		} else {
			kit.RoleIdenticon(c, k.ID, k.Avatar, !k.Unsynced && k.Status.Active(), rowIcon(size))
		}
		if !k.Unsynced && k.Status.Active() {
			levelSpan(c, kit.Shimmer(c, title, size, color), ui.Span{Text: title, Size: size}, size, false)
		} else {
			level(c, ui.Text(c, title).FontSize(size).SingleLine().TextColor(color), size, false, title)
		}
		model := strings.TrimSpace(k.Model + " " + k.Effort)
		level(c, ui.Text(c, model).FontSize(size).SingleLine().TextColor(p.MutedForeground.Alpha(0.65)), size, false, model)
		ch := ui.Icon(c, icons.ChevronRight).FontSize(caretSize(size)).TextColor(color)
		if !hovered {
			ch.Opacity(0)
		}
	}).OnClick(func() { actions.OpenChild(k.ID) })
	keys := [...]string{ChildRunning: "chat_running", ChildCompleted: "turn_completed", ChildFailed: "turn_failed",
		ChildInterrupted: "chat_interrupted", ChildCancelled: "turn_cancelled", ChildQueued: "turn_queued",
		ChildStopping: "chat_stopping", ChildContinued: "turn_continued"}
	key := keys[k.Status]
	if k.Unsynced && k.Status.Active() {
		key = "chat_status_unsynced"
	}
	status := i18n.T(c, key)
	if k.Error != "" && (k.Status == ChildFailed || k.Status == ChildInterrupted) {
		status += "\n" + k.Error
	}
	kit.Tooltip(c, b, status)
}
