package shell

import (
	"net/url"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Source is a knowledge citation's identity
// (sailry-knowledge://<collection>/<chunk>?revision=<rev>).
type Source struct {
	Collection, Chunk, Revision string
}

// ParseSource reads a knowledge link, rejecting what the reference rejects:
// another scheme, a user, password or port, a fragment, more path segments,
// and any query but one revision.
func ParseSource(link string) (Source, bool) {
	u, err := url.Parse(link)
	if err != nil || !strings.EqualFold(u.Scheme, "sailry-knowledge") || u.User != nil || u.Port() != "" ||
		u.Fragment != "" || strings.Contains(link, "#") || u.Host == "" {
		return Source{}, false
	}
	chunk := strings.TrimPrefix(u.Path, "/")
	if chunk == "" || strings.Contains(chunk, "/") {
		return Source{}, false
	}
	q, err := url.ParseQuery(u.RawQuery)
	if err != nil || len(q) != 1 || len(q["revision"]) != 1 || q.Get("revision") == "" {
		return Source{}, false
	}
	return Source{Collection: u.Host, Chunk: chunk, Revision: q.Get("revision")}, true
}

// reading is the knowledge viewer's state: the citation and its page, or the
// page a live controller read.
type reading struct {
	open bool
	cite preview.Citation
	page int
	live *Reading
}

// Reading is one page of a citation read from its Node. The controller owns
// it: it replaces the page and the ends as Previous and Next load.
type Reading struct {
	Title, Host, Path, Position, Text string
	First, Last, Pending              bool
	Turn                              func(forward bool)
}

// ShowKnowledge opens the viewer on a page read from a Node.
func (a *App) ShowKnowledge(r *Reading) { a.reading = &reading{open: true, live: r} }

// Reading is the live page the viewer shows, nil once it is closed.
func (a *App) Reading() *Reading {
	if a.reading == nil || !a.reading.open {
		return nil
	}
	return a.reading.live
}

// OpenKnowledge opens a knowledge link of a conversation on its Node.
func (a *App) OpenKnowledge(c *ui.Context, session, link string) {
	node := ""
	if cv := a.Data.Find(session); cv != nil {
		node = cv.Node
	}
	a.openKnowledge(c, node, link)
}

// openKnowledge opens a knowledge link in the viewer, or tells it is not
// one.
func (a *App) openKnowledge(c *ui.Context, node, link string) {
	source, ok := ParseSource(link)
	if !ok {
		kit.Toast(c, kit.Failed, i18n.T(c, "knowledge_citation_invalid"))
		return
	}
	if a.Hooks.OpenKnowledge != nil {
		a.Hooks.OpenKnowledge(c, node, source)
		return
	}
	for _, ct := range a.Data.Citations {
		if ct.Link == link {
			a.reading = &reading{open: true, cite: ct}
			return
		}
	}
	kit.Toast(c, kit.Failed, i18n.T(c, "knowledge_citation_invalid"))
}

// knowledge is the knowledge source viewer: a dialog up to 800 wide with the
// page's title, where it is from, its text, and Previous, Next and Close. The
// preview pages through its citation in memory.
func (a *App) knowledge(c *ui.Context) {
	r := a.reading
	if r == nil || !r.open {
		return
	}
	p := theme.Of(c)
	w, h := c.Size()
	ct := r.cite
	name, path, text := a.hostName(c, ct.Host), ct.Path, ""
	first, last := r.page == 0, r.page >= len(ct.Pages)-1
	previous, next := func() { r.page-- }, func() { r.page++ }
	lines := ""
	if live := r.live; live != nil {
		ct.Title, name, path, lines, text = live.Title, live.Host, live.Path, live.Position, live.Text
		first, last = live.Pending || live.First, live.Pending || live.Last
		previous, next = func() { live.Turn(false) }, func() { live.Turn(true) }
	} else {
		lines = i18n.T(c, "knowledge_lines", "start", ct.Start, "end", ct.End)
		text = ct.Pages[r.page]
	}
	title := i18n.T(c, "knowledge_source")
	kit.Dialog(c.Key("knowledge"), &r.open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		ui.Text(c, ct.Title).FontSize(14).FontWeight(500)
		ui.Text(c, name+" · "+path+" · "+lines).FontSize(12).TextColor(p.MutedForeground)
		ui.Scroll(c.Key("text")).MaxHeight(max(160, h-260)).Padding(12).Radius(p.Radius).Background(p.Muted).Children(func() {
			ui.Text(c, text).FontSize(12).Font("monospace").PreserveSpaces().Selectable()
		})
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("previous"), i18n.T(c, "knowledge_previous")).Disabled(first).OnClick(previous)
			kit.Secondary(c.Key("next"), i18n.T(c, "knowledge_next")).Disabled(last).OnClick(next)
			kit.Secondary(c.Key("close"), i18n.T(c, "knowledge_close")).OnClick(func() { r.open = false })
		})
	}).Width(min(800, w-48)).Label(title)
}

// overlays are the window's search and knowledge viewer.
func (a *App) overlays(c *ui.Context) {
	a.searchOverlay(c)
	a.knowledge(c)
	a.downloadDialog(c)
	a.welcome(c)
	a.selectionDialog(c)
	a.closeDialog(c)
	// The file picker opens from other dialogs, so it is built after them.
	a.Sidebar.Picker.View(c)
}
