package conversation

import (
	"fmt"
	"runtime"
	"slices"
	"strings"
	"unicode/utf8"

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
	var first string
	var rest strings.Builder
	for _, b := range blocks[AnswerFrom(blocks):] {
		if b.Kind != TextBlock || strings.TrimSpace(b.Text) == "" {
			continue
		}
		if first == "" {
			first = b.Text
			continue
		}
		if rest.Len() == 0 {
			rest.WriteString(first)
		}
		rest.WriteString("\n\n")
		rest.WriteString(b.Text)
	}
	if rest.Len() == 0 {
		return first
	}
	return rest.String()
}

// Resendable reports a run that can run again: one that failed, stopped or
// broke off.
func Resendable(st RunStatus) bool {
	return st == Failed || st == Interrupted || st == Cancelled
}

var citationURIEscaper = strings.NewReplacer("<", "%3C", ">", "%3E", "\\", "%5C", "\n", "%0A", "\r", "%0D")

// cited places turn-relative rune endpoints across all text blocks without
// changing canonical text, copy/export output or citation numbering.
func cited(blocks []Block, cites []Citation) []Block {
	if len(cites) == 0 {
		return blocks
	}
	last := -1
	for i := len(blocks) - 1; i >= 0; i-- {
		if blocks[i].Kind == TextBlock {
			last = i
			break
		}
	}
	if last < 0 {
		return blocks
	}
	out := blocks
	cloned := false
	offset := 0
	for i := range out {
		if out[i].Kind != TextBlock {
			continue
		}
		text := out[i].Text
		length := utf8.RuneCountInString(text)
		marks := map[int][]int{}
		for number, citation := range cites {
			position := -1
			switch {
			case citation.End < 0:
				// Negative endpoints encode entry starts as -(offset+1); zero
				// remains the existing model's unpositioned-source sentinel.
				start := -(citation.End + 1)
				if start >= offset && start < offset+length {
					position = start - offset
				}
			case citation.End > offset && citation.End <= offset+length:
				position = citation.End - offset
			case i == last && (citation.End == 0 || citation.End > offset+length):
				position = length
			}
			if position >= 0 {
				marks[position] = append(marks[position], number)
			}
		}
		if len(marks) > 0 {
			positions := make([]int, 0, len(marks))
			for position := range marks {
				positions = append(positions, position)
			}
			slices.Sort(positions)
			var result strings.Builder
			// Convert only requested rune offsets, walking each byte at most once.
			runeOffset, byteOffset := 0, 0
			for _, position := range positions {
				start := byteOffset
				if position == length {
					byteOffset = len(text)
				} else {
					for runeOffset < position {
						_, width := utf8.DecodeRuneInString(text[byteOffset:])
						byteOffset += width
						runeOffset++
					}
				}
				result.WriteString(text[start:byteOffset])
				for _, number := range marks[position] {
					uri := citationURIEscaper.Replace(cites[number].URI)
					fmt.Fprintf(&result, " [%d](<%s>)", number+1, uri)
				}
			}
			result.WriteString(text[byteOffset:])
			if !cloned {
				out = slices.Clone(blocks)
				cloned = true
			}
			out[i].Text = result.String()
		}
		offset += length
	}
	return out
}

// quiet gives a message's action the footer's lighter tone, the text's own
// while the pointer is on it.
func quiet(c *ui.Context, b ui.Element) ui.Element {
	p := theme.Of(c)
	if b.Hovered() || b.Pressed() {
		return b.TextColor(p.Foreground)
	}
	return b.TextColor(p.Description())
}

