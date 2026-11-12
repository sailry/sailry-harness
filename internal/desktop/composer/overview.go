package composer

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"strconv"
)

// Overview is the session overview a conversation's header button opens:
// the current turn's subagents, task progress and cited pages, which the
// transcript already holds, and the session's outputs and sources, which
// the Host lists a page at a time.
type Overview struct {
	// Session is false on a draft, which has nothing to list: the button
	// is disabled.
	Session  bool
	Children []OverviewChild
	// Steps is the current turn's task progress, nil for none; Active
	// animates the step in progress.
	Steps  []conversation.Step
	Active bool
	// Output and Sources are the rows read so far. Citations are the
	// current turn's cited pages, after the read sources.
	Output, Sources []OverviewAsset
	Citations       []conversation.Citation
	// Loading is a read under way, Failed its failure (Retry reads the same
	// page again) and More a page still unread.
	Loading, Failed, More bool

	// Opened tells the owner the popover opened or closed: it reads the
	// first page when what it holds is behind, and drops a read under way.
	Opened func(open bool)
	// Load reads the next page (more) or the page that failed.
	Load       func(more bool)
	OpenChild  func(id string)
	OpenAsset  func(id string)
	OpenSource func(uri string)
}

// OverviewChild is a subagent of the current turn: Status is an i18n key,
// shown for one that neither runs nor completed.
type OverviewChild struct {
	ID, Name, Status   string
	Running, Completed bool
}

// OverviewAsset is an output or a source: Name is its whole name, the row's
// accessible name, and Label what the row shows when it is shorter (a
// source file's last element, with the path as its tooltip).
type OverviewAsset struct {
	ID, Name, Label string
	Icon            *ui.SVG
}

// OverviewState is the overview popover's own state.
type OverviewState struct {
	open, was bool
	more      bool // the read that failed was for a further page
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

// OverviewButton is the header's overview button, size square, and its
// popover.
func OverviewButton(c *ui.Context, s *OverviewState, o *Overview, size float32) {
	if !o.Session {
		s.open = false
	}
	s.sync(o)
	title := i18n.T(c, "chat_assets_title")
	trigger := kit.IconButton(c.Key("overview"), icons.Tasks, title, size, s.open).HasPopup(ui.PopupDialog).
		Disabled(!o.Session).OnClick(func() { s.open = !s.open })
	_, height := c.Size()
	panel := kit.Popover(c, trigger, &s.open, func() {
		ui.Scroll(c.Key("overview-body")).MaxHeight(max(height-128, 120)).Gap(8).Children(func() { s.body(c, o) })
	})
	if panel != (ui.Element{}) {
		panel.AttachTo(trigger, ui.AnchorBottomEnd, ui.AnchorTopEnd).Padding(12).Width(360).Label(title)
	}
}

func (s *OverviewState) body(c *ui.Context, o *Overview) {
	p := theme.Of(c)
	section := func(key, label string, rows func()) {
		ui.Column(c.Key(key)).FillWidth().MinWidth(0).Gap(2).Children(func() {
			// The heading stands clear of its rows.
			ui.Text(c, i18n.T(c, label)).FontSize(14).FontWeight(600).Padding(0, 0, 6, 0).TextColor(p.PopoverForeground)
			rows()
		})
	}
	empty := func(label string) {
		ui.Text(c, i18n.T(c, label)).FontSize(14).PaddingY(2).TextColor(p.MutedForeground)
	}
	rule := func(key string) { ui.Box(c.Key(key)).FillWidth().Height(1).Shrink(0).Background(p.Border) }
	row := func(key, name string, content func(), click func()) ui.Element {
		return kit.Ghost(c.Key(key), name, false).FillWidth().Height(28).PaddingX(8).Gap(8).Justify(ui.Start).
			Children(content).OnClick(func() {
			s.open = false
			click()
		})
	}
	name := func(text string) {
		ui.Text(c, text).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
	}
	// What the transcript holds shows at once; only the Host's inventory
	// waits for its read.
	pending := func() {
		if o.Loading {
			ui.Row(c.Key("loading")).PaddingY(4).Children(func() { kit.Loader(c, 14) })
		}
	}

	section("activity", "chat_assets_activity", func() {
		if len(o.Children) == 0 {
			empty("chat_assets_no_activity")
			return
		}
		ui.Text(c, count(c, "composer_subagent_one", "composer_subagent_count", len(o.Children))).FontSize(14).
			TextColor(p.MutedForeground)
		for _, k := range o.Children {
			label := k.Name
			if label == "" {
				label = i18n.T(c, "chat_child")
			}
			status := i18n.T(c, k.Status)
			row("child-"+k.ID, label+": "+status, func() {
				kit.Identicon(c, k.ID, k.Running, 16)
				name(label)
				switch {
				case k.Completed:
					ui.Icon(c, icons.Check).FontSize(16).Shrink(0)
				case !k.Running:
					ui.Text(c, status).FontSize(12).Shrink(0).TextColor(p.MutedForeground)
				}
			}, func() { o.OpenChild(k.ID) })
		}
	})
	rule("rule-activity")
	section("progress", "chat_assets_progress", func() {
		if len(o.Steps) == 0 {
			empty("chat_assets_no_progress")
			return
		}
		done := 0
		for _, st := range o.Steps {
			if st.State == conversation.StepCompleted || st.State == conversation.StepSkipped {
				done++
			}
		}
		ui.Text(c, i18n.T(c, "composer_todo_count", "completed", done, "total", len(o.Steps))).FontSize(14).
			TextColor(p.MutedForeground)
		conversation.Steps(c, o.Steps, o.Active)
	})
	rule("rule-progress")
	asset := func(a OverviewAsset) {
		label := a.Label
		if label == "" {
			label = a.Name
		}
		b := row("asset-"+a.ID, a.Name, func() {
			ui.Icon(c, a.Icon).FontSize(16).Shrink(0)
			name(label)
		}, func() { o.OpenAsset(a.ID) })
		if label != a.Name {
			kit.Tooltip(c, b, a.Name)
		}
	}
	section("output", "chat_assets_output", func() {
		pending()
		for _, a := range o.Output {
			asset(a)
		}
		if len(o.Output) == 0 && !o.Loading && !o.Failed {
			empty("chat_assets_no_output")
		}
	})
	rule("rule-output")
	section("sources", "chat_assets_sources", func() {
		pending()
		for _, a := range o.Sources {
			asset(a)
		}
		for i, source := range o.Citations {
			label := source.Title
			if label == "" {
				label = source.URI
			}
			row("source-"+strconv.Itoa(i), label, func() {
				ui.Icon(c, icons.Globe).FontSize(16).Shrink(0)
				name(label)
			}, func() { o.OpenSource(source.URI) })
		}
		if len(o.Sources)+len(o.Citations) == 0 && !o.Loading && !o.Failed {
			empty("chat_assets_no_sources")
		}
	})
	switch {
	case o.Failed:
		ui.Row(c.Key("retry")).FillWidth().Justify(ui.Center).Children(func() {
			kit.Secondary(c, i18n.T(c, "chat_retry")).OnClick(func() { o.Load(s.more) })
		})
	case o.More && !o.Loading:
		ui.Row(c.Key("more")).FillWidth().Justify(ui.Center).Children(func() {
			kit.Secondary(c, i18n.T(c, "chat_assets_more")).OnClick(func() {
				s.more = true
				o.Load(true)
			})
		})
	}
}
