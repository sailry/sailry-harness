package composer

import (
	"math"
	"slices"
	"strings"
	"unicode"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The @ and / pickers and the inline chips: typing @ or / after a space opens
// a picker over the surface listing the page's rows the text after it
// matches; Up and Down move, Enter or Tab chooses, Escape closes it until the
// trigger changes, Backspace on an empty query goes back a page. A chosen
// reference becomes "@label" in the draft, drawn as a chip (the emphasis
// color, medium weight) and deleted whole by Backspace; commands put their
// prompt in the draft or choose the mode, the model or the effort.

// refState is the pickers' state.
type refState struct {
	shown     bool
	at        trigger
	dismissed trigger
	path      []Reference // the pages entered, innermost last
	sel       int
	// pointer is where the pointer was last seen over a row, in window
	// DIPs, once pointed: a row takes the selection as the pointer moves,
	// not while it rests where the picker opened or the rows scroll under it.
	pointer [2]int
	pointed bool
}

// trigger is the @ or / being typed: where its marker is in the draft (in
// runes), the marker, and the text after it up to the caret.
type trigger struct {
	start  int
	marker rune
	query  string
	ok     bool
}

// parseTrigger is the marker before the caret starts the draft or follows a
// space; the query after it holds no space or @, and the caret ends a word.
func parseTrigger(text string, caret int) trigger {
	rs := []rune(text)
	if caret < 0 || caret > len(rs) {
		return trigger{}
	}
	for i := caret - 1; i >= 0; i-- {
		r := rs[i]
		if r != '@' && r != '/' {
			if unicode.IsSpace(r) || (r == '@') {
				return trigger{}
			}
			continue
		}
		if i > 0 && !unicode.IsSpace(rs[i-1]) {
			return trigger{}
		}
		if caret < len(rs) && !unicode.IsSpace(rs[caret]) {
			return trigger{}
		}
		query := string(rs[i+1 : caret])
		if r == '/' && strings.Contains(query, "/") {
			return trigger{}
		}
		return trigger{start: i, marker: r, query: query, ok: true}
	}
	return trigger{}
}

// token is a reference's text in the draft.
func token(r Reference) string { return "@" + strings.TrimPrefix(r.Label, "@") }

// active are the references whose chip is still in the draft.
func (s *State) active() []Reference {
	var out []Reference
	for _, r := range s.References {
		if strings.Contains(s.Draft, token(r)) {
			out = append(out, r)
		}
	}
	return out
}

// syncReferences forgets the references whose chip was deleted.
func (s *State) syncReferences() {
	if len(s.References) > 0 {
		s.References = s.active()
	}
}

// chips styles every reference's token in the draft as a chip: Kit's
// inline token, as MyGo TextRanges (the text area draws no inline
// elements).
func (s *State) chips(c *ui.Context, area ui.Element) {
	if len(s.References) == 0 {
		return
	}
	p := theme.Of(c)
	rs := []rune(s.Draft)
	for _, r := range s.References {
		t := []rune(token(r))
		for i := 0; i+len(t) <= len(rs); i++ {
			if slices.Equal(rs[i:i+len(t)], t) {
				area.TextRanges(ui.TextRange{Start: i, End: i + len(t), Color: p.Primary, Weight: 500})
				i += len(t) - 1
			}
		}
	}
}

// deleteChip deletes a whole chip with Backspace at its end, as an inline
// token is one character.
func (s *State) deleteChip(start, end int, k ui.TextKey) bool {
	if start != end || len(s.References) == 0 {
		return false
	}
	rs := []rune(k.Text())
	for _, r := range s.References {
		t := []rune(token(r))
		if end >= len(t) && end <= len(rs) && slices.Equal(rs[end-len(t):end], t) {
			k.SetText(string(rs[:end-len(t)]) + string(rs[end:]))
			s.caret = end - len(t)
			return true
		}
	}
	return false
}

// rows are the picker's current page filtered by the query: the root's
// for the marker, or the page entered; a folder starts with Select
// current (reference_select_current).
func (s *State) rows(c *ui.Context, ctx *Context) []Reference {
	st := &s.ref
	var list []Reference
	switch {
	case len(st.path) > 0:
		page := st.path[len(st.path)-1]
		if s.referenceChildren != nil {
			if children, ok := s.referenceChildren(page); ok {
				page.Children = children
			}
		}
		if page.Kind == Folder {
			list = append(list, Reference{ID: page.ID, Kind: Folder, Label: "reference_select_current", Key: true,
				Description: label(c, page), Icon: icons.Folder, Insert: page.Label})
		}
		list = append(list, page.Children...)
	case st.at.marker == '/':
		list = ctx.Commands
	default:
		list = ctx.References
	}
	q := strings.ToLower(st.at.query)
	if q == "" {
		return list
	}
	var out []Reference
	for _, r := range list {
		if strings.Contains(strings.ToLower(label(c, r)), q) || strings.Contains(strings.ToLower(description(c, r)), q) ||
			(r.Alias != "" && strings.Contains(r.Alias, q)) {
			out = append(out, r)
		}
	}
	return out
}

func label(c *ui.Context, r Reference) string {
	if r.Key {
		return i18n.T(c, r.Label)
	}
	return r.Label
}

func description(c *ui.Context, r Reference) string {
	if r.Key && r.Description != "" {
		return i18n.T(c, r.Description)
	}
	return r.Description
}

// track follows the trigger at the caret: a new one opens the picker at
// its root; Escape's dismissal holds until it changes.
func (s *State) trackTrigger(area ui.Element) {
	st := &s.ref
	_, caret := area.TextSelection()
	t := parseTrigger(s.Draft, caret)
	if t.start != st.at.start || t.marker != st.at.marker || t.ok != st.at.ok {
		st.path, st.sel = nil, 0
		if t.start != st.dismissed.start || t.marker != st.dismissed.marker {
			st.dismissed = trigger{}
		}
	}
	if t.query != st.at.query {
		st.sel = 0
	}
	st.at = t
	st.shown = t.ok && !(st.dismissed.ok && st.dismissed.start == t.start && st.dismissed.marker == t.marker)
}

// replace puts with in place of the trigger, the caret after it.
func (s *State) replace(with string) {
	rs := []rune(s.Draft)
	t := s.ref.at
	end := min(t.start+1+len([]rune(t.query)), len(rs))
	s.Draft = string(rs[:t.start]) + with + string(rs[end:])
	s.caret = t.start + len([]rune(with))
	s.focus = true
}

// choose carries out a row.
func (s *State) choose(c *ui.Context, ctx *Context, actions Actions, r Reference) {
	st := &s.ref
	if ctx.Stale != nil && ctx.Stale(r) {
		// The command or skill has gone since the picker listed it: say so
		// and show the picker's root again (reference_stale).
		s.inFrame(c, func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "reference_stale")) })
		st.path, st.sel = nil, 0
		return
	}
	if s.pluginCommand != nil {
		if replacement, handled := s.pluginCommand(r); handled {
			s.replace(replacement)
			st.path = nil
			return
		}
	}
	switch r.Kind {
	case Page, Folder:
		if r.Kind == Folder && r.Insert != "" {
			// Select current: the folder itself.
			s.chip(c, Reference{ID: r.ID, Kind: Folder, Label: r.Insert, Icon: icons.Folder})
			break
		}
		// Enter the page: the query starts over after the marker.
		s.replace(string(st.at.marker))
		st.path = append(st.path, r)
		st.sel = 0
		return
	case Prompt:
		// Enter chooses between frames: the prompt is put in the next one,
		// in its language.
		at := s.ref.at
		s.inFrame(c, func(c *ui.Context) {
			s.ref.at = at
			s.replace(i18n.T(c, r.Insert))
		})
	case ModeCommand:
		for i, m := range Modes {
			if m.label == r.Insert {
				s.Mode = Mode(i)
			}
		}
		s.replace("")
	case ModelCommand:
		s.Model, s.Effort = r.Insert, float64(defaultEffort(modelOf(ctx, r.Insert)))
		s.replace("")
	case EffortCommand:
		if i := slices.Index(efforts(s.Chosen(ctx.Models)), r.Insert); i >= 0 {
			s.Effort = float64(i)
		}
		s.replace("")
	case Compact:
		// The focus is what the draft already says after /compact.
		focus, typed := compactFocus(s.Draft)
		if typed {
			s.clear()
			s.caret, s.focus = 0, true
		} else {
			s.replace("")
		}
		s.compact(ctx, actions, focus)
	case AttachFiles:
		s.replace("")
		s.openFiles(c, ctx)
	default:
		if r.Kind == Database && r.Key && r.Insert != "" {
			// Select current: the database page itself.
			r.Label, r.Description, r.Key = r.Insert, "", false
		}
		s.chip(c, r)
	}
	st.path = nil
}