// footer is the ended run's row: Copy, Read aloud, the fork menu (Fork,
// Rewind, New worktree), Retry where the run can run again, the export
// menu (Markdown, HTML, Image), and the time it finished while the pointer
// is over the reply (over) or the row.
func (s *State) footer(c *ui.Context, t *Turn, over bool, actions Actions) {
	p := theme.Of(c)
	text := replyText(t)
	session := s.renderSession
	row := ui.Row(c.Key("footer")).FillWidth().Gap(8).AlignItems(ui.Center).TextColor(p.Description())
	hovered := over || row.Hovered()
	row.Children(func() {
		if text != "" {
			quiet(c, kit.IconButton(c.Key("copy"), icons.Copy, i18n.T(c, "turn_copy"), 28, false)).OnClick(func() {
				c.WriteClipboard(text)
				kit.Copied(c)
			})
			icon, label := icons.Volume, "reader_read"
			reading := s.reading == t.ID
			if s.ReaderActive != nil {
				reading = s.ReaderActive(t.ID)
			}
			if reading {
				icon, label = icons.Square, "reader_stop"
			}
			quiet(c, kit.IconButton(c.Key("read"), icon, i18n.T(c, label), 28, false)).OnClick(func() {
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
			// A read-only session forks, rewinds and branches nothing.
			if ctl := t.Controls; ctl.ReadOnly {
			} else if ctl.Busy {
				ui.Row(c.Key("fork-busy")).Width(28).Height(28).Center().Label(i18n.T(c, "chat_fork")).Role(ui.RoleStatus).
					Children(func() { kit.Loader(c, 14) })
			} else {
				kit.MenuButton(c.Key("fork"), "fork-"+t.ID, func(b ui.Element) {
					quiet(c, kit.IconLook(c, b, icons.Network, i18n.T(c, "chat_fork"), "", 28, false))
				}, func(m *kit.Menu) {
					if m.Item(i18n.T(c, "chat_fork")).Icon(icons.Network).Disabled(ctl.NoFork).Chosen() {
						actions.Fork(t.ID)
					}
					if m.Item(i18n.T(c, "chat_rewind")).Icon(icons.Undo).Disabled(ctl.NoRewind).Chosen() {
						s.rewind, s.rewindOf = t.ID, session
					}
					if m.Item(i18n.T(c, "worktree_create")).Icon(icons.Plus).Disabled(ctl.NoWorktree).Chosen() {
						actions.Worktree(t.ID)
					}
				})
			}
			if Resendable(t.Reply.Status) && !t.Controls.ReadOnly {
				quiet(c, kit.IconButton(c.Key("retry"), icons.RotateCw, i18n.T(c, "chat_retry"), 28, false)).OnClick(func() { actions.Retry(t.ID) })
			}
		}
		// The demo's conversations are in no file: it has no export menu.
		if !kit.Demo(c) {
			exportMenu(c, "export-"+t.ID, t.ID, t.Controls.NoExport, actions)
		}
		if hovered && !t.Reply.Finished.IsZero() {
			ui.Text(c, t.Reply.Finished.Format("15:04")).FontSize(14).SingleLine()
		}
	})
}

// replyText keeps the answer of every exchange when guidance interrupted
// a task; work in a later segment does not hide an earlier answer from Copy.
func replyText(t *Turn) string {
	if len(t.Segments) == 0 {
		return AnswerText(t.Reply.Blocks)
	}
	var texts []string
	for index, segment := range t.Segments {
		end := len(t.Reply.Blocks)
		if index+1 < len(t.Segments) {
			end = t.Segments[index+1].From
		}
		if text := AnswerText(t.Reply.Blocks[segment.From:end]); text != "" {
			texts = append(texts, text)
		}
	}
	return strings.Join(texts, "\n\n")
}

// ExportSupported reports whether this system exports format.
func ExportSupported(format ExportFormat) bool {
	return format != ExportImage || runtime.GOOS == "darwin" || runtime.GOOS == "windows"
}

// exportMenu is the More menu exporting a turn ("" the conversation) as
// Markdown, HTML or an image; disabled while the Node does not answer.
func exportMenu(c *ui.Context, id, turn string, disabled bool, actions Actions) {
	kit.MenuButton(c.Key("export"), id, func(b ui.Element) {
		quiet(c, kit.IconLook(c, b, icons.EllipsisVertical, i18n.T(c, "more"), "", 28, false))
	}, func(m *kit.Menu) {
		for i, key := range []string{"conversation_export_markdown", "conversation_export_html", "conversation_export_image"} {
			if m.Item(i18n.T(c, key)).Disabled(disabled || !ExportSupported(ExportFormat(i))).Chosen() {
				actions.Export(turn, ExportFormat(i))
			}
		}
	})
}

// userFooter is under a user message, at the end: the time it was sent
// while the pointer is over the message (over) or the row, Copy and Edit.
func userFooter(c *ui.Context, t *Turn, over bool, actions Actions) {
	p := theme.Of(c)
	row := ui.Row(c.Key("user-footer")).FillWidth().Gap(8).Justify(ui.End).AlignItems(ui.Center).TextColor(p.Description())
	hovered := over || row.Hovered()
	row.Children(func() {
		if hovered && !t.Sent.IsZero() {
			ui.Text(c, t.Sent.Format("15:04")).FontSize(14).SingleLine()
		}
		if t.User != "" {
			quiet(c, kit.IconButton(c.Key("copy"), icons.Copy, i18n.T(c, "message_copy"), 28, false)).OnClick(func() {
				c.WriteClipboard(t.User)
				kit.Copied(c)
			})
		}
		// A read-only session has no Edit; elsewhere it is disabled while it
		// cannot apply or an editor is already open.
		if !t.Controls.ReadOnly {
			quiet(c, kit.IconButton(c.Key("edit"), icons.Writing, i18n.T(c, "message_edit"), 28, t.Editing != nil)).
				Disabled(t.Controls.NoEdit || t.Editing != nil).OnClick(func() { actions.Edit(t.ID) })
		}
	})
}

// confirmRewind asks before a rewind: the turn is kept, the later messages go
// and the files stay.
func (s *State) confirmRewind(c *ui.Context, session string, actions Actions) {
	open := s.rewind != "" && s.rewindOf == session
	if !open {
		return
	}
	turn := s.rewind
	if kit.Confirm(c.Key(turn), &open, kit.Confirmation{
		Title: i18n.T(c, "chat_rewind"), Message: i18n.T(c, "chat_rewind_confirm"),
		Buttons: []string{i18n.T(c, "interaction_cancel"), i18n.T(c, "chat_rewind_action")}, Default: 1, Destructive: []int{1},
	}) == 1 {
		s.rewind = ""
		actions.Rewind(turn)
	}
	if !open {
		s.rewind = ""
	}
}

// AskRewind opens the rewind confirmation for a turn of session, as the fork
// menu's Rewind does.
func (s *State) AskRewind(session, turn string) { s.rewind, s.rewindOf = turn, session }
