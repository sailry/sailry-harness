package composer

import (
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The queue strip over the input surface: the messages sent while a reply
// runs, each with Edit, Send now and Remove, dragged by its handle to another
// place; Edit opens the message in an editor in the strip with Cancel and
// Save. A message whose guidance is pending shows the loader in the handle's
// place and Withdraw, with Edit and Remove disabled, and Send now too unless
// the guidance is aimed at the turn taking guidance now. A message still on
// its way to the queue is a last row whose actions are all disabled. Every
// row starts with the same slot (queueLead), so the labels share one edge. A child
// session's strip is read-only: Read opens the text with Copy, Close and
// Remove, and nothing is sent ahead or dragged (live/queue/render.rs). Rows
// come in and go out collapsing, as the strip does. The strip's first row
// counts the messages and folds the rest away; it is folded until the user
// unfolds it, for each conversation's composer. The strip lists queued
// input only: a row says nothing about why it waits, and background results
// held back from delivery are not rows (docs/removed-ui.md).

// queueState is the strip's: the message edited and its draft.
type queueState struct {
	generation      uint64
	revision        uint64
	loading, saving bool
	editing         string
	text            string
	original        string
}

// queueMotion is how rows and the strip come and go: Kit's normal
// duration, collapsing and fading.
var queueMotion = ui.ElementTransition{
	Duration: 180 * time.Millisecond,
	Ease:     ui.CubicBezier(0.2, 0, 0, 1),
	Enter:    &ui.Motion{Collapse: true, Opacity: 0},
	Exit:     &ui.Motion{Collapse: true, Opacity: 0},
}

// queueLabel is a row's text: a continuation's marker, a notice's title,
// the compaction's, the preview, or the names of its files.
func queueLabel(c *ui.Context, q Queued) string {
	switch {
	case q.Marker != nil:
		if label := q.Marker.in(c); label != "" {
			return label
		}
		return i18n.T(c, "turn_continued")
	case q.Notice:
		return i18n.T(c, "background_notice_title")
	case q.Compaction:
		return i18n.T(c, "chat_compact")
	case q.Text == "" && len(q.Attachments) > 0:
		return i18n.T(c, "chat_attachment_count", "count", len(q.Attachments))
	}
	return q.Text
}

// queueTray is 12 in from the surface's sides, padding 4, the top corners
// rounded 2xl, in the secondary color: the header naming how many messages
// wait, which folds the tray to that one row and unfolds it again, then a
// hairline and the rows, scrolling past 132 DIPs, or the editor of one of
// them. The tray is folded until the user unfolds it (State.queueOpen), and
// unfolded whatever that says while a message is edited, so an editor and
// its draft are never hidden.
func (s *State) queueTray(c *ui.Context, ctx *Context, actions Actions) {
	q := &s.queue
	if q.editing != "" && !queued(ctx, q.editing) {
		q.editing = "" // sent or removed meanwhile
	}
	ui.Column(c.Key("queue-slot")).FillWidth().Children(func() {
		if len(ctx.Queue) == 0 && q.editing == "" && ctx.Outgoing == nil {
			return
		}
		p := theme.Of(c)
		r := p.Radius2xl()
		// The tray tucks behind the composer; a child session has none, so
		// its tray is closed below too.
		below := float32(0)
		if ctx.Child {
			below = r
		}
		open := s.queueOpen || q.editing != ""
		kit.Frost(c, ui.Column(c.Key("queue")).MarginX(12).Padding(4).Radius(r, r, below, below), p.Secondary, kit.SurfaceTint).
			Label(i18n.T(c, "queue_count", "count", len(ctx.Queue))).Transition(queueMotion).Children(func() {
			s.queueHeader(c, ctx, open)
			if !open {
				return
			}
			// The hairline 4 under the header, then the rows 4 under it,
			// coming and going as the rows do.
			ui.Column(c.Key("queue-body")).FillWidth().Shrink(0).Padding(4, 0, 0, 0).Gap(4).Transition(queueMotion).Children(func() {
				ui.Divider(c.Key("queue-line")).Background(p.Border)
				if q.editing != "" {
					s.queueEditor(c, ctx, actions)
					return
				}
				s.queueList(c, ctx, actions)
			})
		})
	})
}

// queueHeader is the tray's first row, as high as a message's and quieter:
// a chevron in the rows' leading slot, pointing right while the tray is
// folded and down while it is unfolded, then the number of messages waiting,
// both muted. The whole row is the disclosure, as the transcript's: a click,
// Enter or Space folds and unfolds the rows. It takes no fold while a
// message is edited.
func (s *State) queueHeader(c *ui.Context, ctx *Context, open bool) {
	p := theme.Of(c)
	n := len(ctx.Queue)
	if ctx.Outgoing != nil {
		n++ // on its way to the queue, and a row already
	}
	label := count(c, "queue_messages_one", "queue_messages_count", n)
	editing := s.queue.editing != ""
	t := ui.ButtonBase(c.Key("queue-header")).Label(label).Expanded(open).Disabled(editing).FillWidth().MinWidth(0).Height(32).
		PaddingX(8).Gap(8).Radius(p.Radius2xl()).Shrink(0).Justify(ui.Start).AlignItems(ui.Center).TextColor(p.MutedForeground)
	if !editing {
		t.Cursor(ui.CursorPointer)
		if t.Hovered() || t.FocusVisible() {
			t.Background(p.Hover)
		}
	}
	chevron := icons.ChevronRight
	if open {
		chevron = icons.ChevronDown
	}
	t.Children(func() {
		queueLead(c, false, chevron)
		ui.Text(c, label).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0).TextColor(p.MutedForeground)
	}).OnClick(func() { s.queueOpen = !s.queueOpen })
}