// compactFocus reads a draft that starts with the whole /compact command:
// the text after it is what the compaction keeps in focus, "" for none
// (compaction::focus). typed reports that the draft is such a command.
func compactFocus(draft string) (focus string, typed bool) {
	text := strings.TrimLeftFunc(draft, unicode.IsSpace)
	end := strings.IndexFunc(text, unicode.IsSpace)
	if end < 0 {
		end = len(text)
	}
	if text[:end] != "/compact" {
		return "", false
	}
	return strings.TrimSpace(text[end:]), true
}

// compacts reports whether the conversation offers /compact.
func compacts(ctx *Context) bool {
	return slices.ContainsFunc(ctx.Commands, func(r Reference) bool { return r.Kind == Compact })
}

// compact compacts the conversation, with the focus where the owner takes it.
func (s *State) compact(ctx *Context, actions Actions, focus string) {
	if ctx.Compact != nil {
		ctx.Compact(focus)
		return
	}
	actions.Compact()
}

// chip puts a reference in the draft as "@label ", one subagent a message.
func (s *State) chip(c *ui.Context, r Reference) {
	if r.Kind == Agent {
		for _, x := range s.active() {
			if x.Kind == Agent && x.ID != r.ID {
				s.inFrame(c, func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "reference_one_agent")) })
				return
			}
		}
	}
	r.Label, r.Description, r.Key, r.Children = label(c, r), description(c, r), false, nil
	s.replace(token(r) + " ")
	if !slices.ContainsFunc(s.References, func(x Reference) bool { return x.ID == r.ID }) {
		s.References = append(s.References, r)
	}
}

