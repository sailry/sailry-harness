package panel

import (
	"math"
	"slices"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/ELY-Open-Source/CrychicUI/code"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/kit/crychic"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// history is the commits, newest first, as a graph: CrychicUI's CommitGraph
// draws the lanes, the merges and each commit's line, and a row of ours
// over each takes the click, the selection and the right-click menu, which
// the graph's own rows do not. A history the graph cannot draw (a Node that
// sends no parents, a parent listed before its child) is the plain list.
// The Node reads it a page at a time and the list grows by Load more at
// its end: the graph draws a line only between commits of one list, so the
// lines run on from page to page.
func (pr *Project) history(c *ui.Context, own bool) {
	p := theme.Of(c)
	v := &pr.Git
	repo := &pr.Data.Repo
	if pr.Actions != nil && v.log.Loading && len(repo.Commits) == 0 {
		ui.Column(c).Grow(1).Center().Children(func() { kit.Loader(c, 20) })
		return
	}
	if len(repo.Commits) == 0 {
		kit.Result(c, kit.ResultProps{Icon: icons.Clock, Title: i18n.T(c, "git_history_empty")}).Grow(1).Justify(ui.Center)
		return
	}
	ui.Scroll(c.Key("history")).TrackScroll(scroll(&v.history, own)).Grow(1).Padding(8, graphInset).Children(func() {
		if !pr.historyGraph(c) {
			pr.historyList(c)
		}
		if pr.Actions == nil {
			return
		}
		if v.log.Partial {
			ui.Text(c, i18n.T(c, "git_history_partial")).FontSize(12).Padding(8).TextColor(p.MutedForeground)
		}
		switch {
		case v.log.Loading:
			ui.Row(c.Key("more")).FillWidth().Height(32).Center().Children(func() { kit.Loader(c, 16) })
		case v.log.Next:
			label := i18n.T(c, "files_load_more")
			kit.Ghost(c.Key("more"), label, false).FillWidth().Height(32).TextColor(p.MutedForeground).Children(func() {
				ui.Text(c, label).FontSize(13).SingleLine()
			}).OnClick(func() {
				if pr.Actions.GitHistory != nil {
					pr.Actions.GitHistory()
				}
			})
		}
	})
}

// pickCommit chooses a commit of the history and opens its changes.
func (pr *Project) pickCommit(id string) {
	pr.Git.commitID = id
	if pr.Actions != nil && pr.Actions.ReadCommit != nil {
		pr.Actions.ReadCommit(id)
	}
}

// commitRow makes b a commit's row: chosen by a click, with the commit's
// right-click menu, the system's.
func (pr *Project) commitRow(c *ui.Context, b ui.Element, cm preview.Commit) ui.Element {
	g := pr.menus(c)
	return b.Cursor(ui.CursorPointer).AccessSelected(pr.Git.commitID == cm.ID).
		ContextMenu(func(m *ui.Menu) { fillNative(c, m, g.commitActions(cm)) }).
		OnClick(func() { pr.pickCommit(cm.ID) })
}

// historyGraph draws the history as a graph and reports whether it could.
func (pr *Project) historyGraph(c *ui.Context) bool {
	p := theme.Of(c)
	v := &pr.Git
	repo := &pr.Data.Repo
	lanes, ok := graphLanes(repo.Commits)
	if !ok {
		return false
	}
	now := pr.now(c)
	// What a row has for its text: the list's width less the scroll's
	// padding, the graph's own, the lanes, and the short ID with its gap.
	// A narrow row is the subject's: at most one name, the branch checked
	// out first, in up to half of the line, with the count of the others.
	// With more room the names that fit show, and a wide row has the
	// author and the commit's age after the subject. The tooltip says all
	// of it at any width.
	gutter := crychic.CommitGraphPad(c) + crychic.CommitGraphLanes(c, slices.Max(lanes)+1)
	text := v.side - 2*graphInset - gutter - crychic.CommitGraphPad(c) - 56
	wide := text >= graphWide
	most := 1
	if text >= graphNames {
		most = math.MaxInt
	}
	commits := make([]code.GraphCommit, len(repo.Commits))
	for i, cm := range repo.Commits {
		subject, _, _ := strings.Cut(cm.Message, "\n")
		k := code.GraphCommit{Hash: cm.ID, Message: strings.TrimSpace(subject), Lane: lanes[i], Parents: cm.Parents}
		labels := refLabels(cm.Refs, repo.Branch)
		if wide {
			k.Author = cm.Author
			if !cm.Time.IsZero() {
				k.Author = strings.TrimPrefix(cm.Author+" · "+ago(c, cm.Time, now), " · ")
			}
			k.Refs = fitRefs(labels, (text-6*float32(utf8.RuneCountInString(k.Author)))/2, most)
		} else {
			k.Refs = fitRefs(labels, text/2, most)
		}
		commits[i] = k
	}
	top, height := crychic.CommitGraphRows(c)
	// The fills are painted under the graph, by the rows built over it, and
	// start after the lanes: a dot keeps the list's background around it,
	// which a fill under it would show as a ring.
	selected, hovered := -1, -1
	box := ui.Column(c.Key("graph")).FillWidth()
	box.Draw(func(pt *ui.Painter, r ui.Rect) {
		row := func(i int, fill ui.Color) {
			if i >= 0 {
				pt.Fill(ui.Rect{X: r.X + gutter - 6, Y: r.Y + top + float32(i)*height + 2, W: r.W - gutter + 6, H: height - 4}, fill, p.Radius)
			}
		}
		if hovered != selected {
			row(hovered, p.Hover)
		}
		row(selected, p.Selected())
	})
	built := false
	box.Children(func() {
		if _, built = crychic.CommitGraph(c, i18n.T(c, "git_view_history"), commits); !built {
			return
		}
		// The rows are the options of a list to choose from; the graph under
		// them is the same commits as a list to read.
		ui.Column(c.Key("rows")).Absolute().Top(0).Left(0).Right(0).PaddingY(top).Role(ui.RoleListBox).Label(i18n.T(c, "git_view_history")).Children(func() {
			for i, cm := range repo.Commits {
				b := pr.commitRow(c, ui.ButtonBase(c.Key(cm.ID)).Role(ui.RoleOption).Label(commits[i].Message).FillWidth().Height(height).Radius(p.Radius), cm)
				kit.Tooltip(c, b, pr.commitTip(c, cm, now))
				if v.commitID == cm.ID {
					selected = i
				}
				if b.Hovered() {
					hovered = i
				}
			}
		})
	})
	return built
}

// graphNames is the least room for a row's text at which every name that
// fits shows, and graphWide the least at which the author and the age show
// after the subject; narrower, the row's tooltip says them. graphInset is
// the history's padding at each side.
const (
	graphNames = 200
	graphWide  = 340
	graphInset = 4
)

// now is the clock a commit's age is told against: the frame's, or the
// fixture's own in the preview.
func (pr *Project) now(c *ui.Context) time.Time {
	if pr.Actions != nil {
		return c.Now()
	}
	return preview.Now
}

// commitTip is a commit's row in full, for the tooltip: a narrow list cuts
// the subject short and leaves the author, the age and names out.
func (pr *Project) commitTip(c *ui.Context, cm preview.Commit, now time.Time) string {
	subject, _, _ := strings.Cut(cm.Message, "\n")
	parts := []string{cm.ID[:min(8, len(cm.ID))]}
	if cm.Author != "" {
		parts = append(parts, cm.Author)
	}
	if !cm.Time.IsZero() {
		parts = append(parts, ago(c, cm.Time, now))
	}
	if refs := refLabels(cm.Refs, pr.Data.Repo.Branch); len(refs) > 0 {
		parts = append(parts, strings.Join(refs, ", "))
	}
	return strings.TrimSpace(subject) + "\n" + strings.Join(parts, " · ")
}

// graphLanes gives each commit of a history, newest first, its lane. A lane
// is kept for the commit it awaits: a commit takes the lane awaiting it,
// else the first free one; its first parent then continues its lane,
// unless another child already awaits that parent, where the line joins
// that lane; a merge's other parents each take a free lane. The graph
// draws the line to a parent not listed yet straight down the commit's own
// lane to the end, so that lane is kept for it and nothing is put in its
// way. Nothing but the awaited commit is ever put in a lane, so each line
// the graph draws down a lane ends at the next dot in it. It reports false
// when a commit's parents are not known.
func graphLanes(commits []preview.Commit) ([]int, bool) {
	listed := make(map[string]bool, len(commits))
	for _, cm := range commits {
		if cm.Parents == nil {
			return nil, false
		}
		listed[cm.ID] = true
	}
	lanes := make([]int, len(commits))
	var awaited []string // by lane, "" while free
	take := func(id string) int {
		for l, a := range awaited {
			if a == "" {
				awaited[l] = id
				return l
			}
		}
		awaited = append(awaited, id)
		return len(awaited) - 1
	}
	for i, cm := range commits {
		lane := slices.Index(awaited, cm.ID)
		if lane < 0 {
			lane = take(cm.ID)
		}
		lanes[i] = lane
		awaited[lane] = ""
		for n, parent := range cm.Parents {
			switch {
			case !listed[parent]:
				if awaited[lane] == "" {
					awaited[lane] = parent
				}
			case slices.Contains(awaited, parent):
			case n == 0:
				awaited[lane] = parent
			default:
				take(parent)
			}
		}
	}
	return lanes, true
}

// refLabels are the names at a commit as the history shows them: the
// branch checked out first, or HEAD when it is detached there, then the
// tags, the other branches and the remote ones, each once.
func refLabels(refs []string, current string) []string {
	var head, tags, heads, remotes, rest []string
	detached := false
	for _, r := range refs {
		kind, name, _ := strings.Cut(r, "/")
		switch {
		case r == "HEAD":
			detached = true
		case name == "":
			rest = append(rest, r)
		case kind == "heads" && name == current:
			head = []string{name}
		case kind == "heads":
			heads = append(heads, name)
		case kind == "tags":
			tags = append(tags, name)
		case kind == "remotes":
			remotes = append(remotes, name)
		default:
			rest = append(rest, name)
		}
	}
	if detached && head == nil {
		head = []string{"HEAD"}
	}
	var out []string
	for _, name := range slices.Concat(head, tags, heads, remotes, rest) {
		if !slices.Contains(out, name) {
			out = append(out, name)
		}
	}
	return out
}

// fitRefs are the names a row has room for, room DIPs wide and at most
// most of them: the graph draws every name it is given whole, on one line
// with the subject. The first name is cut short when it does not fit, and
// the names left out are counted after the ones shown while there is room
// for the count.
func fitRefs(labels []string, room float32, most int) []string {
	const more = 34 // the width of "+N"
	var out []string
	for i, label := range labels {
		if i >= most {
			break
		}
		if w := chipWidth(label); w <= room {
			out, room = append(out, label), room-w
			continue
		}
		if cut := cutTo(label, room); i == 0 && cut != "" {
			out, room = append(out, cut), room-chipWidth(cut)
		}
		break
	}
	if left := len(labels) - len(out); left > 0 && len(out) > 0 && room >= more {
		out = append(out, "+"+strconv.Itoa(left))
	}
	return out
}

// chipWidth estimates the width of a name's chip with the gap after it: the
// graph's small bold text, wide for CJK.
func chipWidth(label string) float32 {
	w := float32(24)
	for _, r := range label {
		if r >= 0x2E80 {
			w += 10
		} else {
			w += 6
		}
	}
	return w
}

// cutTo is label cut to fit room with an ellipsis, "" when under six
// characters of it would show.
func cutTo(label string, room float32) string {
	runes := []rune(label)
	for n := len(runes) - 1; n >= 6; n-- {
		if cut := string(runes[:n]) + "…"; chipWidth(cut) <= room {
			return cut
		}
	}
	return ""
}

// historyList is the page as plain rows, where there is no graph to draw:
// a track with a dot, the subject, and the short ID, author and branch.
func (pr *Project) historyList(c *ui.Context) {
	p := theme.Of(c)
	v := &pr.Git
	repo := &pr.Data.Repo
	for _, cm := range repo.Commits {
		subject, _, _ := strings.Cut(cm.Message, "\n")
		b := pr.commitRow(c, ui.ButtonBase(c.Key(cm.ID)).Role(ui.RoleOption).Label(subject).FillWidth().PaddingX(8).Gap(8).Radius(p.Radius).TextColor(p.MutedForeground), cm)
		switch {
		case v.commitID == cm.ID:
			b.Background(p.Selected()).TextColor(p.HoverForeground)
		case b.Hovered():
			b.TextColor(p.Foreground)
		}
		b.Children(func() {
			ui.Column(c.Key("track")).Width(12).MinHeight(40).AlignSelf(ui.Stretch).Shrink(0).Center().Children(func() {
				ui.Box(c).Absolute().Left(5.5).Top(0).Bottom(0).Width(1).Background(p.MutedForeground)
				ui.Box(c).Size(8, 8).Radius(4).Background(p.MutedForeground)
			})
			ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(4).PaddingY(6).Children(func() {
				ui.Text(c, subject).FontSize(14).SingleLine()
				ui.Row(c).Gap(8).MinWidth(0).TextColor(p.MutedForeground).Children(func() {
					field := func(icon *ui.SVG, value string, shrink float32) {
						ui.Row(c).Gap(4).MinWidth(0).Shrink(shrink).Children(func() {
							ui.Icon(c, icon).FontSize(12).Shrink(0)
							ui.Text(c, value).FontSize(12).SingleLine().Shrink(1).MinWidth(0)
						})
					}
					field(icons.Hash, cm.ID[:min(8, len(cm.ID))], 0)
					field(icons.User, cm.Author, 1)
					if branch := commitBranch(cm, repo.Branch); branch != "" {
						field(icons.GitBranch, branch, 1)
					}
				})
			})
		})
	}
}
