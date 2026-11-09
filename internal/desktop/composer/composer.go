// Package composer is the message composer under the transcript: the queue
// strip, the attachment chips, the draft with its inline reference chips and
// the @ and / pickers, the toolbar (attach, mode, permission, the model chip
// and its controls, dictation, send and stop), and the context bar (host,
// project and worktree, usage).
//
// Custom drawing, each for a verified MyGo gap: the model controls' effort
// track (track.go: MyGo's StepSlider has no gradient fill or step dots),
// the dictation level meter (four bars of live levels), and the inline
// reference chips, which are MyGo TextRanges (a text area draws no inline
// elements; see docs/composer.md).
package composer

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Efforts are the default reasoning levels the picker steps through, for a
// model that lists none. The reference shows their raw names, untranslated.
var Efforts = []string{"minimal", "low", "medium", "high", "xhigh"}

// Width is the composer's maximum width, the transcript's too.
const Width = theme.ContentWidth

// Widths under which the toolbar folds: below compactWidth mode, permission
// and the model go into the settings popover; below iconWidth the context bar
// shows icons alone.
const (
	compactWidth = 680
	iconWidth    = 440
)

// State is the composer's part of the app state: the draft and the
// choices the user makes for it, kept per conversation, and which of its
// popovers show.
type State struct {
	Draft   string
	Model   string  // the chosen Model's ID
	Effort  float64 // index into the chosen model's efforts
	Running bool    // a reply is running: Stop replaces Send
	// Searching is set by the page while its search bar shows: Escape
	// closes the bar before it reaches the composer.
	Searching bool
	// Metrics builds the context bar's usage at its end (conversation
	// UsageBar), nil for none: compact below compactWidth, its popover
	// open while *open is true.
	Metrics func(c *ui.Context, compact bool, open *bool)
	// Indicator builds the context ring left of the model chip
	// (conversation ContextRing), nil for none.
	Indicator func(c *ui.Context)

	Mode       Mode
	Permission Permission
	Features   map[string]string // feature ID → option ID
	Team       string            // "" runs the model alone
	// Host, Project and Worktree locate the draft (IDs; Worktree a branch).
	Host, Project, Worktree string
	Attachments             []*Attachment
	References              []Reference // chipped into the draft
	Dictation               Dictation

	view     kit.Viewer[*Attachment] // the image viewer over the draft's pictures
	picker   bool                    // the model controls are open
	layer    layer
	member   string // the team member whose model the controls set
	family   string // the picker's rail choice, "" for all
	open     map[string]bool
	efforts  string // the model whose efforts submenu shows
	settings bool   // the compact settings popover
	stats    bool   // the compact usage popover
	page     settingsPage
	width    float32 // the toolbar's width last frame

	in    interaction
	act   activity
	queue queueState
	ref   refState
	files fileState
	caret int // a caret to put in the draft next frame, -1 for none
	focus bool
	area  ui.Element
}

func New(model string) *State {
	return &State{Model: model, Effort: 2, caret: -1, Features: map[string]string{}}
}

// Chosen returns the model chosen, the first one when the ID is unknown.
func (s *State) Chosen(models []Model) Model {
	for _, m := range models {
		if m.ID == s.Model {
			return m
		}
	}
	if len(models) == 0 {
		return Model{}
	}
	return models[0]
}

// EffortName is the effort shown for the chosen model.
func (s *State) EffortName(c *ui.Context, m Model) string {
	list := efforts(m)
	if len(list) == 0 {
		return i18n.T(c, "composer_effort_unavailable")
	}
	return list[min(max(int(s.Effort), 0), len(list)-1)]
}

// efforts are the levels a model takes, none without reasoning.
func efforts(m Model) []string {
	switch {
	case !m.Reasoning:
		return nil
	case len(m.Efforts) > 0:
		return m.Efforts
	}
	return Efforts
}

// message is the draft as Send and Queue take it.
func (s *State) message(ctx *Context) Message {
	m := s.Chosen(ctx.Models)
	effort := ""
	if list := efforts(m); len(list) > 0 {
		effort = s.EffortName(nil, m)
	}
	return Message{Text: strings.TrimSpace(s.Draft), Attachments: s.Attachments, References: s.active(),
		Mode: s.Mode, Permission: s.Permission, Model: s.Model, Effort: effort, Features: s.Features, Team: s.Team}
}

// clear empties the draft once it was sent or queued.
func (s *State) clear() {
	s.Draft, s.Attachments, s.References = "", nil, nil
	s.ref = refState{}
}

// hasInput reports whether the draft has anything to send: text, a file or a
// reference.
func (s *State) hasInput() bool {
	return strings.TrimSpace(s.Draft) != "" || len(s.Attachments) > 0 || len(s.active()) > 0
}

// sendable reports whether the files are all on the host.
func (s *State) sendable() bool {
	for _, a := range s.Attachments {
		if a.Status != Ready {
			return false
		}
	}
	return true
}