func modelOf(ctx *Context, id string) Model {
	for _, m := range ctx.Models {
		if m.ID == id {
			return m
		}
	}
	return Model{}
}

// pickerKey takes the picker's keys from the draft.
func (s *State) pickerKey(c *ui.Context, ctx *Context, actions Actions, k ui.TextKey) bool {
	st := &s.ref
	rows := s.rows(c, ctx)
	switch {
	case k.Mods != 0:
		return false
	case k.Key == ui.KeyDown:
		st.sel = next(len(rows), st.sel, false)
	case k.Key == ui.KeyUp:
		st.sel = next(len(rows), st.sel, true)
	case k.Key == ui.KeyEnter || k.Key == ui.KeyTab:
		if st.sel < 0 || st.sel >= len(rows) {
			return k.Key == ui.KeyEnter
		}
		s.Draft = k.Text()
		s.choose(c, ctx, actions, rows[st.sel])
		k.SetText(s.Draft)
	case k.Key == ui.KeyEscape:
		st.dismissed, st.shown, st.path = st.at, false, nil
	case k.Key == ui.KeyBackspace && st.at.query == "" && len(st.path) > 0:
		st.path, st.sel = st.path[:len(st.path)-1], 0
	default:
		return false
	}
	return true
}

// referencePicker is a panel over the surface, as wide as it: rows 36 high
// under their group headings, 320 high at most. Its rows take no focus, so
// typing goes on in the draft.
func (s *State) referencePicker(c *ui.Context, ctx *Context, actions Actions, surface ui.Element) {
	st := &s.ref
	if s.area != (ui.Element{}) {
		s.trackTrigger(s.area)
		s.area = ui.Element{} // this pass's alone
	}
	visible := st.shown && !ctx.Readonly
	opened := visible && !s.referenceVisible
	s.referenceVisible = visible
	if opened {
		st.pointed = false
	}
	if opened && s.referencePickerOpen != nil {
		s.referencePickerOpen()
	}
	if !visible {
		return
	}
	p := theme.Of(c)
	rows := s.rows(c, ctx)
	if st.sel >= len(rows) {
		st.sel = 0
	}
	title := i18n.T(c, "reference_title")
	switch {
	case len(st.path) > 0:
		title = label(c, st.path[len(st.path)-1])
	case st.at.marker == '/':
		title = i18n.T(c, "composer_commands")
	}
	// As wide as the composer's surface, with no header or footer: the
	// keys move, choose and go back (Backspace on an empty query).
	width := surface.LayoutBounds().W
	if width <= 0 {
		w, _ := c.Size()
		width = min(340, w-48)
	}
	ui.Overlay(c, func() {
		kit.Floating(c, ui.Column(c.Key("reference-picker"))).AttachTo(surface, ui.AnchorTopStart, ui.AnchorBottomStart).Margin(0, 0, 4, 0).
			Width(width).Padding(4).Role(ui.RoleList).Label(title).Children(func() {
			h := float32(max(len(rows), 1) * 36)
			for i, r := range rows {
				if r.Group != "" && (i == 0 || rows[i-1].Group != r.Group) {
					h += 28
				}
			}
			ui.Scroll(c.Key(len(st.path))).Height(min(h, 320)).Children(func() {
				if len(rows) == 0 {
					ui.Text(c, i18n.T(c, "reference_empty")).FontSize(14).Padding(12).TextColor(p.MutedForeground)
					return
				}
				for i, r := range rows {
					if r.Group != "" && (i == 0 || rows[i-1].Group != r.Group) {
						ui.Text(c.Key("group-"+r.Group), i18n.T(c, r.Group)).FontSize(12).Height(28).Padding(10, 8, 0, 8).
							SingleLine().TextColor(p.MutedForeground)
					}
					s.referenceRow(c, ctx, actions, r, i)
				}
			})
		})
	})
}

