package live

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// knowledgeViewer pages one citation on the Node its link came from. Every
// page is read again, so a source that changed reports it instead of showing
// stale text.
type knowledgeViewer struct {
	read    func(protocol.KnowledgeCitationRequest, func(*ui.Context, protocol.Output, error))
	shown   func() *shell.Reading
	input   protocol.KnowledgeCitationRequest
	page    protocol.KnowledgeCitationPage
	history []uint32
	view    *shell.Reading
}

// knowledgeRequest is the first page a link asks for. The shell already
// rejected a malformed link; the identifiers are checked here.
func knowledgeRequest(source shell.Source) (protocol.KnowledgeCitationRequest, bool) {
	collection, err := protocol.ParseKnowledgeID(source.Collection)
	input := protocol.KnowledgeCitationRequest{Collection: collection, Chunk: source.Chunk, Revision: source.Revision, Limit: 16 * 1024}
	return input, err == nil && knowledge.ValidateCitation(input) == nil
}

// knowledgeMatches reports whether a page answers its request.
func knowledgeMatches(page protocol.KnowledgeCitationPage, input protocol.KnowledgeCitationRequest) bool {
	citation := page.Citation
	return citation.Collection == input.Collection && citation.Chunk == input.Chunk && citation.Revision == input.Revision &&
		citation.Link == knowledge.CitationURL(citation) && page.Offset == input.Offset && page.Context == input.Context &&
		page.Untrusted && len(page.Text) <= int(input.Limit) &&
		(page.NextOffset == nil || *page.NextOffset > input.Offset && uint64(*page.NextOffset) == uint64(input.Offset)+uint64(len(page.Text)))
}

func knowledgeErrorKey(code protocol.ErrorCode) string {
	switch code {
	case protocol.ErrorCodeRevisionConflict, protocol.ErrorCodeNotFound:
		return "knowledge_citation_changed"
	case protocol.ErrorCodePermissionDenied:
		return "knowledge_denied"
	case protocol.ErrorCodeInvalidRequest:
		return "knowledge_citation_invalid"
	}
	return "knowledge_citation_unavailable"
}

func knowledgePosition(c *ui.Context, position protocol.KnowledgePosition) string {
	if value, ok := detail[protocol.KnowledgePositionLinesPayload](position.Data); ok {
		return i18n.T(c, "knowledge_lines", "start", value.Start, "end", value.End)
	}
	if value, ok := detail[protocol.KnowledgePositionPagePayload](position.Data); ok {
		return i18n.T(c, "knowledge_page", "page", value.Page)
	}
	if value, ok := detail[protocol.KnowledgePositionCellsPayload](position.Data); ok {
		return i18n.T(c, "knowledge_cells", "sheet", value.Sheet, "start", value.Start, "end", value.End)
	}
	if value, ok := detail[protocol.KnowledgePositionSlidePayload](position.Data); ok {
		return i18n.T(c, "knowledge_slide", "slide", value.Slide)
	}
	if value, ok := detail[protocol.KnowledgePositionSectionPayload](position.Data); ok {
		return value.Name
	}
	value, _ := detail[protocol.KnowledgePositionValuePayload](position.Data)
	return value.Key + value.Pointer
}

// knowledgePage decodes one answer, or names the toast its failure shows.
func knowledgePage(output protocol.Output, err error, input protocol.KnowledgeCitationRequest) (protocol.KnowledgeCitationPage, string) {
	if err != nil {
		if code := faultCode(err); code != "" {
			return protocol.KnowledgeCitationPage{}, knowledgeErrorKey(code)
		}
		return protocol.KnowledgeCitationPage{}, "knowledge_citation_unavailable"
	}
	page, ok := detail[protocol.KnowledgeCitationPage](output.Data)
	if !ok || !knowledgeMatches(page, input) {
		return page, "knowledge_citation_unavailable"
	}
	return page, ""
}

func (v *knowledgeViewer) show(c *ui.Context, input protocol.KnowledgeCitationRequest, page protocol.KnowledgeCitationPage) {
	v.input, v.page = input, page
	v.view.Title, v.view.Path, v.view.Text = page.Title, page.Citation.Path, page.Text
	v.view.Position = knowledgePosition(c, page.Citation.Position)
	v.view.First, v.view.Last = len(v.history) == 0, page.NextOffset == nil
}

// turn loads the adjacent page; the viewer keeps its page when that fails.
func (v *knowledgeViewer) turn(forward bool) {
	if v.view.Pending {
		return
	}
	input := v.input
	switch {
	case forward && v.page.NextOffset != nil:
		input.Offset = *v.page.NextOffset
	case !forward && len(v.history) > 0:
		input.Offset = v.history[len(v.history)-1]
	default:
		return
	}
	v.view.Pending = true
	v.read(input, func(c *ui.Context, output protocol.Output, err error) {
		v.view.Pending = false
		if v.shown() != v.view {
			return
		}
		page, failure := knowledgePage(output, err, input)
		if failure != "" {
			kit.Toast(c, kit.Failed, i18n.T(c, failure))
			return
		}
		if forward {
			v.history = append(v.history, v.page.Offset)
		} else {
			v.history = v.history[:len(v.history)-1]
		}
		v.show(c, input, page)
	})
}

// openKnowledge reads a citation on the Node that named it and opens the
// viewer on it. An identity the Node could not hold never sends a command.
func (r *Runtime) openKnowledge(c *ui.Context, node string, source shell.Source) {
	root := r.localRuntime()
	input, valid := knowledgeRequest(source)
	if !valid {
		kit.Toast(c, kit.Failed, i18n.T(c, "knowledge_citation_invalid"))
		return
	}
	owner := root.runtimeFor(node)
	if owner == nil {
		kit.Toast(c, kit.Failed, i18n.T(c, "knowledge_citation_unavailable"))
		return
	}
	read := func(input protocol.KnowledgeCitationRequest, done func(*ui.Context, protocol.Output, error)) {
		owner.execute(protocol.Command{Kind: "read_knowledge_citation", Data: input}, done)
	}
	read(input, func(c *ui.Context, output protocol.Output, err error) {
		page, failure := knowledgePage(output, err, input)
		if failure != "" {
			kit.Toast(c, kit.Failed, i18n.T(c, failure))
			return
		}
		viewer := &knowledgeViewer{read: read, shown: root.App.Reading, view: &shell.Reading{}}
		viewer.view.Turn = viewer.turn
		viewer.view.Host = i18n.T(c, "local_host_name")
		if owner != root {
			viewer.view.Host = node
			if state := root.hostState(node); state != nil {
				viewer.view.Host = state.view.Name
			}
		}
		viewer.show(c, input, page)
		root.App.ShowKnowledge(viewer.view)
	})
}

// openChatLink takes the knowledge links of a reply.
func (r *Runtime) openChatLink(c *ui.Context, session, link string) bool {
	scheme, _, found := strings.Cut(link, ":")
	if !found || !strings.EqualFold(scheme, "sailry-knowledge") {
		return false
	}
	r.App.OpenKnowledge(c, session, link)
	return true
}