// queueList is the tray's rows, scrolling past 132 DIPs.
func (s *State) queueList(c *ui.Context, ctx *Context, actions Actions) {
	ui.Scroll(c.Key("queue-list")).FillWidth().MaxHeight(132).Gap(4).Children(func() {
		if ctx.Child {
			// Nothing is reordered from a child's strip.
			for _, item := range ctx.Queue {
				s.queueRow(c, ctx, actions, item, func() {})
			}
		} else if len(ctx.Queue) > 0 {
			// Dragged by the handle into another place: the move goes
			// to the conversation, before the message now after it.
			list := slices.Clone(ctx.Queue)
			kit.SortableList(c, &list, func(q Queued) string { return q.ID }, kit.SortableListProps{
				Gap: 4, Handle: i18n.T(c, "queue_drag"), Disabled: queueBlocked(ctx),
				OnMove: func(_, to int) { actions.MoveQueued(list[to].ID, after(list, list[to].ID)) },
			}, func(item Queued, handle func()) { s.queueRow(c, ctx, actions, item, handle) })
		}
		if ctx.Outgoing != nil {
			s.outgoingRow(c, *ctx.Outgoing)
		}
	})
}

// promotable is the first message Enter in an empty draft sends: not a notice
// or a compaction, and not already guiding; "" for none, or while read-only.
func promotable(ctx *Context) string {
	if ctx.Readonly {
		return ""
	}
	for _, q := range ctx.Queue {
		if !q.Notice && !q.Compaction && !q.Guided {
			return q.ID
		}
	}
	return ""
}

func queued(ctx *Context, id string) bool {
	for _, q := range ctx.Queue {
		if q.ID == id {
			return true
		}
	}
	return false
}

// queueBlocked reports whether the strip's actions are held: the composer is
// read-only, or a child session's Node is away.
func queueBlocked(ctx *Context) bool {
	if ctx.Child {
		return !ctx.Activity.Connected
	}
	return ctx.Readonly
}

// queueSmall is a row's action: a small round ghost icon button.
func queueSmall(c *ui.Context, key string, icon *ui.SVG, tip string, disabled bool, fn func()) {
	kit.IconButton(c.Key(key), icon, tip, 24, false).Radius(12).Disabled(disabled).OnClick(fn)
}

// queueLead is a row's leading slot, 24 square as the drag handle is: the
// loader while the row is being taken, or the muted icon of what the row is.
// No row leaves it empty.
func queueLead(c *ui.Context, loading bool, icon *ui.SVG) {
	ui.Box(c.Key("lead")).Size(24, 24).Center().Shrink(0).Children(func() {
		if loading {
			kit.Loader(c, 14)
			return
		}
		ui.Icon(c, icon).FontSize(14).TextColor(theme.Of(c).MutedForeground)
	})
}

