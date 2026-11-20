package live

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// documentSource is a session document as a view shows it: the revision
// read, the runtime that read it and the conversation whose history shows
// it. Only that conversation's own document can take form answers.
type documentSource struct {
	content      protocol.ArtifactContent
	owner        *Runtime
	conversation protocol.SessionID
	// page names a worktree HTML file's preview, which shows as written in
	// the same sandbox and takes no answers; "" for a session document.
	page string
}

// id is the key the source's WebView is kept under.
func (s documentSource) id() string {
	if s.page != "" {
		return s.page
	}
	return s.content.Artifact.ID.String()
}

// same reports whether two sources show the same page.
func (s documentSource) same(other documentSource) bool {
	return s.page == other.page && s.owner == other.owner && s.conversation == other.conversation && s.content.Artifact == other.content.Artifact &&
		s.content.LatestRevision == other.content.LatestRevision && s.content.Html == other.content.Html && (s.content.View == nil) == (other.content.View == nil)
}

// documentView is one session document's sandboxed page.
type documentView struct {
	view   *mygo.WebView
	source documentSource
	shown  bool // the last frame placed it
	// writable and theme are what the page was last told.
	writable bool
	theme    string
	// stop ends the relay of the page loaded last.
	stop context.CancelFunc
}

// close ends the page and its relay.
func (d *documentView) close() {
	if d.stop != nil {
		d.stop()
	}
	d.view.Destroy()
}

// documentOptions is the page a session document renders in: untrusted, so it
// calls nothing of the app, with storage of its own that ends with it.
func documentOptions() mygo.PageOptions {
	return mygo.PageOptions{Untrusted: true, Session: mygo.NewEphemeralSession(), DevTools: mygo.DevToolsDisabled}
}

// documentNavigation reports whether a document's page may go to an address:
// only the blank pages the wrapper and its frame are.
func documentNavigation(address string) bool {
	return address == "about:blank" || address == "about:srcdoc"
}

// documentDraftKey names the unsent answers of one document revision.
type documentDraftKey struct {
	artifact protocol.ArtifactID
	revision uint32
}

// documentWritable reports whether a document's forms can submit: its own
// conversation shows its latest revision while the Node is connected.
func documentWritable(source documentSource) bool {
	content := source.content
	return source.owner != nil && source.conversation == content.Artifact.Session && content.Artifact.Revision == content.LatestRevision &&
		source.owner.controller.Connected()
}

// documentMount is what a source's renderer mounts with in this frame.
func (r *Runtime) documentMount(c *ui.Context, source documentSource) documentMount {
	mount := documentMount{
		Theme: documentTheme(theme.Of(c), c.Theme().FontSize),
		Labels: map[string]string{"contents": i18n.T(c, "artifact_contents"), "submit": i18n.T(c, "send"), "close": i18n.T(c, "artifact_close"),
			"required": i18n.T(c, "artifact_form_required"), "invalid_form": i18n.T(c, "artifact_form_invalid")},
		Writable: documentWritable(source),
	}
	key := documentDraftKey{source.content.Artifact.ID, source.content.Artifact.Revision}
	for _, draft := range r.browser.drafts[key] {
		mount.Drafts = append(mount.Drafts, draft)
	}
	return mount
}

// documentView renders a session document, or a worktree page, in its sandbox. It reports false
// where no WebView can show it, and the caller keeps its fallback.
func (r *Runtime) documentView(c *ui.Context, source documentSource) bool {
	root := r.localRuntime()
	state := root.browser
	if state == nil || !state.available {
		return false
	}
	id := source.id()
	document := state.documents[id]
	if document == nil || !document.source.same(source) {
		mount := root.documentMount(c, source)
		// The WebView is made, or loads the new revision, before the next
		// frame builds.
		root.enqueue(func(*ui.Context) {
			document := state.documents[id]
			if document != nil && !document.view.IsDestroyed() {
				if !document.source.same(source) {
					root.loadDocument(document, source, mount)
				}
				return
			}
			view, err := state.window.NewWebView(mygo.WebViewOptions{Page: documentOptions()})
			if err != nil {
				state.available = false
				root.Report(err)
				return
			}
			page := view.Page()
			page.SetWindowOpenHandler(func(mygo.WindowOpenRequest) *mygo.WindowOptions { return nil })
			page.OnWillNavigate(func(event *mygo.NavigateEvent) {
				if !documentNavigation(event.URL) {
					event.PreventDefault()
				}
			})
			// Shown until the frame that follows decides.
			document = &documentView{view: view, shown: true}
			page.OnDidFinishLoad(func() { root.enqueue(func(*ui.Context) { root.relayDocument(document) }) })
			root.loadDocument(document, source, mount)
			state.documents[id] = document
		})
		if document == nil {
			ui.Row(c.Key("document")).Grow(1).Center().Children(func() { kit.Loader(c, 20) })
			return true
		}
	}
	document.shown = true
	root.syncDocument(c, document)
	ui.WebView(c.Key("document"), document.view).Grow(1)
	return true
}

