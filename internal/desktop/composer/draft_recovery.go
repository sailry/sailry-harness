package composer

import (
	"slices"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

type savedDraft struct {
	text, host  string
	references  []Reference
	attachments []*Attachment
}

type draftRecovery struct {
	drafts   []*savedDraft
	open     bool
	outgoing []*Attachment
	pending  func() bool
}

// KeepFailedDraft keeps a rejected send's input, not the settings it would
// have run with. A newer draft is left in place.
func (s *State) KeepFailedDraft(m Message, restoreNow bool) {
	text := m.draftText
	if text == "" {
		text = m.Text
	}
	d := &savedDraft{text: text, host: m.draftHost, references: slices.Clone(m.References), attachments: slices.Clone(m.Attachments)}
	if restoreNow && s.draftEmpty() && d.onHost(s.Host) {
		s.applyDraft(d)
		return
	}
	s.recovery.drafts = append(s.recovery.drafts, d)
}

// Idle reports whether the composer holds no user draft, attachments,
// recovery entries, active run, dictation or open popovers.
func (s *State) Idle() bool {
	if s == nil {
		return true
	}
	for _, open := range s.act.badges {
		if *open {
			return false
		}
	}
	return !s.in.approvals && !s.in.question && !s.in.receipts && s.in.asked == "" &&
		!s.act.changes && !s.act.progress && !s.act.commands && !s.act.children &&
		!s.pluginEdit.composing && s.pluginEdit.pending == nil &&
		s.Draft == "" && len(s.Attachments) == 0 && len(s.References) == 0 &&
		!s.Running && !s.Stoppable && s.Dictation.Phase == Idle &&
		len(s.recovery.drafts) == 0 && len(s.recovery.outgoing) == 0 && s.recovery.pending == nil &&
		len(s.later) == 0 && !s.picker && !s.settings && !s.stats && !s.projectPicker &&
		!s.queueOpen && s.queue.editing == "" && s.queue.text == "" && !s.queue.saving && !s.queue.loading &&
		len(s.view.Images) == 0
}

// ClearLiveCallbacks drops closures and surface callbacks held for the
// controller before a retained composer leaves view.
func (s *State) ClearLiveCallbacks() {
	if s == nil {
		return
	}
	s.pluginControls = nil
	s.pluginCommand = nil
	s.statusCallbacks = nil
	s.ActivityCallbacks = nil
	s.Metrics = nil
	s.Indicator = nil
	s.SelectTeam = nil
	s.attachmentCallbacks = nil
	s.referenceChildren = nil
	s.referencePickerOpen = nil
	s.view.close()
}

// HoldOutgoingAttachments keeps the staged files valid even if a newer draft
// removes one of their cards or reference chips while admission is pending.
func (s *State) HoldOutgoingAttachments(files []*Attachment, pending func() bool) {
	if len(files) == 0 {
		pending = nil
	}
	s.recovery.outgoing, s.recovery.pending = slices.Clone(files), pending
}

func (s *State) ReleaseCommittedAttachment(a *Attachment) {
	if slices.Contains(s.view.Images, a) {
		s.view.close()
	}
	r := &s.recovery
	if i := slices.Index(r.outgoing, a); i >= 0 {
		r.outgoing = slices.Delete(r.outgoing, i, i+1)
		if len(r.outgoing) == 0 {
			r.outgoing, r.pending = nil, nil
		}
	}
}

func (s *State) holdsDraftAttachment(a *Attachment) bool {
	r := &s.recovery
	return r.pending != nil && r.pending() && slices.Contains(r.outgoing, a) ||
		slices.ContainsFunc(r.drafts, func(d *savedDraft) bool { return slices.Contains(d.attachments, a) })
}

func (s *State) draftEmpty() bool {
	return s.Draft == "" && len(s.References) == 0 && len(s.Attachments) == 0
}

func (d *savedDraft) onHost(host string) bool {
	return len(d.attachments) == 0 || d.host == "" || d.host == host
}

func (s *State) applyDraft(d *savedDraft) {
	s.Draft, s.References, s.Attachments = d.text, d.references, d.attachments
	s.ref = refState{}
	s.caret = utf8.RuneCountInString(d.text)
}

func (s *State) draftRecovery(c *ui.Context, ctx *Context) {
	r := &s.recovery
	if r.pending != nil && !r.pending() {
		r.outgoing, r.pending = nil, nil
	}
	if len(r.drafts) == 0 {
		r.open = false
		return
	}
	label := i18n.T(c, "chat_restore_draft")
	trigger := chip(c.Key("draft-recovery"), label, r.open, func() {
		ui.Icon(c, icons.Restore).FontSize(14)
		chipText(c, label)
	}).HasPopup(ui.PopupDialog).OnClick(func() { r.open = !r.open })
	place(c, kit.Popover(c, trigger, &r.open, func() {
		ui.Scroll(c).MaxHeight(240).Gap(8).Children(func() {
			for i, d := range r.drafts {
				ui.Column(c.Key(i)).Gap(4).Children(func() {
					ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
						ui.Text(c, d.text).FontSize(14).Grow(1).MinWidth(0).MaxLines(3).Selectable()
						blocked := func() bool {
							return ctx.Readonly || s.attachmentsLocked() || s.Dictation.Phase != Idle || !s.draftEmpty() || !d.onHost(s.Host)
						}
						hint := ""
						if !s.draftEmpty() {
							hint = i18n.T(c, "queue_draft_present")
						} else if blocked() {
							hint = i18n.T(c, "chat_restore_unavailable")
						}
						kit.IconButtonHint(c.Key("restore"), icons.Restore, label, hint, 28, false).Disabled(blocked()).OnClick(func() {
							if i := slices.Index(r.drafts, d); i >= 0 && !blocked() {
								s.applyDraft(d)
								r.drafts = slices.Delete(r.drafts, i, i+1)
								r.open = false
								s.Focus()
							}
						})
					})
					if len(d.attachments) > 0 {
						s.strip(c.Key("files"), d.attachments, nil).Shrink(0)
					}
				})
			}
		})
	}), trigger, 8)
}