// queueRow is 32 high, rounded 2xl: the leading slot, the label, a file icon
// when it carries files, then its actions; small round ghost buttons. The
// slot is the drag handle (SortableList's, dimmed while read-only) on a row
// that can be moved: a message or a compaction outside a child session. A
// row that cannot shows its state there: the loader while its guidance is
// pending or, on a background notice, while it is the next to be delivered;
// otherwise the muted icon of its kind, the background command's for a
// notice, which is delivered in its own order. A child session's row has
// Read in Edit's place and no Send now.
func (s *State) queueRow(c *ui.Context, ctx *Context, actions Actions, item Queued, handle func()) {
	p := theme.Of(c)
	label := queueLabel(c, item)
	blocked := queueBlocked(ctx)
	row := ui.Row(c.Key(item.ID)).FillWidth().MinWidth(0).Height(32).PaddingX(8).Gap(8).Radius(p.Radius2xl()).
		AlignItems(ui.Center).Shrink(0).Label(label).Transition(queueMotion)
	if row.Hovered() {
		row.Background(p.Hover)
	}
	row.Children(func() {
		switch {
		case item.Guided:
			queueLead(c, true, nil)
		case item.Notice:
			queueLead(c, ctx.Queue[0].ID == item.ID, icons.Terminal)
		case !ctx.Child:
			handle()
		case item.Compaction:
			queueLead(c, false, icons.Minimize)
		default:
			queueLead(c, false, icons.Chat)
		}
		ui.Text(c, label).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
		if len(item.Attachments) > 0 {
			ui.Icon(c, icons.File).FontSize(16).Shrink(0)
		}
		if !item.Notice && !item.Compaction {
			icon, tip := icons.Replace, "queue_edit"
			if ctx.Child {
				icon, tip = icons.FileText, "queue_read"
			}
			queueSmall(c, "edit", icon, i18n.T(c, tip), blocked || item.Guided, func() {
				s.queue = queueState{editing: item.ID, text: item.Text, original: item.Text, revision: item.Revision, generation: s.queue.generation + 1}
				s.loadQueued(item)
			})
		}
		if item.Guided {
			queueSmall(c, "withdraw", icons.Undo, i18n.T(c, "queue_withdraw"), blocked, func() { actions.WithdrawGuidance(item.ID) })
		}
		if !item.Notice && !ctx.Child {
			queueSmall(c, "send", icons.ArrowUp, i18n.T(c, "queue_send_now"), blocked || item.Guided && !item.Joining, func() { actions.SendQueued(item.ID) })
		}
		queueSmall(c, "delete", icons.Delete, i18n.T(c, "queue_delete"), blocked || item.Guided, func() { actions.RemoveQueued(item.ID) })
	})
}

// outgoingRow is the message on its way to the queue: the loader in the
// leading slot, its label, and Edit, Send now and Remove, all disabled until
// the Node lists it (queue-outgoing).
func (s *State) outgoingRow(c *ui.Context, item Queued) {
	p := theme.Of(c)
	label := queueLabel(c, item)
	ui.Row(c.Key("outgoing")).FillWidth().MinWidth(0).Height(32).PaddingX(8).Gap(8).Radius(p.Radius2xl()).
		AlignItems(ui.Center).Shrink(0).Label(label).Transition(queueMotion).Children(func() {
		queueLead(c, true, nil)
		ui.Text(c, label).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0).TextColor(p.MutedForeground)
		if len(item.Attachments) > 0 {
			ui.Icon(c, icons.File).FontSize(16).Shrink(0)
		}
		queueSmall(c, "edit", icons.Replace, i18n.T(c, "queue_edit"), true, func() {})
		queueSmall(c, "send", icons.ArrowUp, i18n.T(c, "queue_send_now"), true, func() {})
		queueSmall(c, "delete", icons.Delete, i18n.T(c, "queue_delete"), true, func() {})
	})
}

func (s *State) loadQueued(item Queued) {
	if item.Load == nil {
		return
	}
	q := &s.queue
	generation := q.generation
	q.loading = true
	item.Load(func(text string, ok bool) {
		if q.editing != item.ID || q.generation != generation {
			return
		}
		q.loading = false
		if ok {
			q.text = text
		}
	})
}

