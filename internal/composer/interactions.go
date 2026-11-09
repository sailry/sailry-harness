package composer

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/theme"
)

// This file holds the composer's interactions: the approvals and the
// question a run waits on, each a capsule over the composer that opens a
// popover (live/approvals.rs, approvals/choices.rs, approvals/changes.rs,
// live/questions/view.rs). The view models are the backend's; the State
// keeps only what the user is typing and which popover is open.

// Interactions is what a run waits on the user for.
type Interactions struct {
	Approvals []Approval
	Question  *Ask // nil when no question is pending
}

// Approval is a call awaiting the user's decision.
type Approval struct {
	ID string
	// Child names the subagent session asking, "" for the conversation's.
	Child string
	// Title is the tool's label; Context where it acts (a path, a host).
	Title, Context string
	// Description is what the call does; Code shows it as a command.
	Description string
	Code        bool
	Notices     []string
	Changes     []Change // a change set replaces the description
	// Remember offers Allow for the session; Prefix, when set, a command
	// prefix to allow from now on.
	Remember bool
	Prefix   string
	// Unrestricted marks a call with full file and network access: Allow
	// says so in its tooltip.
	Unrestricted bool
}

// ChangeKind is what a change does to its file.
type ChangeKind int

const (
	Added ChangeKind = iota
	Modified
	Renamed
	Deleted
)

// Change is a file of a change set.
type Change struct {
	Path, Destination string // Destination: a rename's new path
	Kind              ChangeKind
	Added, Removed    int
	Truncated         bool
}

// Ask is a question the model asked: a choice of Options (Multi for
// several), Other for a free answer, or a free answer alone without them.
type Ask struct {
	ID, Prompt string
	Options    []string
	Multi      bool
	Other      bool
	Declinable bool // a form or a link: Decline shows
}

// Decision is the user's answer to an approval.
type Decision int

const (
	Allow Decision = iota
	AllowSession
	AllowPrefix
	Deny
)

// Answer is the user's answer to a question: the options chosen, by index,
// and the text typed.
type Answer struct {
	Choices []int
	Text    string
}

// Actions is what the composer asks of the backend. Outcomes show as
// toasts: a decision or answer the run no longer waits on is a conflict.
type Actions interface {
	Send(text string)
	Stop()
	// Decide answers approval id; reason is the text of Deny with reason.
	Decide(id string, d Decision, reason string)
	Answer(id string, a Answer)
	CancelQuestion(id string)
	DeclineQuestion(id string)
}

// interaction is the State's part for the interactions.
type interaction struct {
	approvals, question bool // the popovers are open
	reasons             map[string]*string
	asked               string // the question the draft is for
	chosen              int
	choices             map[int]*bool
	other               bool
	text                string
}

// interactions builds the capsules, right-aligned over the composer.
func (s *State) interactions(c *ui.Context, in Interactions, actions Actions) {
	if len(in.Approvals) == 0 && in.Question == nil {
		return
	}
	ui.Row(c.Key("interactions")).Justify(ui.End).Gap(8).Children(func() {
		if len(in.Approvals) > 0 {
			s.approvals(c, in.Approvals, actions)
		}
		if in.Question != nil {
			s.ask(c, in.Question, actions)
		}
	})
}

// capsule is theme.rs capsule: a small round outlined button, muted.
func capsule(c *ui.Context, label string, open bool) ui.Element {
	p := theme.Of(c)
	b := theme.Ghost(c, label, open).Height(28).PaddingX(12).Gap(6).Radius(14).Border(1, p.Border).
		TextColor(p.MutedForeground)
	return b.HasPopup(ui.PopupDialog).Expanded(open).Children(func() {
		ui.Icon(c, icons.CircleUser).FontSize(16)
		ui.Text(c, label).FontSize(14).SingleLine()
	})
}

// popover is theme/floating.rs: the floating fill, borderless with a
// shadow in light, outlined in dark; 320 DIPs wide, anchored at its
// bottom right 8 DIPs over the capsule.
func popover(c *ui.Context, panel, anchor ui.Element) ui.Element {
	p := theme.Of(c)
	panel.AttachTo(anchor, ui.AnchorBottomEnd, ui.AnchorTopEnd).Margin(0, 0, 8, 0).Width(320).Padding(12).
		Radius(8).Background(p.Popover).Shadow(0, 4, 12, 0, ui.RGBA(0, 0, 0, 0.12))
	if p.Dark {
		panel.Border(1, p.Border)
	}
	return panel
}

func (s *State) approvals(c *ui.Context, list []Approval, actions Actions) {
	p := theme.Of(c)
	label := i18n.T(c, "approval_live_count", "count", len(list))
	trigger := capsule(c.Key("approvals"), label, s.in.approvals).OnClick(func() { s.in.approvals = !s.in.approvals })
	ui.PopoverBase(c, trigger, &s.in.approvals, func(panel ui.Element) {
		popover(c, panel, trigger)
		ui.Scroll(c).MaxHeight(256).Gap(12).Children(func() {
			for i, a := range list {
				row := ui.Column(c.Key(a.ID)).Gap(8).Label(a.Title)
				if i > 0 {
					row.Padding(12, 0, 0, 0).BorderWidth(1, 0, 0, 0).BorderColor(p.Border)
				}
				row.Children(func() { s.approval(c, a, i == 0, actions) })
			}
		})
	})
}

