package composer

import (
	"math"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// This file holds the composer's interactions: the approvals and the question
// a run waits on, each a capsule over the composer that opens a popover. The
// view models are the backend's; the State keeps only what the user is typing
// and which popover is open.

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
	// Trust is the SSH host key the call asks to trust, nil for other calls.
	Trust *Trust
	// Loading marks details still being read and Failed a read that failed;
	// Reload reads them again. Allow waits for the details, Deny does not.
	Loading, Failed bool
	Reload          func()
	// Disabled marks a request that cannot be decided now: the host is not
	// connected, its turn is not running, or a decision is on its way. Busy
	// marks the request whose decision is on its way.
	Disabled, Busy bool
	// Again resends a decision whose outcome is unknown, nil without one.
	// Receipt marks such a request the run no longer lists.
	Again   func()
	Receipt bool
}

// Trust is an SSH host key awaiting trust; Changed marks a key that replaces
// the one known for the host.
type Trust struct {
	Host                   string
	Port                   int
	Algorithm, Fingerprint string
	Changed                bool
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

// AskKind is what a question asks for.
type AskKind int

const (
	// AskChoice is a choice of Options (Multi for several), Other for a free
	// answer, or a free answer alone without them.
	AskChoice AskKind = iota
	// AskPlan reviews a plan: continue planning or start coding.
	AskPlan
	// AskForm fills Fields.
	AskForm
	// AskLink opens URL in the system browser.
	AskLink
)

// Ask is a question the model, a plan review or a connector asked.
type Ask struct {
	ID, Prompt string
	Kind       AskKind
	Options    []string
	Multi      bool
	Other      bool
	Fields     []Field
	URL        string
	Declinable bool // a form or a link: Decline shows
	// Disabled marks a question that cannot be answered while the host is
	// not connected. Busy marks an answer on its way; Retry an answer whose
	// outcome is unknown, which Submit sends again.
	Disabled, Busy, Retry bool
}

// FieldKind is the input of a form field.
type FieldKind int

const (
	FieldText FieldKind = iota
	FieldNumber
	FieldBoolean
	FieldChoice
)

// Field is one input of a form. The bounds that are nil do not apply.
type Field struct {
	Name, Title, Description string
	Required                 bool
	Kind                     FieldKind
	// Text: Multiline for free text, a single line for a format.
	Multiline            bool
	MinLength, MaxLength *uint64
	// Number.
	Integer          bool
	Minimum, Maximum *float64
	// Choice: the titles of the options, Multi for several.
	Options            []string
	Multi              bool
	MinItems, MaxItems *uint64
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
// and the text typed. A plan review answers Start to start coding and an
// empty Answer to continue planning; a link answers Opened; a form answers
// Values by field name: a string, a float64, a bool, an option's index or a
// []int of them.
type Answer struct {
	Choices []int
	Text    string
	Start   bool
	Opened  bool
	Values  map[string]any
}

// Actions is what the composer asks of the backend. Outcomes show as
// toasts: a decision or answer the run no longer waits on is a conflict.
type Actions interface {
	// Send submits the draft; Guide joins it to the running reply
	// (Enter while a reply runs, GuideTurn); Queue puts it behind the
	// running reply (Tab). Failures toast; the page puts the message back
	// in the draft.
	Send(m Message)
	Guide(m Message)
	Queue(m Message)
	Stop()
	// The queue: send one now (guiding the running reply while one runs),
	// withdraw its pending guidance, save its edited text, remove it, or
	// move it before another ("" for the end).
	SendQueued(id string)
	WithdrawGuidance(id string)
	SaveQueued(id, text string)
	RemoveQueued(id string)
	MoveQueued(id, before string)
	// Upload sends a file of the draft to the host, moving its Status and
	// Progress on; it retries a failed one. Discard drops it.
	Upload(a *Attachment)
	Discard(a *Attachment)
	// Dictate starts, finishes or cancels voice input, moving d on.
	Dictate(d *Dictation, cmd DictationCommand)
	// Compact compacts the conversation (/compact).
	Compact()
	// CreateWorktree, AddProject and ManageTeams open the flows that
	// make a worktree, add a project and edit the teams.
	CreateWorktree()
	AddProject()
	ManageTeams()
	// Decide answers approval id. The popover has no reason form
	// (AGENTS.md, Removed UI) and passes "".
	Decide(id string, d Decision, reason string)
	Answer(id string, a Answer)
	CancelQuestion(id string)
	DeclineQuestion(id string)
	// The activity strip: OpenChange shows a changed file's diff,
	// Badge presses a plugin badge (button "") or a button of its panel,
	// StopCommand stops a background command, OpenService forwards a
	// command's port and opens it, OpenChild opens a subagent's session.
	OpenChange(path string)
	Badge(id, button string)
	StopCommand(id string)
	OpenService(port int)
	OpenChild(id string)
}

// interaction is the State's part for the interactions.
type interaction struct {
	approvals, question bool // the popovers are open
	at                  int  // the approval shown
	// offered are the approvals the popover has opened for, or first showed
	// with: a request that arrives opens it once, and closing it keeps it
	// closed.
	offered map[string]bool
	asked   string // the question the draft is for
	chosen  int
	choices map[int]*bool
	other   bool
	text    string
	fields  []fieldDraft
}

// fieldDraft is what the user entered in a form field. An optional field
// answers only while it is included.
type fieldDraft struct {
	include bool
	text    string
	chosen  int
	choices map[int]*bool
}

// capsule is an interaction's capsule: the person icon and its label.
func capsule(c *ui.Context, label string, open bool) ui.Element {
	return chip(c, label, open, func() {
		ui.Icon(c, icons.CircleUser).FontSize(14)
		chipText(c, label)
	}).HasPopup(ui.PopupDialog)
}

// chip is a small round outlined button, muted, named label, holding
// children; the strip's capsules are all chips.
func chip(c *ui.Context, label string, open bool, children func()) ui.Element {
	p := theme.Of(c)
	b := kit.Ghost(c, label, open).Height(28).PaddingX(12).Gap(6).Radius(14).Border(1, p.Border).
		TextColor(p.MutedForeground).Shrink(0)
	return b.Expanded(open).Children(children)
}

// chipText is a chip's text, 14 DIPs on one line, level with the chip's
// icon (kit.Level).
func chipText(c *ui.Context, text string) ui.Element {
	return kit.Level(c, ui.Text(c, text).FontSize(14).SingleLine(), 14)
}

// place puts a capsule's popover (MyGo's, as it is) anchored at its bottom
// right 8 DIPs over the capsule (Anchor::BottomRight, bottom_2): Kit's
// popover padding (p_3) around a 320 DIP body (w_80), its parts gap apart,
// in the popover color. It does nothing while the popover is closed.
func place(c *ui.Context, panel, anchor ui.Element, gap float32) {
	if panel != (ui.Element{}) {
		panel.AttachTo(anchor, ui.AnchorTopEnd, ui.AnchorBottomEnd).Margin(0, 0, 8, 0).
			Padding(12).Width(320 + 2*12).Gap(gap)
	}
}

// offer opens a request's popover when the request arrives while the
// composer shows, an approval's or the question's, so a run does not wait
// behind a capsule nobody pressed; the other popover closes, so one shows
// at a time and no draft is lost. The requests a composer first shows with
// stay behind their capsule: a popover opening by itself over a window just
// opened, or a dialog, would take the first click meant for something else.
// It runs every frame, also with no request, so the first one to arrive is
// known as new.
func (s *State) offer(list []Approval, question *Ask) {
	first := s.in.offered == nil
	if first {
		s.in.offered = map[string]bool{}
	}
	for i, a := range list {
		if a.Receipt || s.in.offered[a.ID] {
			continue
		}
		// Once for each request: one that leaves the list and returns, as
		// while its host reconnects, does not open the popover again.
		s.in.offered[a.ID] = true
		if !first {
			s.in.approvals, s.in.question, s.in.at = true, false, i
		}
	}
	// A question's ID is kept apart from the approvals' by its prefix.
	if question != nil && !s.in.offered["question/"+question.ID] {
		s.in.offered["question/"+question.ID] = true
		if !first {
			s.in.question, s.in.approvals = true, false
		}
	}
}

// approvals shows one pending request at a time; with several, a "1/2"
// position and previous and next buttons page through them. With only
// receipts left the capsule says so.
func (s *State) approvals(c *ui.Context, list []Approval, actions Actions) {
	p := theme.Of(c)
	pending := 0
	for _, a := range list {
		if !a.Receipt {
			pending++
		}
	}
	label := i18n.T(c, "approval_receipt")
	if pending > 0 {
		label = i18n.T(c, "approval_live_count", "count", pending)
	}
	trigger := capsule(c.Key("approvals"), label, s.in.approvals).OnClick(func() { s.in.approvals = !s.in.approvals })
	s.in.at = min(max(s.in.at, 0), len(list)-1)
	place(c, kit.Popover(c, trigger, &s.in.approvals, func() {
		if len(list) > 1 {
			ui.Row(c.Key("pager")).Gap(4).Children(func() {
				ui.Textf(c, "%d/%d", s.in.at+1, len(list)).FontSize(14).TextColor(p.MutedForeground)
				ui.Spacer(c)
				kit.IconButton(c.Key("previous"), icons.ChevronLeft, i18n.T(c, "knowledge_previous"), 24, false).
					Disabled(s.in.at == 0).OnClick(func() { s.in.at-- })
				kit.IconButton(c.Key("next"), icons.ChevronRight, i18n.T(c, "knowledge_next"), 24, false).
					Disabled(s.in.at == len(list)-1).OnClick(func() { s.in.at++ })
			})
		}
		a := list[s.in.at]
		ui.Column(c.Key(a.ID)).Gap(8).Label(a.Title).Children(func() { s.approval(c, a, actions) })
	}), trigger, 12)
}

// approval is one request: its title (who asks, the tool, where, on one
// line), the request in a card (the host key, the command, the text or the
// change set), the notices, and Deny and Allow, Allow with a menu of the
// wider grants. Deny takes no reason (AGENTS.md, Removed UI). Details still
// being read show a spinner, a failed read Retry.
func (s *State) approval(c *ui.Context, a Approval, actions Actions) {
	p := theme.Of(c)
	ui.Row(c.Key("title")).Gap(8).AlignItems(ui.Center).Children(func() {
		title := a.Title
		if a.Child != "" {
			title = a.Child + " · " + title
		}
		// The title keeps its width beside a context and gives way alone.
		shown := ui.Text(c, title).FontSize(14).FontWeight(500).SingleLine().Shrink(0)
		if a.Context == "" {
			shown.Shrink(1).MinWidth(0)
		}
		if a.Context != "" {
			ui.Text(c, a.Context).FontSize(13).SingleLine().Shrink(1).MinWidth(0).TextColor(p.MutedForeground)
		}
	})
	description := a.Description
	if t := a.Trust; t != nil {
		key := "approval_ssh_trust"
		if t.Changed {
			key = "approval_ssh_replace"
		}
		description = i18n.T(c, key, "host", t.Host, "port", t.Port, "algorithm", t.Algorithm, "fingerprint", t.Fingerprint)
	}
	if len(a.Changes) > 0 || description != "" {
		ui.Scroll(c.Key("card")).MaxHeight(160).Shrink(0).Radius(p.Radius).Background(p.Foreground.Alpha(0.05)).Padding(12).
			Children(func() {
				switch {
				case len(a.Changes) > 0:
					changes(c, a.Changes)
				case a.Code && a.Trust == nil:
					ui.Text(c, description).Font("monospace").FontSize(13).Selectable()
				default:
					ui.Text(c, description).FontSize(14).Selectable()
				}
			})
	}
	for _, n := range a.Notices {
		ui.Text(c, n).FontSize(13).MaxLines(3).TextColor(p.MutedForeground)
	}
	retry := i18n.T(c, "chat_retry")
	switch {
	case a.Failed:
		ui.Row(c.Key("details")).Children(func() {
			outlined(c.Key("reload"), retry).Disabled(a.Reload == nil).OnClick(func() { a.Reload() })
		})
	case a.Loading:
		ui.Row(c.Key("details")).Height(28).AlignItems(ui.Center).Children(func() { kit.Busy(c) })
	}
	if a.Receipt || a.Again != nil {
		ui.Row(c.Key("decision")).Gap(8).Padding(4, 0, 0, 0).Justify(ui.End).AlignItems(ui.Center).Children(func() {
			kit.Primary(c.Key("again"), retry, a.Again == nil || a.Busy).Height(28).Radius(14).MinWidth(48).
				Children(func() { busyLabel(c, retry, a.Busy) }).
				OnClick(func() { a.Again() })
		})
		return
	}
	locked := a.Disabled || a.Busy
	ui.Row(c.Key("decision")).Gap(8).Padding(4, 0, 0, 0).Justify(ui.End).AlignItems(ui.Center).Children(func() {
		if a.Busy {
			kit.Busy(c)
		}
		deny := i18n.T(c, "approval_reject")
		// The decision's buttons are one family: pills 28 tall, Deny and the
		// options outlined, Allow filled.
		kit.Ghost(c.Key("deny"), deny, false).Height(28).PaddingX(12).MinWidth(48).Radius(14).Border(1, theme.Of(c).Border).Disabled(locked).
			Children(func() { ui.Text(c, deny).FontSize(14) }).
			OnClick(func() { actions.Decide(a.ID, Deny, "") })
		ui.Row(c.Key("allow-group")).Gap(4).AlignItems(ui.Center).Children(func() {
			allow := i18n.T(c, "approval_allow_once")
			b := kit.Primary(c.Key("allow"), allow, locked || a.Loading || a.Failed).Height(28).Radius(14)
			if a.Unrestricted {
				kit.Tooltip(c, b, i18n.T(c, "approval_unrestricted"))
			}
			b.MinWidth(48).Children(func() { ui.Text(c, allow).FontSize(14) }).OnClick(func() { actions.Decide(a.ID, Allow, "") })
			if a.Remember {
				kit.MenuButton(c.Key("allow-options"), "allow-"+a.ID, func(b ui.Element) {
					kit.IconLook(c, b, icons.ChevronDown, i18n.T(c, "approval_allow_session"), "", 28, false).
						Radius(14).Border(1, theme.Of(c).Border).Disabled(locked || a.Loading || a.Failed)
				}, func(m *kit.Menu) {
					if m.Item(i18n.T(c, "approval_allow_session")).Chosen() {
						actions.Decide(a.ID, AllowSession, "")
					}
					if a.Prefix != "" && m.Item(i18n.T(c, "approval_allow_prefix", "prefix", a.Prefix)).Chosen() {
						actions.Decide(a.ID, AllowPrefix, "")
					}
				})
			}
		})
	})
}

// outlined is a popover's secondary button.
func outlined(c *ui.Context, label string) ui.Element {
	p := theme.Of(c)
	return kit.Ghost(c, label, false).Height(28).PaddingX(12).MinWidth(48).Radius(14).Border(1, p.Border).
		Children(func() { ui.Text(c, label).FontSize(14) })
}

// busyLabel is a button's label, behind a spinner while its action runs.
func busyLabel(c *ui.Context, label string, busy bool) {
	if busy {
		kit.Busy(c)
	}
	ui.Text(c, label).FontSize(14)
}

// changesShown is how many of a change set's files the approval lists.
const changesShown = 5

// changes is a change set's files, a path a line: the first changesShown,
// then an ellipsis for the rest. What each change does and its counts are
// the diff's to show.
func changes(c *ui.Context, list []Change) {
	p := theme.Of(c)
	ui.Column(c).Gap(6).Children(func() {
		for i, f := range list[:min(len(list), changesShown)] {
			path := f.Path
			if f.Destination != "" {
				path += " → " + f.Destination
			}
			ui.Text(c.Key(i), path).Font("monospace").FontSize(13).SingleLine().TruncateMiddle().FillWidth().MinWidth(0)
		}
		if len(list) > changesShown {
			ui.Text(c.Key("more"), "…").FontSize(13).TextColor(p.MutedForeground)
		}
	})
}

// ask is the pending question's capsule and its popover: the prompt, the
// inputs of its kind, and Decline, Cancel and Submit. A plan review offers
// Continue and Start; a link opens its page. An answer on its way locks the
// inputs and shows a spinner; one whose outcome is unknown is sent again by
// Retry, the draft kept.
func (s *State) ask(c *ui.Context, q *Ask, actions Actions) {
	p := theme.Of(c)
	in := &s.in
	if in.asked != q.ID { // a new question: a new draft
		in.asked, in.chosen, in.choices, in.other, in.text = q.ID, -1, map[int]*bool{}, false, ""
		in.fields = make([]fieldDraft, len(q.Fields))
		for i, f := range q.Fields {
			in.fields[i] = fieldDraft{include: f.Required, chosen: -1, choices: map[int]*bool{}}
		}
	}
	locked := q.Disabled || q.Busy || q.Retry
	label := i18n.T(c, "question_reply")
	trigger := capsule(c.Key("question"), label, in.question).OnClick(func() { in.question = !in.question })
	place(c, kit.Popover(c, trigger, &in.question, func() {
		_, height := c.Size()
		ui.Scroll(c).Gap(16).MaxHeight(max(min(height/2, 420), 160)).Shrink(1).Children(func() {
			prompt := q.Prompt
			if q.Kind == AskPlan {
				prompt = i18n.T(c, "plan_execute_confirm")
			}
			ui.Text(c, prompt).FontSize(14).FontWeight(500)
			// MyGo fills an unchecked box or radio with the theme's
			// Background, the window's darkest color; on the panel they
			// take the panel's surface instead.
			t := *c.Theme()
			t.Background = ui.Transparent
			switch q.Kind {
			case AskPlan:
			case AskLink:
				ui.Text(c, q.URL).FontSize(13).MaxLines(2).TextColor(p.MutedForeground).Selectable()
			case AskForm:
				ui.Themed(c, &t).Background(ui.Transparent).Children(func() {
					ui.Column(c.Key("fields")).Gap(16).Children(func() {
						for i := range q.Fields {
							ui.Column(c.Key(i)).Gap(8).Children(func() { field(c, q.Fields[i], &in.fields[i], locked) })
						}
					})
				})
			default:
				other := len(q.Options) // the index of Other
				ui.Themed(c, &t).Background(ui.Transparent).Children(func() {
					if q.Multi {
						ui.Column(c).Gap(12).Children(func() {
							for i, o := range q.Options {
								b, ok := in.choices[i]
								if !ok {
									b = new(bool)
									in.choices[i] = b
								}
								ui.Checkbox(c.Key(i), b, o).Disabled(locked)
							}
							if q.Other {
								ui.Checkbox(c.Key("other"), &in.other, i18n.T(c, "question_other")).Disabled(locked)
							}
						})
					} else if len(q.Options) > 0 {
						ui.RadioGroup(c, func() {
							for i, o := range q.Options {
								ui.Radio(c.Key(i), &in.chosen, i, o).Disabled(locked)
							}
							if q.Other {
								ui.Radio(c.Key("other"), &in.chosen, other, i18n.T(c, "question_other")).Disabled(locked)
							}
						}).Gap(12).Label(q.Prompt)
						in.other = in.chosen == other
					}
				})
				if q.Other || len(q.Options) == 0 {
					ui.TextAreaBase(c.Key("answer"), &in.text).Label(i18n.T(c, "interaction_input")).
						Placeholder(i18n.T(c, "interaction_input")).PlaceholderColor(p.MutedForeground).
						Lines(1, 4).FontSize(14).Padding(8).Radius(p.Radius).Border(1, p.Border).
						Disabled(locked || len(q.Options) > 0 && !in.other)
				}
			}
		})
		answer := s.answer(q)
		ui.Row(c).Gap(8).Justify(ui.End).Children(func() {
			held := q.Disabled || q.Busy
			if q.Declinable {
				outlined(c.Key("decline"), i18n.T(c, "question_decline")).Disabled(held).
					OnClick(func() { actions.DeclineQuestion(q.ID) })
			}
			if q.Kind == AskPlan {
				outlined(c.Key("continue"), i18n.T(c, "interaction_continue")).Disabled(held).
					OnClick(func() { actions.Answer(q.ID, Answer{}) })
			} else {
				outlined(c.Key("cancel"), i18n.T(c, "interaction_cancel")).Disabled(held).
					OnClick(func() { actions.CancelQuestion(q.ID) })
			}
			submit := i18n.T(c, "interaction_submit")
			switch {
			case q.Retry:
				submit = i18n.T(c, "chat_retry")
			case q.Kind == AskPlan:
				submit = i18n.T(c, "interaction_start")
			case q.Kind == AskLink:
				submit = i18n.T(c, "question_open_url")
			}
			kit.Primary(c.Key("submit"), submit, held || answer == nil).Height(28).Radius(14).
				Children(func() { busyLabel(c, submit, q.Busy) }).
				OnClick(func() {
					if a := s.answer(q); a != nil {
						actions.Answer(q.ID, *a)
					}
				})
		})
	}), trigger, 16)
}

// field is one form field: its title, marked when required and a checkbox
// that includes it when optional, its description, and the input of its
// kind.
func field(c *ui.Context, f Field, d *fieldDraft, locked bool) {
	p := theme.Of(c)
	if f.Required {
		ui.Text(c, f.Title+" *").FontSize(14).FontWeight(500)
	} else {
		ui.Checkbox(c.Key("include"), &d.include, i18n.T(c, "question_optional", "field", f.Title)).Disabled(locked)
	}
	if f.Description != "" {
		ui.Text(c, f.Description).FontSize(13).TextColor(p.MutedForeground)
	}
	off := locked || !d.include
	switch f.Kind {
	case FieldText, FieldNumber:
		if f.Kind == FieldText && f.Multiline {
			ui.TextAreaBase(c.Key("input"), &d.text).Label(f.Title).Lines(1, 4).FontSize(14).Padding(8).
				Radius(p.Radius).Border(1, p.Border).Disabled(off)
		} else {
			ui.TextInput(c.Key("input"), &d.text).Label(f.Title).FillWidth().Disabled(off)
		}
	case FieldBoolean:
		ui.RadioGroup(c, func() {
			ui.Radio(c.Key(0), &d.chosen, 0, i18n.T(c, "question_yes")).Disabled(off)
			ui.Radio(c.Key(1), &d.chosen, 1, i18n.T(c, "question_no")).Disabled(off)
		}).Gap(12).Label(f.Title)
	case FieldChoice:
		if f.Multi {
			ui.Column(c).Gap(12).Children(func() {
				for i, o := range f.Options {
					b, ok := d.choices[i]
					if !ok {
						b = new(bool)
						d.choices[i] = b
					}
					ui.Checkbox(c.Key(i), b, o).Disabled(off)
				}
			})
		} else {
			ui.RadioGroup(c, func() {
				for i, o := range f.Options {
					ui.Radio(c.Key(i), &d.chosen, i, o).Disabled(off)
				}
			}).Gap(12).Label(f.Title)
		}
	}
}

// value is the field's answer, false while what was entered does not fit
// the field.
func (f Field) value(d *fieldDraft) (any, bool) {
	within := func(n int, low, high *uint64) bool {
		return (low == nil || uint64(n) >= *low) && (high == nil || uint64(n) <= *high)
	}
	switch f.Kind {
	case FieldText:
		return d.text, within(utf8.RuneCountInString(d.text), f.MinLength, f.MaxLength)
	case FieldNumber:
		n, err := strconv.ParseFloat(strings.TrimSpace(d.text), 64)
		ok := err == nil && !math.IsInf(n, 0) && !math.IsNaN(n) && (!f.Integer || n == math.Trunc(n)) &&
			(f.Minimum == nil || n >= *f.Minimum) && (f.Maximum == nil || n <= *f.Maximum)
		return n, ok
	case FieldBoolean:
		return d.chosen == 0, d.chosen == 0 || d.chosen == 1
	}
	if !f.Multi {
		return d.chosen, d.chosen >= 0 && d.chosen < len(f.Options)
	}
	chosen := []int{}
	for i := range f.Options {
		if b := d.choices[i]; b != nil && *b {
			chosen = append(chosen, i)
		}
	}
	return chosen, within(len(chosen), f.MinItems, f.MaxItems)
}

// answer is the draft's answer, nil while it answers nothing or a form
// field does not fit.
func (s *State) answer(q *Ask) *Answer {
	in := &s.in
	a := &Answer{}
	switch q.Kind {
	case AskPlan:
		a.Start = true
		return a
	case AskLink:
		a.Opened = true
		return a
	case AskForm:
		a.Values = map[string]any{}
		for i, f := range q.Fields {
			if i >= len(in.fields) {
				return nil
			}
			if !in.fields[i].include {
				continue
			}
			v, ok := f.value(&in.fields[i])
			if !ok {
				return nil
			}
			a.Values[f.Name] = v
		}
		return a
	}
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