// after is the ID of the message after id, "" at the end.
func after(list []Queued, id string) string {
	for i, q := range list {
		if q.ID == id && i+1 < len(list) {
			return list[i+1].ID
		}
	}
	return ""
}

// queueEditor is the message in a text area, then Reload when the queued
// message changed meanwhile, Cancel and Save, then the message's files as the
// draft's cards, without their actions, at the text's inset. Enter saves,
// Shift+Enter breaks the line, Escape cancels (the conversation's Escape
// order, State.Escape). A child session's is read-only: the text, then Copy,
// Close and Remove.
func (s *State) queueEditor(c *ui.Context, ctx *Context, actions Actions) {
	p := theme.Of(c)
	q := &s.queue
	var item Queued
	for _, x := range ctx.Queue {
		if x.ID == q.editing {
			item = x
		}
	}
	changed := item.Text != q.original || item.Revision != q.revision
	empty := strings.TrimSpace(q.text) == "" && len(item.Attachments) == 0
	save := func() {
		if changed || empty || q.loading || q.saving {
			return
		}
		if item.Save != nil {
			q.saving = true
			id, generation, text := q.editing, q.generation, strings.TrimSpace(q.text)
			item.Save(text, func(ok bool) {
				if q.editing != id || q.generation != generation {
					return
				}
				q.saving = false
				if ok && strings.TrimSpace(q.text) == text {
					*q = queueState{generation: generation}
				}
			})
		} else {
			actions.SaveQueued(q.editing, strings.TrimSpace(q.text))
			*q = queueState{generation: q.generation}
		}
	}
	ui.Column(c.Key("queue-editor")).FillWidth().Padding(4).Gap(8).Children(func() {
		name := "queue_edit"
		if ctx.Child {
			name = "queue_read"
		}
		area := ui.TextAreaBase(c.Key("queue-input"), &q.text).Label(i18n.T(c, name)).Lines(1, 6).FontSize(14).
			LineHeight(1.5).TextColor(p.Foreground).Padding(4).AutoFocus().Disabled(q.loading || q.saving).ReadOnly(ctx.Child)
		composing := area.Composing() // the callback runs after this pass
		area.OnTextKey(func(k ui.TextKey) bool {
			if k.Key == ui.KeyEnter && k.Mods == 0 && !composing && !ctx.Child {
				save()
				return true
			}
			return false
		})
		// One row: the message's files at the text's inset, scrolling in
		// the room they have, then the actions at the end.
		ui.Row(c.Key("queue-actions")).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
			if len(item.Attachments) > 0 {
				s.strip(c.Key("queue-attachments"), item.Attachments, nil).Shrink(1).MinWidth(0).PaddingX(4)
			}
			ui.Spacer(c)
			button := func(key, label string) ui.Element {
				return kit.Ghost(c.Key(key), label, false).Height(28).PaddingX(12).Radius(14).Border(1, p.Border).
					Children(func() { ui.Text(c, label).FontSize(14) })
			}
			if ctx.Child {
				kit.IconButton(c.Key("copy"), icons.Copy, i18n.T(c, "message_copy"), 28, false).Radius(14).Disabled(q.loading).
					OnClick(func() {
						c.WriteClipboard(q.text)
						kit.Message(c, i18n.T(c, "content_copied"))
					})
				button("close", i18n.T(c, "close")).OnClick(func() { *q = queueState{generation: q.generation + 1} })
				id := q.editing
				kit.IconButton(c.Key("remove"), icons.Delete, i18n.T(c, "queue_delete"), 28, false).Radius(14).
					Disabled(queueBlocked(ctx) || changed).OnClick(func() {
					*q = queueState{generation: q.generation + 1}
					actions.RemoveQueued(id)
				})
				return
			}
			if changed {
				reload := button("reload", i18n.T(c, "queue_reload"))
				kit.Tooltip(c, reload, i18n.T(c, "queue_reload_draft"))
				reload.OnClick(func() {
					q.text, q.original = item.Text, item.Text
					q.revision = item.Revision
					s.loadQueued(item)
				})
			}
			button("cancel", i18n.T(c, "settings_cancel")).OnClick(func() { *q = queueState{generation: q.generation + 1} })
			kit.PrimaryBusy(c.Key("save"), i18n.T(c, "queue_save"), changed || empty || q.loading, q.saving).Height(28).Radius(14).OnClick(save)
		})
	})
}
