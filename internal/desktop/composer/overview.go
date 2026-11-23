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
	// Turn tells there is a current turn: Status is how it goes and Elapsed
	// how long it ran.
	Turn     bool
	Status   conversation.RunStatus
	Elapsed  time.Duration
	Children []OverviewChild
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
	// folded hides a section without forgetting whether it was shown whole.
	folded map[string]bool
	// all are the sections shown whole past their first rows.
	all map[string]bool
	// refolded is the section just folded back to its first rows, whose
	// row showing it whole takes the focus and comes into view once.
	refolded string
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

// OverviewButton toggles the activity card in a wide pane and opens the
// popover in a narrow pane. The card's visibility is a window preference.
func OverviewButton(c *ui.Context, s *OverviewState, o *Overview, size float32, hidden *bool) {
	if !o.Session || s.Docked {
		s.open = false
	}
	if !s.Docked {
		s.sync(o)
	}
	title := i18n.T(c, "chat_assets_title")
	shown := s.open
	docked := s.Docked
	if docked {
		shown = !*hidden
	}
	trigger := kit.IconButton(c.Key("overview"), icons.Tasks, title, size, shown).Disabled(!o.Session).
		OnClick(func() {
			if docked {
				*hidden = !*hidden
			} else {
				s.open = !s.open
			}
		})
	if o.Active && !shown {
		p := theme.Of(c)
		trigger.Children(func() {
			ui.Box(c.Key("running")).Absolute().Top(size/2-9).Right(size/2-9).Size(6, 6).Radius(3).Background(p.Primary).PassThrough()
		})
	}
	if docked {
		return
	}
	trigger.HasPopup(ui.PopupDialog)
	_, height := c.Size()
	panel := kit.Popover(c, trigger, &s.open, func() {
		ui.Scroll(c.Key("overview-body")).MaxHeight(max(height-128, 120)).Children(func() { s.body(c, o, true) })
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

// overviewRows is how many rows a long section lists before a row that
// shows it whole.
const overviewRows = 3

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
	title := i18n.T(c, "chat_assets_title")
	card := kit.Floating(c, ui.Column(c.Key("overview-card"))).Width(OverviewWidth).Shrink(0).Clip().Radius(16).
		Role(ui.RoleComplementary).Label(title)
	card.Children(func() {
		ui.Scroll(c.Key("overview-body")).FillWidth().MaxHeight(maxHeight).Padding(14, 12, 12, 12).
			Children(func() { s.body(c, o, false) })
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
func (s *OverviewState) body(c *ui.Context, o *Overview, popover bool) {
	p := theme.Of(c)
	ui.Column(c.Key("overview")).FillWidth().MinWidth(0).Gap(10).Children(func() {
		// A quiet head, then every part a muted heading over a few rows,
		// summed up where they are many, the parts set apart by a rule
		// inset to the text.
		rule := func(key string) {
			ui.Row(c.Key("rule-" + key)).FillWidth().PaddingX(6).Children(func() {
				ui.Box(c).FillWidth().Height(1).Background(p.Border)
			})
		}
		s.head(c, o)
		if o.empty() {
			// One quiet line: what will come, not instructions.
			ui.Column(c.Key("empty")).FillWidth().PaddingY(16).Gap(8).AlignItems(ui.Center).TextColor(p.MutedForeground).Children(func() {
				ui.Icon(c, icons.Tasks).FontSize(18)
				ui.Text(c, i18n.T(c, "chat_assets_waiting")).FontSize(13).TextAlign(ui.Center)
			})
			return
		}
		section := func(key, label string, trailing func(), rows func()) {
			rule(key)
			open := !s.folded[key]
			ui.Column(c.Key(key)).FillWidth().MinWidth(0).Gap(2).Children(func() {
				ui.Row(c.Key("heading")).FillWidth().Height(28).Gap(8).PaddingX(6).AlignItems(ui.Center).Justify(ui.SpaceBetween).Children(func() {
					title := i18n.T(c, label)
					toggle := ui.ButtonBase(c.Key("title")).Label(title).Expanded(open).FillHeight().Shrink(1).MinWidth(0).Gap(4).
						Justify(ui.Start).Cursor(ui.CursorPointer).TextColor(p.MutedForeground)
					hovered := toggle.Hovered()
					toggle.Children(func() {
						ui.Text(c, title).FontSize(13).SingleLine().Shrink(1).MinWidth(0)
						chevron := icons.ChevronRight
						if open {
							chevron = icons.ChevronDown
						}
						arrow := ui.Icon(c, chevron).FontSize(14).Shrink(0)
						if !hovered {
							arrow.Opacity(0)
						}
					}).OnClick(func() {
						if s.folded == nil {
							s.folded = map[string]bool{}
						}
						s.folded[key] = !s.folded[key]
					})
					if trailing != nil {
						trailing()
					}
				})
				if open {
					ui.Column(c.Key("rows")).FillWidth().MinWidth(0).Children(rows)
				}
			})
		}
		ghost := func(key, name string, content func()) ui.Element {
			return kit.Ghost(c.Key(key), name, false).FillWidth().MinWidth(0).Height(30).PaddingX(6).Gap(10).Justify(ui.Start).
				Children(content)
		}
		// A row opens what it names, closing the popover; expand lists a
		// section whole and keeps it open.
		row := func(key, name string, content func(), click func()) ui.Element {
			return ghost(key, name, content).OnClick(func() {
				if popover {
					s.open = false
				}
				click()
			})
		}
		expand := func(key, name, section string, content func()) ui.Element {
			return ghost(key, name, content).OnClick(func() {
				if s.all == nil {
					s.all = map[string]bool{}
				}
				s.all[section] = true
			})
		}
		name := func(text string) ui.Element {
			return ui.Text(c, text).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
		}
		// capped lists a section's first rows, then a row showing it whole.
		// A foldable section shown whole ends in a row folding it back, so a
		// long list scrolled to its end need not be scrolled back up; the row
		// showing it whole then takes the focus and comes into view.
		capped := func(key string, n int, foldable bool, item func(i int)) {
			shown := n
			if n > overviewRows && !s.all[key] {
				shown = overviewRows
			}
			for i := range shown {
				item(i)
			}
			refolded := s.refolded == key
			if refolded {
				s.refolded = ""
			}
			switch {
			case shown < n:
				all := i18n.T(c, "chat_assets_all") + " " + strconv.Itoa(n)
				b := expand("all-"+key, all, key, func() {
					ui.Icon(c, icons.ChevronDown).FontSize(15).Shrink(0)
					name(all)
				}).TextColor(p.MutedForeground)
				if refolded {
					b.AutoFocus().ScrollIntoView()
				}
			case foldable && n > overviewRows:
				fold := i18n.T(c, "chat_assets_fold")
				ghost("fold-"+key, fold, func() {
					ui.Icon(c, icons.ChevronUp).FontSize(15).Shrink(0)
					name(fold)
				}).TextColor(p.MutedForeground).OnClick(func() {
					delete(s.all, key)
					s.refolded = key
				})
			}
		}
		tally := func(text string) func() {
			return func() {
				ui.Text(c, text).FontSize(13).FontFeatures("tnum").SingleLine().Shrink(0).TextColor(p.MutedForeground)
			}
		}
		if n := len(o.Steps); n > 0 {
			done := 0
			for _, st := range o.Steps {
				if st.State == conversation.StepCompleted || st.State == conversation.StepSkipped {
					done++
				}
			}
			section("progress", "chat_assets_progress", tally(fmt.Sprintf("%d/%d", done, n)), func() {
				// The steps' marks stand under the heading's text, as the
				// other sections' rows do; the tally tells how far it is.
				ui.Column(c.Key("steps")).FillWidth().MinWidth(0).PaddingX(6).Children(func() {
					conversation.QuietSteps(c, o.Steps, o.Active)
				})
			})
		}
		if n := len(o.Children); n > 0 {
			section("activity", "chat_assets_activity", nil, func() {
				// One line of their avatars and how many finished, which
				// opens the list.
				if !s.all["activity"] {
					done, running := 0, false
					for _, k := range o.Children {
						// A child that neither runs nor waits has ended,
						// whatever its outcome.
						if k.Completed || !k.Running && k.Status != "turn_queued" {
							done++
						}
						running = running || k.Running
					}
					summary := i18n.T(c, "subagent_count", "completed", done, "total", n)
					expand("children", summary, "activity", func() {
						shown := min(n, 4)
						ui.Box(c.Key("avatars")).Size(float32(18+12*(shown-1)), 18).Shrink(0).Children(func() {
							for i := range shown {
								k := o.Children[i]
								ui.Box(c.Key(k.ID)).Absolute().Left(float32(12 * i)).Top(0).Children(func() {
									kit.RoleIdenticon(c, k.ID, k.Avatar, k.Running, 18)
								})
							}
						})
						name(summary)
						if running {
							kit.BusySized(c, 12)
						}
					})
					return
				}
				for _, k := range o.Children {
					label := k.Name
					if label == "" {
						label = i18n.T(c, "chat_child")
					}
					b := row("child-"+k.ID, label, func() {
						kit.RoleIdenticon(c, k.ID, k.Avatar, k.Running, 16)
						name(label)
						if k.Completed {
							ui.Icon(c, icons.Check).FontSize(16).Shrink(0).TextColor(p.Success)
						}
					}, func() { o.OpenChild(k.ID) })
					if k.Status != "" {
						kit.Tooltip(c, b, i18n.T(c, k.Status))
					}
				}
			})
		}
		if len(o.Changes) > 0 {
			added, removed := 0, 0
			for _, ch := range o.Changes {
				added, removed = added+ch.Added, removed+ch.Removed
			}
			section("changes", "chat_assets_changes", func() { lines(c, added, removed) }, func() {
				capped("changes", len(o.Changes), true, func(i int) {
					ch := o.Changes[i]
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
				})
			})
		}
		asset := func(a OverviewAsset) {
			label := a.Label
			if label == "" {
				label = a.Name
			}
			b := row("asset-"+a.ID, a.Name, func() {
				ui.Icon(c, a.Icon).FontSize(15).Shrink(0).TextColor(p.MutedForeground)
				name(label)
			}, func() { o.OpenAsset(a.ID) })
			if label != a.Name {
				kit.Tooltip(c, b, a.Name)
			}
		}
		if len(o.Output) > 0 {
			section("output", "chat_assets_output", nil, func() {
				capped("output", len(o.Output), false, func(i int) { asset(o.Output[i]) })
			})
		}
		if n := len(o.Sources) + len(o.Citations); n > 0 {
			section("sources", "chat_assets_sources", nil, func() {
				capped("sources", n, false, func(i int) {
					if i < len(o.Sources) {
						asset(o.Sources[i])
						return
					}
					i -= len(o.Sources)
					source := o.Citations[i]
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
				})
			})
		}
		// Only the Host's inventory waits for its read; what the transcript
		// holds shows at once.
		// The Host's further pages follow a list shown whole.
		switch {
		case o.Loading:
			ui.Row(c.Key("loading")).PaddingX(8).Children(func() { kit.Loader(c, 14) })
		case o.Failed:
			ui.Row(c.Key("retry")).FillWidth().Gap(8).PaddingX(8).AlignItems(ui.Center).Children(func() {
				ui.Text(c, i18n.T(c, "chat_assets_failed")).FontSize(13).Grow(1).Shrink(1).MinWidth(0).TextColor(p.MutedForeground)
				kit.Secondary(c, i18n.T(c, "chat_retry")).Height(28).OnClick(func() { o.Load(s.more) })
			})
		case o.More && s.all["output"] || o.More && s.all["sources"] || o.More && len(o.Output)+len(o.Sources) <= overviewRows:
			ui.Row(c.Key("more")).FillWidth().Justify(ui.Center).Children(func() {
				kit.Secondary(c, i18n.T(c, "chat_assets_more")).Height(28).OnClick(func() {
					s.more = true
					o.Load(true)
				})
			})
		}
	})
}

// head is the overview's quiet first line: the current turn's mark, state
// and the time at the end; or the overview's title before a turn. It stays
// one line so the sections below keep their place while a turn runs.
func (s *OverviewState) head(c *ui.Context, o *Overview) {
	p := theme.Of(c)
	ui.Column(c.Key("head")).FillWidth().MinWidth(0).PaddingX(6).Children(func() {
		ui.Row(c.Key("state")).FillWidth().MinWidth(0).Height(24).Gap(8).AlignItems(ui.Center).Children(func() {
			ui.Box(c.Key("mark")).Size(16, 16).Shrink(0).Center().Children(func() {
				switch {
				case !o.Turn:
					ui.Icon(c, icons.Tasks).FontSize(15).TextColor(p.MutedForeground)
				case o.Status.Active():
					kit.BusySized(c, 14)
				case o.Status == conversation.Completed:
					ui.Icon(c, icons.CircleCheck).FontSize(15).TextColor(p.Success)
				case o.Status == conversation.Failed:
					ui.Icon(c, icons.CircleX).FontSize(15).TextColor(p.Danger)
				default:
					ui.Box(c).Size(8, 8).Radius(4).Background(p.MutedForeground)
				}
			})
			state := i18n.T(c, "chat_assets_title")
			if o.Turn {
				state = i18n.T(c, conversation.StatusKey(o.Status))
			}
			ui.Text(c, state).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
			if o.Turn && o.Elapsed > 0 {
				ui.Text(c, conversation.Elapsed(o.Elapsed)).FontSize(13).FontFeatures("tnum").SingleLine().Shrink(0).
					TextColor(p.MutedForeground)
			}
		})
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
