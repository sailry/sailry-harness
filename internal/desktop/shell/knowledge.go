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

// reading is the knowledge viewer's state: the citation and its page.
type reading struct {
	open bool
	cite preview.Citation
	page int
}

// openKnowledge opens a knowledge link in the viewer, or tells it is not
// one.
func (a *App) openKnowledge(c *ui.Context, link string) {
	if _, ok := ParseSource(link); !ok {
		kit.Toast(c, kit.Failed, i18n.T(c, "knowledge_citation_invalid"))
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
	title := i18n.T(c, "knowledge_source")
	kit.Dialog(c.Key("knowledge"), &r.open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		ui.Text(c, ct.Title).FontSize(14).FontWeight(500)
		lines := i18n.T(c, "knowledge_lines", "start", ct.Start, "end", ct.End)
		ui.Text(c, i18n.T(c, ct.Host)+" · "+ct.Path+" · "+lines).FontSize(12).TextColor(p.MutedForeground)
		ui.Scroll(c.Key("text")).MaxHeight(max(160, h-260)).Padding(12).Radius(p.Radius).Background(p.Muted).Children(func() {
			ui.Text(c, ct.Pages[r.page]).FontSize(12).Font("monospace").PreserveSpaces().Selectable()
		})
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("previous"), i18n.T(c, "knowledge_previous")).Disabled(r.page == 0).OnClick(func() { r.page-- })
			kit.Secondary(c.Key("next"), i18n.T(c, "knowledge_next")).Disabled(r.page >= len(ct.Pages)-1).OnClick(func() { r.page++ })
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
}