// loadDocument shows a source's page. A renderer that cannot compose its
// page leaves the plain fragment.
func (r *Runtime) loadDocument(document *documentView, source documentSource, mount documentMount) {
	if document.stop != nil {
		document.stop()
		document.stop = nil
	}
	frame, err := documentFrame(source.content, mount)
	plain := source.content.View == nil
	if err != nil {
		r.Report(err)
		source.content.View, plain = nil, true
		frame, _ = documentFrame(source.content, mount)
	}
	theme, _ := json.Marshal(mount.Theme)
	document.source, document.writable, document.theme = source, mount.Writable, string(theme)
	document.view.Page().LoadHTML(documentPage(frame, plain), "about:blank")
}

// relayDocument takes the events the loaded page posts until it loads
// again or closes. One evaluation waits at a time, so the page cannot
// raise more work than Desktop asks for.
func (r *Runtime) relayDocument(document *documentView) {
	if document.view.IsDestroyed() || document.source.content.View == nil {
		return
	}
	if document.stop != nil {
		document.stop()
	}
	ctx, cancel := context.WithCancel(r.ctx)
	document.stop = cancel
	page := document.view.Page()
	if !r.observe(func() {
		for ctx.Err() == nil {
			value, err := page.EvalContext(ctx, "window.sailryHost.next()")
			if err != nil {
				return
			}
			items, _ := value.([]any)
			for _, item := range items {
				if raw, ok := item.(string); ok {
					r.enqueue(func(c *ui.Context) {
						if ctx.Err() == nil {
							r.documentEvent(c, document, raw)
						}
					})
				}
			}
		}
	}) {
		cancel()
	}
}

// syncDocument tells a page what changed since it loaded: whether its forms
// can submit, and the theme.
func (r *Runtime) syncDocument(c *ui.Context, document *documentView) {
	if document.source.content.View == nil {
		return
	}
	if writable := documentWritable(document.source); writable != document.writable {
		document.writable = writable
		r.postDocument(document, map[string]any{"kind": "writable", "writable": writable})
	}
	palette, _ := json.Marshal(documentTheme(theme.Of(c), c.Theme().FontSize))
	if string(palette) != document.theme {
		document.theme = string(palette)
		r.postDocument(document, map[string]any{"kind": "theme", "theme": json.RawMessage(palette)})
	}
}

// postDocument sends a message into the page's frame through its relay.
func (r *Runtime) postDocument(document *documentView, message any) {
	if document.view.IsDestroyed() {
		return
	}
	encoded, err := json.Marshal(message)
	if err != nil {
		return
	}
	argument, _ := json.Marshal(string(encoded))
	page := document.view.Page()
	r.async(func() { _, _ = page.Eval("window.sailryHost?.send(" + string(argument) + ")") })
}

// documentEvent handles one event of the page as loaded for its source.
// The page is untrusted: Desktop decodes and checks everything it uses.
func (r *Runtime) documentEvent(c *ui.Context, document *documentView, raw string) {
	source := document.source
	event, ok := decodeDocumentEvent(raw, source.content.Artifact)
	if !ok {
		return
	}
	key := documentDraftKey{source.content.Artifact.ID, source.content.Artifact.Revision}
	switch event.Kind {
	case "draft":
		draft, err := documentFormDraft(event)
		if err != nil {
			return
		}
		drafts := r.browser.drafts[key]
		if drafts == nil {
			if len(r.browser.drafts) >= 64 {
				clear(r.browser.drafts)
			}
			drafts = map[string]json.RawMessage{}
			r.browser.drafts[key] = drafts
		}
		if _, exists := drafts[event.Form]; exists || len(drafts) < documentFormFields {
			drafts[event.Form] = draft
		}
	case "required":
		kit.Toast(c, kit.Failed, i18n.T(c, "artifact_form_required"))
	case "invalid_form":
		kit.Toast(c, kit.Failed, i18n.T(c, "artifact_form_invalid"))
	case "submit":
		r.submitDocumentForm(c, document, event)
	}
}