// approval is one request: who asks, the tool, where, what, the decision.
func (s *State) approval(c *ui.Context, a Approval, first bool, actions Actions) {
	p := theme.Of(c)
	if a.Child != "" {
		ui.Text(c, a.Child).FontSize(14).SingleLine()
	}
	ui.Text(c, a.Title).FontSize(14).FontWeight(500)
	if a.Context != "" {
		ui.Text(c, a.Context).FontSize(14).TextColor(p.MutedForeground)
	}
	for _, n := range a.Notices {
		ui.Text(c, n).FontSize(14).MaxLines(3)
	}
	if len(a.Changes) > 0 {
		changes(c, a.Changes)
	} else if a.Description != "" {
		if a.Code {
			bg := p.Background.MixOklab(p.Muted, 0.5)
			if p.Dark {
				bg = p.Background
			}
			ui.Scroll(c).MaxHeight(128).Radius(6).Background(bg).Padding(12).Children(func() {
				ui.Text(c, a.Description).Font("monospace").FontSize(14).Selectable()
			})
		} else {
			ui.Text(c, a.Description).FontSize(14).MaxLines(2)
		}
	}
	reason, editing := s.in.reasons[a.ID]
	row := ui.Row(c.Key("decision")).Gap(8).Padding(8, 0, 0, 0).Justify(ui.End)
	if editing {
		ui.TextAreaBase(c.Key("reason"), reason).Label(i18n.T(c, "approval_reason")).Lines(2, 4).FontSize(14).
			Padding(8).Radius(6).Border(1, p.Border)
		row.Children(func() {
			theme.Ghost(c.Key("reason-cancel"), i18n.T(c, "approval_reason_cancel"), false).Height(28).PaddingX(12).Radius(14).
				Children(func() { ui.Text(c, i18n.T(c, "approval_reason_cancel")).FontSize(14) }).
				OnClick(func() { delete(s.in.reasons, a.ID) })
			empty := strings.TrimSpace(*reason) == ""
			theme.Primary(c.Key("reason-submit"), i18n.T(c, "approval_reject"), empty).Height(28).Radius(14).
				Children(func() { ui.Text(c, i18n.T(c, "approval_reject")).FontSize(14) }).
				OnClick(func() {
					text := strings.TrimSpace(*reason)
					delete(s.in.reasons, a.ID)
					actions.Decide(a.ID, Deny, text)
				})
		})
		return
	}
	row.Children(func() {
		// Deny, with a menu of Deny with reason (Kit DropdownButton).
		theme.Ghost(c.Key("deny"), i18n.T(c, "approval_reject"), false).Height(28).PaddingX(12).MinWidth(48).
			Children(func() { ui.Text(c, i18n.T(c, "approval_reject")).FontSize(14) }).
			OnClick(func() { actions.Decide(a.ID, Deny, "") })
		theme.IconButton(c.Key("deny-options"), icons.ChevronDown, i18n.T(c, "approval_deny_reason"), 28, false).
			PopoverMenuButton(func(m *ui.Menu) {
				if m.Item(i18n.T(c, "approval_deny_reason")).Chosen() {
					s.in.reasons[a.ID] = new(string)
				}
			})
		allow := i18n.T(c, "approval_allow_once")
		var b ui.Element
		if first {
			b = theme.Primary(c.Key("allow"), allow, false).Height(28)
		} else {
			b = theme.Ghost(c.Key("allow"), allow, false).Height(28).PaddingX(12).Border(1, p.Border)
		}
		if !a.Remember {
			b.Radius(14)
		}
		if a.Unrestricted {
			b.Tooltip(i18n.T(c, "approval_unrestricted"))
		}
		b.MinWidth(48).Children(func() { ui.Text(c, allow).FontSize(14) }).OnClick(func() { actions.Decide(a.ID, Allow, "") })
		if a.Remember {
			theme.IconButton(c.Key("allow-options"), icons.ChevronDown, i18n.T(c, "approval_allow_session"), 28, false).
				PopoverMenuButton(func(m *ui.Menu) {
					if m.Item(i18n.T(c, "approval_allow_session")).Chosen() {
						actions.Decide(a.ID, AllowSession, "")
					}
					if a.Prefix != "" && m.Item(i18n.T(c, "approval_allow_prefix", "prefix", a.Prefix)).Chosen() {
						actions.Decide(a.ID, AllowPrefix, "")
					}
				})
		}
	})
}

