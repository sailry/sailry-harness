package composer

import (
	"fmt"
	"net/url"
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

// Overview is the session's activity, which a conversation's header button
// opens as a popover or, where the transcript leaves room beside it, shows
// as a column (OverviewColumn): how the current turn goes, its task
// progress and subagents, the files the session changed, and its outputs
// and sources, which the Host lists a page at a time.
type Overview struct {
	// Session is false on a draft, which has nothing to list: the button
	// is disabled.
	Session bool
	// Turn tells there is a current turn: Status is how it goes, Elapsed
	// how long it ran and, while it runs, Doing the label of the tool call
	// under way and DoingDetail what it acts on.
	Turn               bool
	Status             conversation.RunStatus
	Elapsed            time.Duration
	Doing, DoingDetail string
	Children           []OverviewChild
	// Steps is the current turn's task progress, nil for none; Active
	// animates the step in progress.
	Steps  []conversation.Step
	Active bool
	// Changes are the files the session's turns changed, each once.
	Changes []OverviewChange
	// Output and Sources are the rows read so far. Citations are the
	// current turn's cited pages, after the read sources.
	Output, Sources []OverviewAsset
	Citations       []conversation.Citation
	// Loading is a read under way, Failed its failure (Retry reads the same
	// page again) and More a page still unread.
	Loading, Failed, More bool

	// Opened tells the owner the overview shows or no longer does: it reads
	// the first page when what it holds is behind, and drops a read under
	// way.
	Opened func(open bool)
	// Load reads the next page (more) or the page that failed.
	Load       func(more bool)
	OpenChild  func(id string)
	OpenAsset  func(id string)
	OpenSource func(uri string)
	// OpenChange reviews a changed file in the turn that changed it last.
	OpenChange func(turn, path string)
}

// OverviewChange is a file the session changed: the lines its turns added
// and removed, and the last turn that changed it.
type OverviewChange struct {
	Turn, Path     string
	Added, Removed int
}

// empty reports whether the overview has nothing to show but its status.
func (o *Overview) empty() bool {
	return len(o.Children)+len(o.Steps)+len(o.Changes)+len(o.Output)+len(o.Sources)+len(o.Citations) == 0 &&
		!o.Loading && !o.Failed
}

// OverviewChild is a subagent of the current turn: Status is an i18n key,
// shown for one that neither runs nor completed.
type OverviewChild struct {
	ID, Name, Status   string
	Running, Completed bool
	// Avatar is the avatar of the role it runs as, "" for its identicon.
	Avatar string
}

// OverviewAsset is an output or a source: Name is its whole name, the row's
// accessible name, and Label what the row shows when it is shorter (a
// source file's last element, with the path as its tooltip).
type OverviewAsset struct {
	ID, Name, Label string
	Icon            *ui.SVG
}

// OverviewState is the overview's own state, as a popover or a column.
type OverviewState struct {
	open, was bool
	more      bool // the read that failed was for a further page
	// Docked means the conversation has room for the overview column.
	Docked bool
}

// Open reports whether the popover shows.
func (s *OverviewState) Open() bool { return s.open }

func (s *OverviewState) sync(o *Overview) {
	if s.open == s.was {
		return
	}
	s.was = s.open
	if s.open {
		s.more = false
	}
	if o != nil && o.Opened != nil {
		o.Opened(s.open)
	}
}

// OverviewButton opens the activity popover when the pane has no room for
// its fixed column. Wide panes show the column without a visibility toggle.
func OverviewButton(c *ui.Context, s *OverviewState, o *Overview, size float32) {
	if !o.Session || s.Docked {
		s.open = false
	}
	if s.Docked {
		return
	}
	s.sync(o)
	title := i18n.T(c, "chat_assets_title")
	shown := s.open
	trigger := kit.IconButton(c.Key("overview"), icons.Tasks, title, size, shown).Disabled(!o.Session).
		OnClick(func() { s.open = !s.open }).HasPopup(ui.PopupDialog)
	if o.Active && !shown {
		p := theme.Of(c)
		trigger.Children(func() {
			ui.Box(c.Key("running")).Absolute().Top(size/2-9).Right(size/2-9).Size(6, 6).Radius(3).Background(p.Primary).PassThrough()
		})
	}
	_, height := c.Size()
	panel := kit.Popover(c, trigger, &s.open, func() {
		ui.Scroll(c.Key("overview-body")).MaxHeight(max(height-128, 120)).Children(func() { s.body(c, o, true, false) })
	})
	if panel != (ui.Element{}) {
		panel.AttachTo(trigger, ui.AnchorBottomEnd, ui.AnchorTopEnd).Padding(12).Width(OverviewWidth).Label(title)
	}
}

// OverviewCardShown reports whether the overview is a session's, which has
// a card wherever there is room for one, its place fixed: with nothing to
// list yet it says so in one line under its head.
func OverviewCardShown(o *Overview) bool { return o.Session }

// OverviewWidth is the overview's width, as a popover and as a card.
const OverviewWidth = 320

// OverviewCard is the activity card beside a wide transcript. Its header
// shows the current turn's status, or the activity title before a turn.
// The sections scroll within maxHeight only past it. Only a session has a
// card (OverviewCardShown). It tells the owner it shows as the popover's
// opening does, so the inventory is read for it.
func OverviewCard(c *ui.Context, s *OverviewState, o *Overview, maxHeight float32) {
	if !s.was {
		s.was = true
		if o.Opened != nil {
			o.Opened(true)
		}
	}
	if !OverviewCardShown(o) {
		return
	}
	p := theme.Of(c)
	title := i18n.T(c, "chat_assets_title")
	card := kit.Floating(c, ui.Column(c.Key("overview-card"))).Width(OverviewWidth).Shrink(0).Clip().
		Role(ui.RoleComplementary).Label(title)
	card.Children(func() {
		head := ui.Row(c.Key("head")).FillWidth().MinWidth(0).Padding(12, 14).Gap(8).AlignItems(ui.Center)
		head.Children(func() {
			ui.Box(c.Key("mark")).Size(16, 16).Shrink(0).Center().Children(func() {
				switch {
				case !o.Turn:
					ui.Icon(c, icons.Tasks).FontSize(16).TextColor(p.MutedForeground)
				case o.Status.Active():
					kit.BusySized(c, 14)
				case o.Status == conversation.Completed:
					ui.Icon(c, icons.CircleCheck).FontSize(16).TextColor(p.Success)
				case o.Status == conversation.Failed:
					ui.Icon(c, icons.CircleX).FontSize(16).TextColor(p.Danger)
				default:
					ui.Box(c).Size(8, 8).Radius(4).Background(p.MutedForeground)
				}
			})
			ui.Column(c.Key("text")).Grow(1).Shrink(1).MinWidth(0).Gap(2).Children(func() {
				state := title
				if o.Turn {
					state = i18n.T(c, conversation.StatusKey(o.Status))
				}
				ui.Text(c, state).FontSize(14).FontWeight(500).SingleLine().MinWidth(0)
				if o.Turn && o.Status.Active() && o.Doing != "" {
					doing := o.Doing
					if o.DoingDetail != "" {
						doing += " · " + o.DoingDetail
					}
					ui.Text(c, doing).FontSize(12).SingleLine().MinWidth(0).TextColor(p.MutedForeground)
				}
			})
			if o.Turn && o.Elapsed > 0 {
				ui.Text(c, conversation.Elapsed(o.Elapsed)).FontSize(13).FontFeatures("tnum").SingleLine().Shrink(0).
					TextColor(p.MutedForeground)
			}
		})
		ui.Divider(c.Key("divider")).Background(p.Border)
		ui.Scroll(c.Key("overview-body")).FillWidth().MaxHeight(max(maxHeight-56, 120)).Padding(12, 6, 12, 6).
			Children(func() { s.body(c, o, false, true) })
	})
}

// OverviewClosed tells the owner a column no longer shows, as a popover's
// closing does.
func OverviewClosed(s *OverviewState, o *Overview) {
	if s.was && !s.open {
		s.was = false
		if o != nil && o.Opened != nil {
			o.Opened(false)
		}
	}
}

// body is the overview: the current turn's status, then each section that
// has rows, its count beside its name; a session with nothing yet says
// what will show. popover closes the popover as a row opens what it names.
func (s *OverviewState) body(c *ui.Context, o *Overview, popover, card bool) {
	p := theme.Of(c)
	ui.Column(c.Key("overview")).FillWidth().MinWidth(0).Gap(12).Children(func() {
		// Every part is a heading over its rows, the parts set apart by a
		// rule inset to the rows' text. The card's head is the status.
		first := true
		rule := func(key string) {
			if first {
				first = false
				return
			}
			ui.Row(c.Key("rule-" + key)).FillWidth().PaddingX(8).Children(func() {
				ui.Box(c).FillWidth().Height(1).Background(p.Border)
			})
		}
		if o.Turn && !card {
			rule("status")
			s.status(c, o)
		}
		if o.empty() {
			// One quiet line: what will come, not instructions.
			ui.Column(c.Key("empty")).FillWidth().PaddingY(16).Gap(8).AlignItems(ui.Center).TextColor(p.MutedForeground).Children(func() {
				ui.Icon(c, icons.Tasks).FontSize(18)
				ui.Text(c, i18n.T(c, "chat_assets_waiting")).FontSize(13).TextAlign(ui.Center)
			})
			return
		}
		section := func(key, label, tally string, rows func()) {
			rule(key)
			ui.Column(c.Key(key)).FillWidth().MinWidth(0).Gap(2).Children(func() {
				ui.Row(c.Key("heading")).FillWidth().Height(24).Gap(8).Padding(0, 8, 4, 8).AlignItems(ui.Center).Children(func() {
					ui.Text(c, i18n.T(c, label)).FontSize(13).FontWeight(500).SingleLine().Grow(1).Shrink(1).MinWidth(0).
						TextColor(p.MutedForeground)
					if tally != "" {
						ui.Text(c, tally).FontSize(13).FontFeatures("tnum").SingleLine().Shrink(0).TextColor(p.MutedForeground)
					}
				})
				// The rows stand in from their heading, under it.
				ui.Column(c.Key("rows")).FillWidth().MinWidth(0).PaddingStart(8).Children(rows)
			})
		}
		row := func(key, name string, content func(), click func()) ui.Element {
			return kit.Ghost(c.Key(key), name, false).FillWidth().MinWidth(0).Height(32).PaddingX(8).Gap(8).Justify(ui.Start).
				Children(content).OnClick(func() {
				if popover {
					s.open = false
				}
				click()
			})
		}
		name := func(text string) ui.Element {
			return ui.Text(c, text).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
		}
		if n := len(o.Steps); n > 0 {
			done := 0
			for _, st := range o.Steps {
				if st.State == conversation.StepCompleted || st.State == conversation.StepSkipped {
					done++
				}
			}
			section("progress", "chat_assets_progress", fmt.Sprintf("%d/%d", done, n), func() {
				// The steps' marks stand under the heading's text, as the
				// other sections' rows do; the tally tells how far it is.
				ui.Column(c.Key("steps")).FillWidth().MinWidth(0).PaddingX(8).Children(func() {
					conversation.QuietSteps(c, o.Steps, o.Active)
				})
			})
		}
		if len(o.Children) > 0 {
			section("activity", "chat_assets_activity", strconv.Itoa(len(o.Children)), func() {
				for _, k := range o.Children {
					label := k.Name
					if label == "" {
						label = i18n.T(c, "chat_child")
					}
					status := i18n.T(c, k.Status)
					row("child-"+k.ID, label+": "+status, func() {
						kit.RoleIdenticon(c, k.ID, k.Avatar, k.Running, 16)
						name(label)
						switch {
						case k.Completed:
							ui.Icon(c, icons.Check).FontSize(16).Shrink(0).TextColor(p.Success)
						default:
							ui.Text(c, status).FontSize(13).SingleLine().Shrink(0).TextColor(p.MutedForeground)
						}
					}, func() { o.OpenChild(k.ID) })
				}
			})
		}
		if len(o.Changes) > 0 {
			added, removed := 0, 0
			for _, ch := range o.Changes {
				added, removed = added+ch.Added, removed+ch.Removed
			}
			section("changes", "chat_assets_changes", strconv.Itoa(len(o.Changes)), func() {
				for _, ch := range o.Changes {
					file := path.Base(ch.Path)
					b := row("change-"+ch.Path, ch.Path, func() {
						ui.Icon(c, icons.File).FontSize(16).Shrink(0).TextColor(p.MutedForeground)
						name(file)
						lines(c, ch.Added, ch.Removed)
					}, func() {
						if o.OpenChange != nil {
							o.OpenChange(ch.Turn, ch.Path)
						}
					})
					if file != ch.Path {
						kit.Tooltip(c, b, ch.Path)
					}
				}
			})
		}
		asset := func(a OverviewAsset) {
			label := a.Label
			if label == "" {
				label = a.Name
			}
			b := row("asset-"+a.ID, a.Name, func() {
				ui.Icon(c, a.Icon).FontSize(16).Shrink(0).TextColor(p.MutedForeground)
				name(label)
			}, func() { o.OpenAsset(a.ID) })
			if label != a.Name {
				kit.Tooltip(c, b, a.Name)
			}
		}
		if len(o.Output) > 0 {
			section("output", "chat_assets_output", strconv.Itoa(len(o.Output)), func() {
				for _, a := range o.Output {
					asset(a)
				}
			})
		}
		if n := len(o.Sources) + len(o.Citations); n > 0 {
			section("sources", "chat_assets_sources", strconv.Itoa(n), func() {
				for _, a := range o.Sources {
					asset(a)
				}
				for i, source := range o.Citations {
					label := source.Title
					if label == "" {
						label = source.URI
					}
					b := row("source-"+strconv.Itoa(i), label, func() {
						ui.Icon(c, icons.Globe).FontSize(16).Shrink(0).TextColor(p.MutedForeground)
						name(label)
						if host := siteOf(source.URI); host != "" && host != label {
							ui.Text(c, host).FontSize(13).SingleLine().Shrink(1).MinWidth(0).MaxWidth(120).TextColor(p.MutedForeground)
						}
					}, func() { o.OpenSource(source.URI) })
					kit.Tooltip(c, b, source.URI)
				}
			})
		}
		// Only the Host's inventory waits for its read; what the transcript
		// holds shows at once.
		switch {
		case o.Loading:
			ui.Row(c.Key("loading")).PaddingX(8).Children(func() { kit.Loader(c, 14) })
		case o.Failed:
			ui.Row(c.Key("retry")).FillWidth().Gap(8).PaddingX(8).AlignItems(ui.Center).Children(func() {
				ui.Text(c, i18n.T(c, "chat_assets_failed")).FontSize(13).Grow(1).Shrink(1).MinWidth(0).TextColor(p.MutedForeground)
				kit.Secondary(c, i18n.T(c, "chat_retry")).Height(28).OnClick(func() { o.Load(s.more) })
			})
		case o.More:
			ui.Row(c.Key("more")).FillWidth().Justify(ui.Center).Children(func() {
				kit.Secondary(c, i18n.T(c, "chat_assets_more")).Height(28).OnClick(func() {
					s.more = true
					o.Load(true)
				})
			})
		}
	})
}

// status is the current turn's line: its state's mark and name and how long
// it ran, and while it runs the tool call under way below, muted.
func (s *OverviewState) status(c *ui.Context, o *Overview) {
	p := theme.Of(c)
	ui.Column(c.Key("status")).FillWidth().MinWidth(0).Gap(4).Padding(10, 12).Radius(p.RadiusLg).Background(p.Muted).Children(func() {
		ui.Row(c.Key("state")).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
			ui.Box(c.Key("mark")).Size(16, 16).Shrink(0).Center().Children(func() {
				switch {
				case o.Status.Active():
					kit.BusySized(c, 14)
				case o.Status == conversation.Completed:
					ui.Icon(c, icons.CircleCheck).FontSize(16).TextColor(p.Success)
				case o.Status == conversation.Failed:
					ui.Icon(c, icons.CircleX).FontSize(16).TextColor(p.Danger)
				default:
					ui.Box(c).Size(8, 8).Radius(4).Background(p.MutedForeground)
				}
			})
			ui.Text(c, i18n.T(c, conversation.StatusKey(o.Status))).FontSize(14).FontWeight(500).SingleLine().Grow(1).Shrink(1).MinWidth(0)
			if o.Elapsed > 0 {
				ui.Text(c, conversation.Elapsed(o.Elapsed)).FontSize(13).FontFeatures("tnum").SingleLine().Shrink(0).TextColor(p.MutedForeground)
			}
		})
		if o.Status.Active() && o.Doing != "" {
			doing := o.Doing
			if o.DoingDetail != "" {
				doing += " · " + o.DoingDetail
			}
			ui.Text(c, doing).FontSize(13).SingleLine().MinWidth(0).Padding(0, 0, 0, 24).TextColor(p.MutedForeground)
		}
	})
}

// lines are a change's line counts, added in the success color and removed
// in the danger color, in tabular figures.
func lines(c *ui.Context, added, removed int) {
	p := theme.Of(c)
	ui.Row(c.Key("lines")).Gap(4).Shrink(0).Children(func() {
		if added > 0 {
			ui.Text(c, "+"+strconv.Itoa(added)).FontSize(13).FontFeatures("tnum").SingleLine().TextColor(p.Success)
		}
		if removed > 0 {
			ui.Text(c, "−"+strconv.Itoa(removed)).FontSize(13).FontFeatures("tnum").SingleLine().TextColor(p.Danger)
		}
	})
}

// siteOf is a page's host without its www., "" for no web address.
func siteOf(uri string) string {
	u, err := url.Parse(uri)
	if err != nil || u.Host == "" {
		return ""
	}
	return strings.TrimPrefix(u.Hostname(), "www.")
}
