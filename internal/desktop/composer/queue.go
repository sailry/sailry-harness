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
// place and Withdraw, with Edit, Send now and Remove disabled. Rows come in
// and go out collapsing, as the strip does.

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

// queueLabel is a row's text: a notice's title, the compaction's, the
// preview, or the names of its files.
func queueLabel(c *ui.Context, q Queued) string {
	switch {
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
// rounded 2xl, in the secondary color, the rows scrolling past 132 DIPs; or
// the editor of one of them.
func (s *State) queueTray(c *ui.Context, ctx *Context, actions Actions) {
	q := &s.queue
	if q.editing != "" && !queued(ctx, q.editing) {
		q.editing = "" // sent or removed meanwhile
	}
	ui.Column(c.Key("queue-slot")).FillWidth().Children(func() {
		if len(ctx.Queue) == 0 && q.editing == "" {
			return
		}
		p := theme.Of(c)
		r := p.Radius2xl()
		kit.Frost(c, ui.Column(c.Key("queue")).MarginX(12).Padding(4).Gap(4).Radius(r, r, 0, 0), p.Secondary, kit.SurfaceTint).
			Label(i18n.T(c, "queue_count", "count", len(ctx.Queue))).Transition(queueMotion).Children(func() {
			if q.editing != "" {
				s.queueEditor(c, ctx, actions)
				return
			}
			ui.Scroll(c.Key("queue-list")).FillWidth().MaxHeight(132).Children(func() {
				// Dragged by the handle into another place: the move goes
				// to the conversation, before the message now after it.
				list := slices.Clone(ctx.Queue)
				kit.SortableList(c, &list, func(q Queued) string { return q.ID }, kit.SortableListProps{
					Gap: 4, Handle: i18n.T(c, "queue_drag"),
					OnMove: func(_, to int) { actions.MoveQueued(list[to].ID, after(list, list[to].ID)) },
				}, func(item Queued, handle func()) { s.queueRow(c, ctx, actions, item, handle) })
			})
		})
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

// queueRow is 32 high, rounded 2xl, the drag handle (SortableList's; none for
// a notice or while read-only) or, while its guidance is pending, the loader,
// the label, a file icon when it carries files, then its actions; small round
// ghost buttons.
func (s *State) queueRow(c *ui.Context, ctx *Context, actions Actions, item Queued, handle func()) {
	p := theme.Of(c)
	label := queueLabel(c, item)
	draggable := !item.Notice && !ctx.Readonly && !item.Guided
	row := ui.Row(c.Key(item.ID)).FillWidth().MinWidth(0).Height(32).PaddingX(8).Gap(8).Radius(p.Radius2xl()).
		AlignItems(ui.Center).Shrink(0).Label(label).Transition(queueMotion)
	if row.Hovered() {
		row.Background(p.Hover)
	}
	small := func(key string, icon *ui.SVG, tip string, disabled bool, fn func()) {
		kit.IconButton(c.Key(key), icon, tip, 24, false).Radius(12).Disabled(disabled).OnClick(fn)
	}
	row.Children(func() {
		switch {
		case item.Guided:
			ui.Box(c.Key("joining")).Size(24, 24).Center().Shrink(0).Children(func() { kit.Loader(c, 16) })
		case draggable:
			handle()
		}
		ui.Text(c, label).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
		if len(item.Attachments) > 0 {
			ui.Icon(c, icons.File).FontSize(16).Shrink(0)
		}
		if !item.Notice && !item.Compaction {
			small("edit", icons.Replace, i18n.T(c, "queue_edit"), ctx.Readonly || item.Guided, func() {
				s.queue = queueState{editing: item.ID, text: item.Text, original: item.Text, revision: item.Revision, generation: s.queue.generation + 1}
				s.loadQueued(item)
			})
		}
		if item.Guided {
			small("withdraw", icons.Undo, i18n.T(c, "queue_withdraw"), ctx.Readonly, func() { actions.WithdrawGuidance(item.ID) })
		}
		if !item.Notice {
			small("send", icons.ArrowUp, i18n.T(c, "queue_send_now"), ctx.Readonly || item.Guided, func() { actions.SendQueued(item.ID) })
		}
		small("delete", icons.Delete, i18n.T(c, "queue_delete"), ctx.Readonly || item.Guided, func() { actions.RemoveQueued(item.ID) })
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
// order, State.Escape).
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
		area := ui.TextAreaBase(c.Key("queue-input"), &q.text).Label(i18n.T(c, "queue_edit")).Lines(1, 6).FontSize(14).
			LineHeight(1.5).TextColor(p.Foreground).Padding(4).AutoFocus().Disabled(q.loading || q.saving)
		composing := area.Composing() // the callback runs after this pass
		area.OnTextKey(func(k ui.TextKey) bool {
			if k.Key == ui.KeyEnter && k.Mods == 0 && !composing {
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
