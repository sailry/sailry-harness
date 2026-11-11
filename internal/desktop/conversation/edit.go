package conversation

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// edit is the text of the message a conversation edits in its bubble. The
// backend owns whether a turn is edited (Turn.Editing); the view owns what is
// typed.
type edit struct {
	turn, text string
}

// editor is a sent message's bubble as its editor: the text in a field of 1
// to 12 lines, Cancel and Save under it. Enter or Save replaces the turn;
// Escape, Cancel or the focus leaving the editor restores the bubble. While
// the replacement is in flight the field and Save are disabled and nothing
// cancels.
func (s *State) editor(c *ui.Context, session string, t *Turn, actions Actions) {
	p := theme.Of(c)
	e := s.edits[session]
	if e == nil || e.turn != t.ID {
		if s.edits == nil {
			s.edits = map[string]*edit{}
		}
		e = &edit{turn: t.ID, text: t.Editing.Text}
		s.edits[session] = e
	}
	pending := t.Editing.Pending
	empty := strings.TrimSpace(e.text) == "" && len(t.Images) == 0 && len(t.Files) == 0
	save := func() {
		if !pending && !empty {
			actions.SaveEdit(t.ID, e.text)
		}
	}
	cancel := func() {
		if !pending {
			delete(s.edits, session)
			actions.CancelEdit(t.ID)
		}
	}
	ui.Row(c).Justify(ui.End).Children(func() {
		box := ui.Column(c.Key("message-editor")).Width(bubbleMax).MaxWidth(bubbleMax).Shrink(1).Padding(6, 8).Gap(6)
		bubble(c, box).Children(func() {
			area := ui.TextAreaBase(c.Key("message-input"), &e.text).Label(i18n.T(c, "message_edit")).Lines(1, 12).
				FontSize(kit.MessageTextSize(c)).LineHeight(1.6).Padding(3, 6).AutoFocus().ReadOnly(pending)
			composing := area.Composing() // the callback runs after this pass
			area.OnTextKey(func(k ui.TextKey) bool {
				switch {
				case k.Key == ui.KeyEnter && k.Mods == 0 && !composing:
					save()
					return true
				case k.Key == ui.KeyEscape && k.Mods == 0:
					cancel()
					return true
				}
				return false
			})
			left := area.FocusLeft()
			// Pressing an action keeps the field's focus, so only leaving the
			// editor cancels.
			ui.Row(c.Key("message-actions")).FillWidth().Gap(8).Justify(ui.End).AlignItems(ui.Center).KeepFocus().Children(func() {
				if pending {
					kit.Loader(c, 14)
				}
				kit.Ghost(c.Key("cancel"), i18n.T(c, "settings_cancel"), false).Height(28).PaddingX(12).Radius(14).Border(1, p.Border).
					Disabled(pending).Children(func() { ui.Text(c, i18n.T(c, "settings_cancel")).FontSize(14) }).OnClick(cancel)
				kit.Primary(c.Key("save"), i18n.T(c, "queue_save"), pending || empty).Height(28).Radius(14).
					Children(func() { ui.Text(c, i18n.T(c, "queue_save")).FontSize(14) }).OnClick(save)
			})
			if left {
				cancel()
			}
		})
	})
}