// View builds the composer: the capsules of what the run waits on, the
// queue strip, the input surface with its chips, draft and toolbar, and
// the context bar. The page builds it under the transcript.
func View(c *ui.Context, s *State, ctx *Context, actions Actions) {
	// submit sends the draft, or guides the running reply with it
	// (Delivery::enter).
	submit := func() {
		if s.hasInput() && s.sendable() && !ctx.Readonly {
			msg := s.message(ctx)
			s.clear()
			if s.Running {
				actions.Guide(msg)
			} else {
				actions.Send(msg)
			}
		}
	}
	s.syncReferences()
	ui.Column(c.Key("composer")).FillWidth().MaxWidth(Width).Margin(0, ui.Auto).Padding(8, 24, 16, 24).Gap(8).Children(func() {
		s.activity(c, ctx, actions)
		// the queue tray and the reference picker over the surface, the context bar
		// under it.
		ui.Column(c.Key("stack")).FillWidth().Shrink(0).Children(func() {
			s.queueTray(c, ctx, actions)
			surface := s.surface(c, ctx, actions, submit)
			s.referencePicker(c, ctx, actions, surface)
			s.contextBar(c, ctx, actions)
		})
	})
	s.filePicker(c, ctx, actions)
}

// surface is padding 12, radius_2xl, muted, the package's border width in the
// input border color and its corner art; files dropped on it attach, tinting
// it while they are over it (drop_target).
func (s *State) surface(c *ui.Context, ctx *Context, actions Actions, submit func()) ui.Element {
	p := theme.Of(c)
	surface := kit.Frost(c, ui.Column(c.Key("surface")).FillWidth().Padding(12).Gap(8).Radius(p.Radius2xl()), p.Muted, kit.SurfaceTint)
	if v := p.Variant; v != nil {
		if w := v.Composer.BorderWidth; w > 0 {
			surface.Border(w, p.Input)
		}
		if v.Asset("composer.top_left") != nil || v.Asset("composer.bottom_right") != nil {
			surface.Draw(func(pt *ui.Painter, r ui.Rect) { kit.Corners(pt, r, v, "composer") })
		}
	}
	if !ctx.Readonly {
		if surface.FileDragOver() {
			surface.Background(p.Muted.Mix(p.Primary, 0.12))
		}
		if paths := surface.DroppedFiles(); len(paths) > 0 {
			s.attachPaths(c, paths, actions)
		}
	}
	if b := surface.LayoutBounds(); b.W > 0 {
		s.width = b.W - 24
	}
	surface.Children(func() {
		s.attachments(c, actions)
		s.input(c, ctx, actions, submit)
		s.toolbar(c, ctx, actions, submit)
	})
	return surface
}

// input is the draft: Enter sends, or guides the running reply, and in an
// empty draft sends the first queued message (promote_queued); Shift+Enter
// breaks the line, Tab queues it while a reply runs, Up puts the last message
// back in an empty draft or moves the picker, and the picker takes Down, Tab,
// Enter and Escape while it shows.
func (s *State) input(c *ui.Context, ctx *Context, actions Actions, submit func()) {
	p := theme.Of(c)
	area := ui.TextAreaBase(c.Key("draft"), &s.Draft).Label(i18n.T(c, "composer")).Placeholder(i18n.T(c, "composer")).
		PlaceholderColor(p.MutedForeground).Lines(2, 6).FontSize(14).LineHeight(1.5).TextColor(p.Foreground).
		ReadOnly(ctx.Readonly)
	s.area = area
	if s.caret >= 0 {
		area.SetTextSelection(s.caret, s.caret)
		s.caret = -1
	}
	if s.focus {
		area.Focus()
		s.focus = false
	}
	s.chips(c, area)
	area.OnPaste(func(text string) bool { return text == "" && s.paste(c, actions) })
	// The element is this pass's: the callback runs later, so it reads
	// what the pass saw.
	composing := area.Composing()
	selStart, selEnd := area.TextSelection()
	area.OnTextKey(func(k ui.TextKey) bool {
		if composing {
			return false
		}
		// The key may come before the frame that takes what was typed.
		s.Draft = k.Text()
		if s.ref.shown && s.pickerKey(c, ctx, actions, k) {
			return true
		}
		switch {
		case k.Key == ui.KeyEnter && k.Mods == 0:
			switch {
			case !s.hasInput():
				if id := promotable(ctx); id != "" {
					actions.SendQueued(id)
				}
			case s.sendable() && !ctx.Readonly:
				submit()
				k.SetText("")
			}
			return true
		case k.Key == ui.KeyTab && k.Mods == 0:
			// Delivery::Queue: Tab sends the draft to the queue.
			if !s.hasInput() || ctx.Readonly {
				return false
			}
			msg := s.message(ctx)
			k.SetText("")
			s.clear()
			actions.Queue(msg)
			return true
		case k.Key == ui.KeyUp && k.Mods == 0:
			// recall_message: an empty draft takes the last message.
			if k.Text() != "" || ctx.Last == "" || ctx.Readonly {
				return false
			}
			k.SetText(ctx.Last)
			s.caret = len([]rune(ctx.Last))
			return true
		case k.Key == ui.KeyTab && k.Mods == ui.Shift:
			// CycleMode (OutdentInline): the next mode.
			s.Mode = (s.Mode + 1) % Mode(len(Modes))
			return true
		case k.Key == ui.KeyBackspace && k.Mods == 0:
			return s.deleteChip(selStart, selEnd, k)
		}
		return false
	})
}