// submitDocumentForm sends a form's answers to the document's conversation
// as an ordinary user message, never as an approval. A stale revision or
// an invalid or incomplete form sends nothing.
func (r *Runtime) submitDocumentForm(c *ui.Context, document *documentView, event documentEvent) {
	source := document.source
	content := source.content
	acknowledge := func(accepted bool) {
		if document.source.content.Artifact == content.Artifact {
			r.postDocument(document, map[string]any{"kind": "submitted", "form": event.Form, "accepted": accepted})
		}
	}
	if content.Artifact.Revision != content.LatestRevision {
		kit.Toast(c, kit.Failed, i18n.T(c, "artifact_form_stale"))
		acknowledge(false)
		return
	}
	if !documentWritable(source) {
		kit.Toast(c, kit.Failed, i18n.T(c, "unavailable"))
		acknowledge(false)
		return
	}
	reply, err := documentFormReply(i18n.Match(c.Locale()), content, event)
	if err != nil {
		key := "artifact_form_invalid"
		if errors.Is(err, errDocumentFormRequired) {
			key = "artifact_form_required"
		}
		kit.Toast(c, kit.Failed, i18n.T(c, key))
		acknowledge(false)
		return
	}
	// The revision read may have been replaced since; the Node's latest
	// visible revision decides.
	owner := source.owner
	artifact := content.Artifact
	if !owner.async(func() {
		output, err := owner.controller.Do(r.ctx, protocol.Command{Kind: "list_artifact_revisions", Data: protocol.CommandListArtifactRevisionsPayload{
			ID: artifact.ID, Limit: 1, Session: &source.conversation,
		}})
		page, valid := detail[protocol.ArtifactRevisionPage](output.Data)
		r.enqueue(func(c *ui.Context) {
			switch {
			case err != nil || output.Kind != "artifact_revisions" || !valid || len(page.Items) == 0:
				if err == nil {
					err = errors.New("unexpected artifact revisions response")
				}
				r.reportKeys(c, err, "chat_action_failed", "session_changed", "chat_action_unknown")
				acknowledge(false)
			case !documentLatest(artifact, page):
				kit.Toast(c, kit.Failed, i18n.T(c, "artifact_form_stale"))
				acknowledge(false)
			default:
				r.sendDocumentReply(c, source.conversation, reply, func(accepted bool) {
					if accepted {
						delete(r.browser.drafts[documentDraftKey{artifact.ID, artifact.Revision}], event.Form)
					}
					acknowledge(accepted)
				})
			}
		})
	}) {
		acknowledge(false)
	}
}

// documentLatest reports whether a revision is the latest the Node shows
// its conversation.
func documentLatest(artifact protocol.ArtifactSummary, page protocol.ArtifactRevisionPage) bool {
	return len(page.Items) > 0 && page.Items[0].Revision == artifact.Revision
}

// documentReplyDraft stands for no composer draft, so a sent reply leaves
// the user's draft alone and a failed one is kept for recovery.
const documentReplyDraft = "\x00document-reply"

// sendDocumentReply sends text to a conversation as its composer sends a
// message with the composer's current model and mode, queued behind an
// active run. done hears whether the Node admitted it.
func (r *Runtime) sendDocumentReply(c *ui.Context, session protocol.SessionID, text string, done func(bool)) {
	root := r.localRuntime()
	cv := root.App.Data.Find(session.String())
	if cv == nil {
		kit.Toast(c, kit.Failed, i18n.T(c, "unavailable"))
		done(false)
		return
	}
	comp := root.App.Composer(cv.ID)
	owner := root.conversationRuntime(cv, comp)
	actions := &conversationActions{r: owner, c: c, cv: cv, comp: comp, settled: done}
	if actions.readonly() {
		actions.unavailable()
		done(false)
		return
	}
	// submit drops a send these refuse without settling it.
	if current, exists := owner.session(cv.ID); !exists || current.Profile != nil && current.Profile.Source != owner.controller.Target() {
		actions.unavailable()
		done(false)
		return
	}
	message := comp.Reply(owner.models(), text)
	if _, _, err := owner.composerConfig(comp, message); err != nil {
		owner.report(c, err, "session_changed")
		done(false)
		return
	}
	_, running := actions.activeRun()
	actions.submit(message, false, running, documentReplyDraft)
}

// sessionDocumentView renders a panel document tab from the HTML read for it.
func (r *Runtime) sessionDocumentView(c *ui.Context, id string) bool {
	if r.browser == nil {
		return false
	}
	source, read := r.browser.sources[id]
	return read && r.documentView(c, source)
}

// storeDocument keeps the HTML read for a panel document tab of a
// conversation.
func (r *Runtime) storeDocument(id string, session protocol.SessionID, content protocol.ArtifactContent) {
	if root := r.localRuntime(); root.browser != nil {
		root.browser.sources[id] = documentSource{content: content, owner: r, conversation: session}
	}
}