// changes is a change set's files: what each does, its path, +N −M.
func changes(c *ui.Context, list []Change) {
	p := theme.Of(c)
	title := i18n.T(c, "file_changes_approval_one")
	if len(list) != 1 {
		title = i18n.T(c, "file_changes_approval", "count", len(list))
	}
	ui.Column(c).Gap(8).Children(func() {
		ui.Text(c, title).FontSize(14)
		for i, f := range list {
			kind := [...]string{"file_changes_added", "file_changes_modified", "file_changes_renamed", "file_changes_deleted"}[f.Kind]
			path := f.Path
			if f.Destination != "" {
				path += " → " + f.Destination
			}
			ui.Column(c.Key(i)).Gap(4).Children(func() {
				ui.Row(c).Gap(8).Children(func() {
					ui.Text(c, i18n.T(c, kind)).FontSize(14).Shrink(0).TextColor(p.MutedForeground)
					ui.Text(c, path).Font("monospace").FontSize(13).SingleLine().Grow(1).Shrink(1).MinWidth(0)
					ui.Row(c).Gap(6).Shrink(0).Children(func() {
						if f.Added > 0 || f.Removed == 0 {
							ui.Textf(c, "+%d", f.Added).Font("monospace").FontSize(12).TextColor(p.Success)
						}
						if f.Removed > 0 {
							ui.Textf(c, "−%d", f.Removed).Font("monospace").FontSize(12).TextColor(p.Danger)
						}
					})
				})
				if f.Truncated {
					ui.Text(c, i18n.T(c, "file_changes_shortened")).FontSize(12).TextColor(p.MutedForeground)
				}
			})
		}
	})
}

// ask is the pending question's capsule and its popover: the prompt, the
// choices, a free answer, and Decline, Cancel and Submit.
func (s *State) ask(c *ui.Context, q *Ask, actions Actions) {
	p := theme.Of(c)
	in := &s.in
	if in.asked != q.ID { // a new question: a new draft
		in.asked, in.chosen, in.choices, in.other, in.text = q.ID, -1, map[int]*bool{}, false, ""
	}
	label := i18n.T(c, "question_reply")
	trigger := capsule(c.Key("question"), label, in.question).OnClick(func() { in.question = !in.question })
	ui.PopoverBase(c, trigger, &in.question, func(panel ui.Element) {
		popover(c, panel, trigger).Gap(16)
		ui.Scroll(c).Gap(16).Children(func() {
			ui.Text(c, q.Prompt).FontSize(14).FontWeight(500)
			other := len(q.Options) // the index of Other
			if q.Multi {
				ui.Column(c).Gap(12).Children(func() {
					for i, o := range q.Options {
						b, ok := in.choices[i]
						if !ok {
							b = new(bool)
							in.choices[i] = b
						}
						ui.Checkbox(c.Key(i), b, o)
					}
					if q.Other {
						ui.Checkbox(c.Key("other"), &in.other, i18n.T(c, "question_other"))
					}
				})
			} else if len(q.Options) > 0 {
				ui.RadioGroup(c, func() {
					for i, o := range q.Options {
						ui.Radio(c.Key(i), &in.chosen, i, o)
					}
					if q.Other {
						ui.Radio(c.Key("other"), &in.chosen, other, i18n.T(c, "question_other"))
					}
				}).Gap(12).Label(q.Prompt)
				in.other = in.chosen == other
			}
			if q.Other || len(q.Options) == 0 {
				ui.TextAreaBase(c.Key("answer"), &in.text).Label(i18n.T(c, "interaction_input")).
					Placeholder(i18n.T(c, "interaction_input")).PlaceholderColor(p.MutedForeground).
					Lines(1, 4).FontSize(14).Padding(8).Radius(6).Border(1, p.Border).
					Disabled(len(q.Options) > 0 && !in.other)
			}
		})
		answer := s.answer(q)
		ui.Row(c).Gap(8).Justify(ui.End).Children(func() {
			button := func(key, label string) ui.Element {
				return theme.Ghost(c.Key(key), label, false).Height(28).PaddingX(12).Border(1, p.Border).
					Children(func() { ui.Text(c, label).FontSize(14) })
			}
			if q.Declinable {
				button("decline", i18n.T(c, "question_decline")).OnClick(func() { actions.DeclineQuestion(q.ID) })
			}
			button("cancel", i18n.T(c, "interaction_cancel")).OnClick(func() { actions.CancelQuestion(q.ID) })
			submit := i18n.T(c, "interaction_submit")
			theme.Primary(c.Key("submit"), submit, answer == nil).Height(28).
				Children(func() { ui.Text(c, submit).FontSize(14) }).
				OnClick(func() {
					if a := s.answer(q); a != nil {
						actions.Answer(q.ID, *a)
					}
				})
		})
	})
}

// answer is the draft's answer, nil while it answers nothing.
func (s *State) answer(q *Ask) *Answer {
	in := &s.in
	a := &Answer{}
	if q.Multi {
		for i := range q.Options {
			if b := in.choices[i]; b != nil && *b {
				a.Choices = append(a.Choices, i)
			}
		}
	} else if in.chosen >= 0 && in.chosen < len(q.Options) {
		a.Choices = []int{in.chosen}
	}
	if in.other || len(q.Options) == 0 {
		a.Text = strings.TrimSpace(in.text)
	}
	if len(a.Choices) == 0 && a.Text == "" {
		return nil
	}
	return a
}