// toolbar is attach, then the settings popover when compact, mode and
// permission; at the end the context indicator, the model chip, dictation,
// and send or stop (hidden while dictating, unless a reply runs).
func (s *State) toolbar(c *ui.Context, ctx *Context, actions Actions, submit func()) {
	compact := s.width > 0 && s.width < compactWidth
	ui.Row(c.Key("toolbar")).FillWidth().MinWidth(0).MinHeight(32).Gap(4).Wrap().AlignItems(ui.Center).Children(func() {
		ui.Row(c.Key("start")).Gap(4).Wrap().AlignItems(ui.Center).Children(func() {
			kit.IconButton(c.Key("attach"), icons.Plus, i18n.T(c, "composer_attachment"), 32, false).Radius(16).
				Disabled(ctx.Readonly).OnClick(func() { s.openFiles(ctx) })
			if compact {
				s.settingsButton(c, ctx, actions)
			} else {
				s.modeMenu(c, ctx)
				s.permissionMenu(c, ctx)
			}
		})
		ui.Spacer(c)
		ui.Row(c.Key("end")).Gap(4).AlignItems(ui.Center).MinWidth(0).Shrink(1).Children(func() {
			if s.Indicator != nil {
				s.Indicator(c)
			}
			if !compact {
				s.modelChip(c, ctx, actions)
			}
			s.dictation(c, ctx, actions)
			if s.Running || s.Dictation.Phase == Idle {
				s.sendButton(c, ctx, submit, actions.Stop)
			}
		})
	})
}

func (s *State) sendButton(c *ui.Context, ctx *Context, submit, stop func()) {
	v := theme.Of(c).Variant
	if s.Running {
		b := kit.Primary(c.Key("stop"), i18n.T(c, "chat_stop"), false).Size(32, 32).PaddingX(0).Radius(16)
		if v != nil {
			action(b, v.Composer.Stop, false)
		}
		kit.Tooltip(c, b, i18n.T(c, "chat_stop"))
		b.Children(func() { ui.Icon(c, icons.Stop).FontSize(16) }).OnClick(stop)
		return
	}
	blocked := !s.hasInput() || !s.sendable() || ctx.Readonly
	b := kit.Primary(c.Key("send"), i18n.T(c, "send"), blocked).Size(32, 32).PaddingX(0).Radius(16)
	if v != nil {
		action(b, v.Composer.Send, blocked)
	}
	kit.Tooltip(c, b, i18n.T(c, "send"))
	b.Children(func() { ui.Icon(c, icons.ArrowUp).FontSize(16) }).OnClick(submit)
}

// action paints send or stop in the package's colors; disabled, the button
// keeps Kit's look.
func action(b ui.Element, a *theme.Action, disabled bool) {
	if a == nil || disabled {
		return
	}
	bg := a.Background.Or(ui.Transparent)
	switch {
	case b.Pressed():
		bg = a.Active.Or(bg)
	case b.Hovered():
		bg = a.Hover.Or(bg)
	}
	b.Background(bg)
	if a.Foreground.OK {
		b.TextColor(a.Foreground.V)
	}
}

// contextBar is under the surface: the host, the project and the worktree,
// then the usage at its end. Wide, both wrap; compact, they stay on one line
// and the usage is one button.
func (s *State) contextBar(c *ui.Context, ctx *Context, actions Actions) {
	p := theme.Of(c)
	compact := s.width > 0 && s.width < compactWidth
	icon := s.width > 0 && s.width < iconWidth
	bar := ui.Row(c.Key("context")).MinHeight(32).MinWidth(0).MarginX(12).Padding(4, 8).Gap(4).TextColor(p.MutedForeground).
		AlignItems(ui.Center)
	if !compact {
		bar.Wrap()
	}
	bar.Children(func() {
		location := ui.Row(c.Key("location")).Grow(1).MinWidth(0).Gap(4).AlignItems(ui.Center)
		if !compact {
			location.Wrap()
		}
		location.Children(func() {
			s.projectButton(c, ctx, actions, icon)
			s.worktreeMenu(c, ctx, actions, icon)
		})
		if s.Metrics != nil {
			s.Metrics(c, compact, &s.stats)
		}
	})
}

// Escape is the conversation's Escape after the picker (the draft's own key)
// and the search bar (an overlay): it closes the queue's editor, or stops the
// reply. It reports whether it did either; the page calls it while the focus
// is in the conversation.
func (s *State) Escape(actions Actions) bool {
	switch {
	case s.Searching:
		return false
	case s.queue.editing != "":
		s.queue.editing = ""
		return true
	case s.Running:
		actions.Stop()
		return true
	}
	return false
}

// Escapes reports whether Escape would do something now.
func (s *State) Escapes() bool {
	// The picker takes Escape in the draft; a window shortcut claiming it
	// would come first.
	return !s.Searching && !s.ref.shown && (s.queue.editing != "" || s.Running)
}
