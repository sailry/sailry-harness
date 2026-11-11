package composer

import (
	"path"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// This file holds the composer's activity strip: one row over the composer of
// capsules, left to right the worktree's Git changes, the plugins' Status
// badges, the task progress, the background commands and their services, the
// running subagents, then, right-aligned, the approvals and the question
// (interactions.go). An empty strip still takes a row, so the composer never
// moves.

// Activity is what runs around the conversation, from the backend.
type Activity struct {
	// Changes is the worktree's Git changes, nil outside a repository.
	// Git is a core feature here, so its badge is native, not a plugin's.
	Changes *Changes
	// Badges are the plugins' Status-slot contributions, in their order.
	Badges []StatusBadge
	// Progress is the running turn's task progress, nil for none.
	Progress []conversation.Step
	// Commands are the session's background commands.
	Commands []Command
	// Children are the session's running subagents.
	Children []conversation.Child
	// Connected is the session's link: the busy capsules spin while it is up.
	Connected bool
}

// Changes are the totals of the worktree's Git status and its files
// changed last.
type Changes struct {
	Added, Removed, Files int
	Partial               bool          // the status was cut short
	Recent                []ChangedFile // the files changed last, newest first, at most RecentChanges
}

// ActivityCallbacks adds labels for host-owned worktree identities.
type ActivityCallbacks struct {
	ChildElapsed    func(string, time.Time) string
	CommandState    func() (bool, bool)
	CommandError    func(string) bool
	CommandBusy     func(string) bool
	ServiceLabel    func(int) string
	ServiceBusy     func(int) bool
	WorktreeLabel   func(string) string
	Worktree        func(string)
	WorktreeEnabled func() bool
}

// RecentChanges is how many changed files the Git badge lists.
const RecentChanges = 10

// ChangedFile is a file of the worktree's Git status.
type ChangedFile struct {
	Path           string
	Status         string // M, A, D
	Added, Removed int
}

// Text is a plugin's label: its English and its translations by locale.
type Text struct {
	Label   string
	Locales map[string]string
}

func (t Text) in(c *ui.Context) string {
	if s, ok := t.Locales[i18n.Match(c.Locale())]; ok {
		return s
	}
	return t.Label
}

// Segment is a part of a badge's label in a tone (plugin ui SegmentTone:
// Neutral is muted, Info the accent).
type Segment struct {
	Text Text
	Tone kit.Tone
}

// Detail is a line of a badge's tooltip, "label: value".
type Detail struct {
	Label Text
	Value string
}

// StatusBadge is a plugin's Status-slot control: a capsule of its icon and
// segments, or its label without them, the details as its tooltip. A popover
// badge opens its Panel.
type StatusBadge struct {
	ID      string // package/contribution
	Label   Text
	Icon    *ui.SVG // nil: Settings2
	Segment []Segment
	Details []Detail
	Panel   *Panel // nil for a button
}

// Panel is a plugin's status popover in the StatusList form: its body, or
// Empty without one, and its buttons at the end.
type Panel struct {
	Body, Empty Text
	Buttons     []PanelButton
}

// PanelButton is a button of a plugin's panel; Ghost for the quiet one.
type PanelButton struct {
	ID    string
	Label Text
	Ghost bool
}

// CommandStatus is a background command's state.
type CommandStatus int

const (
	CommandRunning CommandStatus = iota
	CommandStopping
)

// Command is a background command (sailry_protocol process::Info).
type Command struct {
	ID, Command string
	Started     time.Time
	Status      CommandStatus
	Output      string // its last output, shown when the row opens
	Ports       []int  // the services it listens on
}

// activity is the State's part for the strip: which popovers are open.
type activity struct {
	changes, progress, commands, children bool
	badges                                map[string]*bool
	command                               string // the command row open
}

// activity builds the strip: min_h_7, px_4, gap_2.
func (s *State) activity(c *ui.Context, ctx *Context, actions Actions) {
	a := ctx.Activity
	ui.Row(c.Key("activity")).FillWidth().MinHeight(28).PaddingX(16).Gap(8).Wrap().AlignItems(ui.Center).Children(func() {
		if a.Changes != nil {
			s.gitChanges(c, *a.Changes, actions)
		}
		for _, b := range a.Badges {
			s.badge(c, b, actions)
		}
		s.progress(c, a.Progress, a.Connected)
		s.commandActivity(c, a, actions)
		if !ctx.Readonly {
			s.subagents(c, a, actions)
		}
		ui.Spacer(c)
		in := ctx.Interactions
		if len(in.Approvals) > 0 {
			s.approvals(c, in.Approvals, actions)
		}
		if in.Question != nil {
			s.ask(c, in.Question, actions)
		}
	})
}

// gitChanges is the worktree's +added −removed; it opens the totals and
// the files changed last, each opening its diff in the panel.
func (s *State) gitChanges(c *ui.Context, ch Changes, actions Actions) {
	p := theme.Of(c)
	name := "Git +" + strconv.Itoa(ch.Added) + " -" + strconv.Itoa(ch.Removed)
	b := chip(c.Key("git-changes"), name, s.act.changes, func() {
		ui.Icon(c, icons.Network).FontSize(14)
		ui.Row(c).Gap(8).Children(func() {
			chipText(c, "+"+strconv.Itoa(ch.Added)).TextColor(p.Success)
			chipText(c, "-"+strconv.Itoa(ch.Removed)).TextColor(p.Danger)
		})
	}).HasPopup(ui.PopupDialog).OnClick(func() { s.act.changes = !s.act.changes })
	placeStart(kit.Popover(c, b, &s.act.changes, func() {
		// The files' count leads; the totals end the line, over the rows'.
		ui.Row(c.Key("totals")).Gap(8).Padding(0, 4, 4, 4).AlignItems(ui.Center).Children(func() {
			ui.Text(c, i18n.T(c, "directory_count_files", "count", ch.Files)).FontSize(13).SingleLine().TextColor(p.MutedForeground)
			ui.Spacer(c)
			ui.Textf(c, "+%d", ch.Added).FontSize(13).FontWeight(600).SingleLine().TextColor(p.Success)
			ui.Textf(c, "-%d", ch.Removed).FontSize(13).FontWeight(600).SingleLine().TextColor(p.Danger)
		})
		for _, f := range ch.Recent {
			dir, base := path.Split(f.Path)
			kit.Ghost(c.Key(f.Path), f.Path, false).FillWidth().Height(32).PaddingX(4).Gap(8).Children(func() {
				ui.Icon(c, icons.File).FontSize(14).TextColor(p.MutedForeground)
				ui.Row(c).Grow(1).MinWidth(0).Gap(6).AlignItems(ui.Center).Children(func() {
					ui.Text(c, base).FontSize(14).SingleLine().Shrink(0).MaxWidth(200).TextColor(p.Foreground)
					ui.Text(c, strings.TrimSuffix(dir, "/")).FontSize(12).SingleLine().Shrink(1).MinWidth(0).TextColor(p.MutedForeground)
				})
				ui.Textf(c, "+%d", f.Added).FontSize(12).SingleLine().TextColor(p.Success)
				ui.Textf(c, "-%d", f.Removed).FontSize(12).SingleLine().TextColor(p.Danger)
			}).OnClick(func() {
				s.act.changes = false
				actions.OpenChange(f.Path)
			})
		}
	}), b, 320, 8, 0)
}

// tone is a segment's color.
func tone(p *theme.Palette, t kit.Tone) ui.Color {
	switch t {
	case kit.Info:
		return p.Primary
	case kit.Positive:
		return p.Success
	case kit.Caution:
		return p.Warning
	case kit.Negative:
		return p.Danger
	}
	return p.MutedForeground
}

// badge is a plugin's Status capsule; a popover one opens its panel above
// it at its start (Anchor::BottomLeft, bottom_2).
func (s *State) badge(c *ui.Context, b StatusBadge, actions Actions) {
	p := theme.Of(c)
	if s.act.badges == nil {
		s.act.badges = map[string]*bool{}
	}
	open := s.act.badges[b.ID]
	if open == nil {
		open = new(bool)
		s.act.badges[b.ID] = open
	}
	if s.statusCallbacks != nil && s.statusCallbacks.Open != nil {
		*open = s.statusCallbacks.Open(b.ID)
	}
	icon := b.Icon
	if icon == nil {
		icon = icons.Settings2
	}
	label := b.Label.in(c)
	trigger := chip(c.Key("badge-"+b.ID), label, *open, func() {
		ui.Icon(c, icon).FontSize(14)
		ui.Row(c).Gap(8).Children(func() {
			for _, sg := range b.Segment {
				chipText(c, sg.Text.in(c)).TextColor(tone(p, sg.Tone))
			}
			if len(b.Segment) == 0 {
				chipText(c, label)
			}
		})
	})
	if s.statusCallbacks != nil && s.statusCallbacks.Enabled != nil {
		trigger.Disabled(!*open && !s.statusCallbacks.Enabled(b.ID))
	}
	var tip []string
	for _, d := range b.Details {
		line := d.Label.in(c)
		if d.Value != "" {
			line += ": " + d.Value
		}
		tip = append(tip, line)
	}
	kit.Tooltip(c, trigger, strings.Join(tip, "\n"))
	if b.Panel == nil {
		trigger.OnClick(func() { actions.Badge(b.ID, "") })
		return
	}
	trigger.HasPopup(ui.PopupDialog).OnClick(func() {
		next := !*open
		if s.statusCallbacks == nil || s.statusCallbacks.SetOpen == nil || s.statusCallbacks.SetOpen(b.ID, next) {
			*open = next
		}
	})
	pn := b.Panel
	wasOpen := *open
	placeStart(kit.Popover(c, trigger, open, func() {
		if s.statusCallbacks != nil && s.statusCallbacks.Render != nil && s.statusCallbacks.Render(c, b.ID) {
			return
		}
		if body := pn.Body.in(c); body != "" {
			ui.Text(c, body).FontSize(14).Selectable().TextColor(p.Foreground)
		} else {
			ui.Text(c, pn.Empty.in(c)).FontSize(14).TextColor(p.MutedForeground)
		}
		if len(pn.Buttons) > 0 {
			ui.Row(c.Key("footer")).Gap(8).Padding(8, 0, 0, 0).Justify(ui.End).Children(func() {
				for _, x := range pn.Buttons {
					text := x.Label.in(c)
					var bt ui.Element
					if x.Ghost {
						bt = kit.Ghost(c.Key(x.ID), text, false).PaddingX(8).Children(func() { ui.Text(c, text).FontSize(14) })
					} else {
						bt = kit.Secondary(c.Key(x.ID), text)
					}
					bt.Height(24).OnClick(func() { actions.Badge(b.ID, x.ID) })
				}
			})
		}
	}), trigger, 320, 12, 0)
	if wasOpen && !*open && s.statusCallbacks != nil && s.statusCallbacks.SetOpen != nil {
		s.statusCallbacks.SetOpen(b.ID, false)
	}
}

// progress is the running turn's task capsule, while steps remain: its
// done/total and a popover of the steps.
func (s *State) progress(c *ui.Context, steps []conversation.Step, animated bool) {
	done := 0
	for _, st := range steps {
		if st.State == conversation.StepCompleted || st.State == conversation.StepSkipped {
			done++
		}
	}
	if done >= len(steps) {
		return
	}
	label := i18n.T(c, "composer_todo_count", "completed", done, "total", len(steps))
	trigger := chip(c.Key("todo"), i18n.T(c, "task_progress"), s.act.progress, func() {
		ui.Icon(c, icons.Tasks).FontSize(14)
		chipText(c, label)
	}).HasPopup(ui.PopupDialog).OnClick(func() { s.act.progress = !s.act.progress })
	placeStart(kit.Popover(c, trigger, &s.act.progress, func() {
		ui.Scroll(c).MaxHeight(320).PaddingStart(8).Children(func() { conversation.Steps(c, steps, animated) })
	}), trigger, 320, 12, 0)
}

// working is a capsule whose icon is the loader (kit.Loader), the icon's
// size and in the chip's muted color, while the session is connected.
func working(c *ui.Context, icon *ui.SVG, label string, running, open bool) ui.Element {
	name := label
	if running {
		name += ": " + i18n.T(c, "chat_running")
	}
	return chip(c, name, open, func() {
		if running {
			kit.Loader(c, chipIcon)
		} else {
			ui.Icon(c, icon).FontSize(chipIcon)
		}
		chipText(c, label)
	}).HasPopup(ui.PopupDialog)
}

// chipIcon is the size of a chip's icon, and of the busy mark in its place.
const chipIcon = 14

// count is one item has its own key.
func count(c *ui.Context, one, many string, n int) string {
	if n == 1 {
		return i18n.T(c, one)
	}
	return i18n.T(c, many, "count", n)
}

// commandActivity is the running commands' capsule and popover, then a
// capsule per service port.
func (s *State) commandActivity(c *ui.Context, a Activity, actions Actions) {
	connected, unavailable := a.Connected, false
	if s.ActivityCallbacks != nil && s.ActivityCallbacks.CommandState != nil {
		connected, unavailable = s.ActivityCallbacks.CommandState()
	}
	var running []Command
	for _, cmd := range a.Commands {
		if cmd.Status == CommandRunning || cmd.Status == CommandStopping {
			running = append(running, cmd)
		}
	}
	if len(running) == 0 {
		s.act.commands = false
		return
	}
	label := count(c, "background_one", "background_count", len(running))
	trigger := working(c.Key("background"), icons.Terminal, label, connected, s.act.commands).
		OnClick(func() { s.act.commands = !s.act.commands })
	placeStart(kit.Popover(c, trigger, &s.act.commands, func() {
		c.After(time.Second) // the elapsed times tick while it shows
		if unavailable {
			ui.Text(c, i18n.T(c, "background_unavailable")).FontSize(14).TextColor(theme.Of(c).MutedForeground)
		}
		ui.Scroll(c.Key("background-rows")).MaxHeight(320).Gap(4).Children(func() {
			for _, cmd := range running {
				s.commandRow(c, cmd, connected, actions)
			}
		})
	}), trigger, 400, 8, 0)
	seen := map[int]bool{}
	for _, cmd := range running {
		for _, port := range cmd.Ports {
			if seen[port] {
				continue
			}
			seen[port] = true
			label, busy := strconv.Itoa(port), false
			if s.ActivityCallbacks != nil {
				if s.ActivityCallbacks.ServiceLabel != nil {
					label = s.ActivityCallbacks.ServiceLabel(port)
				}
				if s.ActivityCallbacks.ServiceBusy != nil {
					busy = s.ActivityCallbacks.ServiceBusy(port)
				}
			}
			b := chip(c.Key("service-"+strconv.Itoa(port)), label, false, func() {
				ui.Icon(c, icons.Globe).FontSize(14)
				chipText(c, label)
			}).Disabled(!connected || busy).OnClick(func() { actions.OpenService(port) })
			kit.Tooltip(c, b, i18n.T(c, "port_open_service"))
		}
	}
}

// commandRow is the status icon, the command (a click shows its output), the
// time it has run and Stop, a chevron until the row is hovered.
func (s *State) commandRow(c *ui.Context, cmd Command, connected bool, actions Actions) {
	p := theme.Of(c)
	open := s.act.command == cmd.ID
	status := i18n.T(c, "chat_running")
	if cmd.Status == CommandStopping {
		status = i18n.T(c, "chat_stopping")
	}
	ui.Column(c.Key(cmd.ID)).Children(func() {
		row := ui.Row(c.Key("header")).Height(24).Gap(8).AlignItems(ui.Center)
		hovered := row.Hovered()
		row.Children(func() {
			kit.Tooltip(c, ui.Icon(c, icons.Terminal).FontSize(14).Shrink(0).TextColor(p.MutedForeground), status)
			text := strings.Join(strings.Fields(cmd.Command), " ")
			ui.ButtonBase(c.Key("toggle")).Label(text).Grow(1).Shrink(1).MinWidth(0).Height(24).Justify(ui.Start).
				Cursor(ui.CursorPointer).Children(func() {
				ui.Text(c, text).FontSize(14).SingleLine().TextColor(p.MutedForeground)
			}).OnClick(func() {
				if open {
					s.act.command = ""
				} else {
					s.act.command = cmd.ID
				}
			})
			ui.Text(c, conversation.Elapsed(c.Now().Sub(cmd.Started))).FontSize(12).Shrink(0).TextColor(p.MutedForeground)
			icon := icons.ChevronRight
			switch {
			case hovered:
				icon = icons.Square
			case open:
				icon = icons.ChevronDown
			}
			kit.IconButton(c.Key("stop"), icon, i18n.T(c, "background_end"), 24, false).
				Disabled(!connected || cmd.Status == CommandStopping || s.ActivityCallbacks != nil && s.ActivityCallbacks.CommandBusy != nil && s.ActivityCallbacks.CommandBusy(cmd.ID)).OnClick(func() { actions.StopCommand(cmd.ID) })
		})
		if open && s.ActivityCallbacks != nil && s.ActivityCallbacks.CommandError != nil && s.ActivityCallbacks.CommandError(cmd.ID) {
			ui.Text(c, i18n.T(c, "background_unavailable")).FontSize(14).TextColor(p.MutedForeground)
		} else if open && cmd.Output != "" {
			ui.Scroll(c.Key("output")).MaxHeight(160).Margin(6, 0, 4, 0).Padding(8, 12).Radius(p.Radius).
				Background(p.Background).Children(func() {
				ui.Text(c, cmd.Output).Font("monospace").FontSize(12).LineHeight(18.0 / 12).Selectable().
					TextColor(p.MutedForeground)
			})
		}
	})
}

// subagents is the running subagents' capsule and its picker, a row each
// opening its session.
func (s *State) subagents(c *ui.Context, a Activity, actions Actions) {
	if len(a.Children) == 0 {
		s.act.children = false
		return
	}
	label := count(c, "composer_subagent_one", "composer_subagent_count", len(a.Children))
	running := false
	for _, child := range a.Children {
		running = running || child.Status.Active()
	}
	trigger := working(c.Key("subagents"), icons.Bot, label, a.Connected && running, s.act.children).
		OnClick(func() { s.act.children = !s.act.children })
	placeStart(kit.Popover(c, trigger, &s.act.children, func() {
		if a.Connected {
			c.After(time.Second)
		}
		ui.Scroll(c.Key("subagent-rows")).MaxHeight(6 * 28).Gap(2).Children(func() {
			for _, k := range a.Children {
				name := k.Title
				if name == "" {
					name = i18n.T(c, "chat_child")
				}
				row := kit.Ghost(c.Key(k.ID), name, false).FillWidth().Height(26).PaddingX(8).Gap(8).Justify(ui.Start).
					Children(func() {
						kit.Identicon(c, k.ID, k.Status.Active(), 16)
						ui.Text(c, name).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
						if s.ActivityCallbacks != nil && s.ActivityCallbacks.ChildElapsed != nil {
							if elapsed := s.ActivityCallbacks.ChildElapsed(k.ID, c.Now()); elapsed != "" {
								ui.Text(c, elapsed).FontSize(12).Shrink(0).TextColor(theme.Of(c).MutedForeground)
							}
						}
					}).OnClick(func() {
					s.act.children = false
					actions.OpenChild(k.ID)
				})
				status := "chat_running"
				if k.Status == conversation.ChildQueued {
					status = "turn_queued"
				} else if k.Status == conversation.ChildStopping {
					status = "chat_stopping"
				}
				kit.Tooltip(c, row, i18n.T(c, status))
			}
		})
	}), trigger, 360, 8, 0)
}

// placeStart puts a strip popover above its capsule at its start, 8 DIPs
// over it (Anchor::BottomLeft, bottom_2): pad around a body width wide.
func placeStart(panel, anchor ui.Element, width, pad, gap float32) {
	if panel != (ui.Element{}) {
		panel.AttachTo(anchor, ui.AnchorTopStart, ui.AnchorBottomStart).Margin(0, 0, 8, 0).
			Padding(pad).Width(width + 2*pad).Gap(gap)
	}
}