// referenceRow is the icon, the label, its muted description, a chevron for a
// page.
func (s *State) referenceRow(c *ui.Context, ctx *Context, actions Actions, r Reference, i int) {
	p := theme.Of(c)
	st := &s.ref
	selected := i == st.sel
	name := label(c, r)
	b := ui.ButtonBase(c.Key(r.ID)).Label(name).FillWidth().Height(36).PaddingX(8).Gap(8).Radius(p.Radius).Shrink(0).
		AlignItems(ui.Center).Role(ui.RoleOption).AccessSelected(selected).TabStop(false)
	if b.Hovered() {
		x, y, _ := b.PointerPosition()
		box := b.Bounds()
		at := [2]int{int(math.Round(float64(box.X + x))), int(math.Round(float64(box.Y + y)))}
		if st.pointed && at != st.pointer {
			st.sel = i
		}
		st.pointer, st.pointed = at, true
	}
	if selected {
		b.Background(p.Hover).TextColor(p.HoverForeground).ScrollIntoView()
	}
	icon := r.Icon
	if icon == nil {
		icon = icons.File
	}
	b.Children(func() {
		ui.Icon(c, icon).FontSize(16).Shrink(0).TextColor(p.MutedForeground)
		ui.Text(c, name).FontSize(14).SingleLine().Shrink(0).MaxWidth(240)
		if d := description(c, r); d != "" {
			ui.Text(c, d).FontSize(13).SingleLine().Grow(1).Shrink(1).MinWidth(0).TextColor(p.MutedForeground)
		} else {
			ui.Spacer(c)
		}
		if r.Kind == Page || (r.Kind == Folder && r.Insert == "") {
			ui.Icon(c, icons.ChevronRight).FontSize(12).Shrink(0)
		}
	}).OnClick(func() {
		s.choose(c, ctx, actions, r)
		s.focus = true
	})
}
