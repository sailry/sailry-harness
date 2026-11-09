package conversation

import (
	"runtime"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Message actions. The buttons always show, as Kit's MessageFooter; the time
// shows while the pointer is over the footer, which alone reads its hover, so
// hovering rebuilds no more than that row.

// AnswerText is what Copy copies of a reply: its answer's text blocks.
func AnswerText(blocks []Block) string {
	var parts []string
	for _, b := range blocks[AnswerFrom(blocks):] {
		if b.Kind == TextBlock {
			parts = append(parts, b.Text)
		}
	}
	return strings.Join(parts, "\n\n")
}

// Resendable reports a run that can run again: one that failed, stopped or
// broke off.
func Resendable(st RunStatus) bool {
	return st == Failed || st == Interrupted || st == Cancelled
}

// cited returns the answer with the citations placed in its last text
// block, a copy so the turn's own blocks stay as they came.
func cited(blocks []Block, cites []Citation) []Block {
	for i := len(blocks) - 1; i >= 0; i-- {
		if blocks[i].Kind == TextBlock {
			out := append([]Block(nil), blocks...)
			out[i].Text = Cite(out[i].Text, cites)
			return out
		}
	}
	return blocks
}

// footer is the ended run's row: Copy, Read aloud, the fork menu (Fork,
// Rewind, New worktree), Retry where the run can run again, the export
// menu (Markdown, HTML, Image), and the time it finished on hover.
func (s *State) footer(c *ui.Context, t *Turn, blocks []Block, actions Actions) {
	p := theme.Of(c)
	text := AnswerText(blocks)
	row := ui.Row(c.Key("footer")).FillWidth().Gap(8).AlignItems(ui.Center).TextColor(p.MutedForeground)
	hovered := row.Hovered()
	row.Children(func() {
		if text != "" {
			kit.IconButton(c.Key("copy"), icons.Copy, i18n.T(c, "turn_copy"), 28, false).OnClick(func() {
				c.WriteClipboard(text)
				kit.Message(c, i18n.T(c, "content_copied"))
			})
			icon, label := icons.Volume, "reader_read"
			reading := s.reading == t.ID
			if s.ReaderActive != nil {
				reading = s.ReaderActive(t.ID)
			}
			if reading {
				icon, label = icons.Square, "reader_stop"
			}
			kit.IconButton(c.Key("read"), icon, i18n.T(c, label), 28, false).OnClick(func() {
				if s.ReaderActive == nil {
					if s.reading == t.ID {
						s.reading = ""
					} else {
						s.reading = t.ID
					}
				}
				actions.Read(t.ID, text)
			})
		}
		if !t.Reply.Notice {
			kit.MenuButton(c.Key("fork"), "fork-"+t.ID, func(b ui.Element) {
				kit.IconLook(c, b, icons.Network, i18n.T(c, "chat_fork"), "", 28, false)
			}, func(m *kit.Menu) {
				if m.Item(i18n.T(c, "chat_fork")).Icon(icons.Network).Chosen() {
					actions.Fork(t.ID)
				}
				if m.Item(i18n.T(c, "chat_rewind")).Icon(icons.Undo).Chosen() {
					s.rewind = t.ID
				}
				if m.Item(i18n.T(c, "worktree_create")).Icon(icons.Plus).Chosen() {
					actions.Worktree(t.ID)
				}
			})
			if Resendable(t.Reply.Status) {
				kit.IconButton(c.Key("retry"), icons.RotateCw, i18n.T(c, "chat_retry"), 28, false).OnClick(func() { actions.Retry(t.ID) })
			}
		}
		exportMenu(c, "export-"+t.ID, t.ID, actions)
		if hovered && !t.Reply.Finished.IsZero() {
			ui.Text(c, t.Reply.Finished.Format("15:04")).FontSize(14).SingleLine()
		}
	})
}

// exportMenu is the More menu exporting a turn ("" the conversation) as
// Markdown, HTML or an image.
func ExportSupported(format ExportFormat) bool {
	return format != ExportImage || runtime.GOOS == "darwin" || runtime.GOOS == "windows"
}

func exportMenu(c *ui.Context, id, turn string, actions Actions) {
	kit.MenuButton(c.Key("export"), id, func(b ui.Element) {
		kit.IconLook(c, b, icons.EllipsisVertical, i18n.T(c, "more"), "", 28, false)
	}, func(m *kit.Menu) {
		for i, key := range []string{"conversation_export_markdown", "conversation_export_html", "conversation_export_image"} {
			if m.Item(i18n.T(c, key)).Disabled(!ExportSupported(ExportFormat(i))).Chosen() {
				actions.Export(turn, ExportFormat(i))
			}
		}
	})
}

// userFooter is under a user message, at the end: the time it was sent on
// hover, Copy and Edit.
func userFooter(c *ui.Context, t *Turn, actions Actions) {
	p := theme.Of(c)
	row := ui.Row(c.Key("user-footer")).FillWidth().Gap(8).Justify(ui.End).AlignItems(ui.Center).TextColor(p.MutedForeground)
	hovered := row.Hovered()
	row.Children(func() {
		if hovered && !t.Sent.IsZero() {
			ui.Text(c, t.Sent.Format("15:04")).FontSize(14).SingleLine()
		}
		if t.User != "" {
			kit.IconButton(c.Key("copy"), icons.Copy, i18n.T(c, "message_copy"), 28, false).OnClick(func() {
				c.WriteClipboard(t.User)
				kit.Message(c, i18n.T(c, "content_copied"))
			})
		}
		kit.IconButton(c.Key("edit"), icons.Writing, i18n.T(c, "message_edit"), 28, false).OnClick(func() { actions.Edit(t.ID) })
	})
}

// confirmRewind asks before a rewind: the turn is kept, the later messages go
// and the files stay.
func (s *State) confirmRewind(c *ui.Context, actions Actions) {
	open := s.rewind != ""
	if !open {
		return
	}
	turn := s.rewind
	kit.Dialog(c, &open, func() {
		ui.Text(c, i18n.T(c, "chat_rewind")).FontSize(16).FontWeight(600)
		ui.Text(c, i18n.T(c, "chat_rewind_confirm")).FontSize(14).MaxWidth(400)
		ui.Row(c).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("cancel"), i18n.T(c, "interaction_cancel")).OnClick(func() { s.rewind = "" })
			kit.Danger(c.Key("rewind"), i18n.T(c, "chat_rewind_action")).OnClick(func() {
				s.rewind = ""
				actions.Rewind(turn)
			})
		})
	})
	if !open {
		s.rewind = ""
	}
}
