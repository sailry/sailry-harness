package live

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type legacyDocument struct {
	id, path  string
	lines     int
	pending   *legacyDocumentSave
	uncertain bool
	failure   *protocol.Fault
}

type legacyDocumentSave struct {
	request protocol.Request
	text    string
}

type legacyDocumentReveal struct {
	Document string `json:"document"`
	Sequence uint64 `json:"sequence,string"`
}

type legacyDocumentState struct {
	panel.DocumentEditorState
	ID        string          `json:"id"`
	Path      string          `json:"path"`
	Revision  *string         `json:"revision"`
	Truncated bool            `json:"truncated"`
	Uncertain bool            `json:"uncertain"`
	Error     *protocol.Fault `json:"error"`
}

type legacyDocumentClose struct {
	ctx      context.Context
	document *legacyDocument
	open     bool
	result   chan bool
}

type legacyDocumentView struct {
	feed       legacyFileFeed
	values     []legacyDocumentState
	scratch    []legacyDocumentState
	reveal     legacyDocumentReveal
	openCancel context.CancelFunc
	refreshing bool
	closing    *legacyDocumentClose
}

func (host *legacyFileHost) documentOperations() {
	ops := host.state.native.Operations
	ops["sailry/documents.readDocuments"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		if err := host.check(false); err != nil {
			return nil, err
		}
		host.publishDocuments(nil)
		return host.documents.feed.read(), nil
	}
	ops["sailry/documents.refreshDocuments"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		if err := host.check(false); err != nil {
			return nil, err
		}
		host.refreshDocuments()
		return json.RawMessage("null"), nil
	}
	ops["sailry/documents.nextDocumentChange"] = host.documents.feed.next
	ops["sailry/documents.openDocument"] = host.openDocument
	ops["sailry/documents.documentAction"] = host.documentAction
	ops["sailry/documents.saveDocument"] = host.saveDocument
	ops["sailry/documents.closeDocument"] = host.closeDocument
}

func (host *legacyFileHost) document(id string) *legacyDocument {
	for _, doc := range host.workspace.documents {
		if doc.id == id {
			return doc
		}
	}
	return nil
}

func (host *legacyFileHost) documentState(doc *legacyDocument, write bool) (legacyDocumentState, bool) {
	content := host.project.files[doc.path]
	value, ok := host.project.panel.DocumentEditor(doc.path, write && doc.pending == nil && !doc.uncertain, doc.lines)
	uncertain := doc.uncertain || doc.pending != nil && !value.Saving
	if uncertain && doc.pending != nil {
		value.CanSave = write && !host.project.runtime.documentsLocked()
	}
	return legacyDocumentState{DocumentEditorState: value, ID: doc.id, Path: doc.path, Revision: content.Revision, Truncated: content.Truncated, Uncertain: uncertain, Error: doc.failure}, ok
}

func (host *legacyFileHost) publishDocuments(view *ui.Context) {
	if host.check(false) != nil {
		return
	}
	if view != nil {
		host.renderDocumentClose(view)
	}
	write := host.check(true) == nil
	reveal := host.workspace.reveal
	projection := &host.documents
	values := projection.scratch[:0]
	for _, doc := range host.workspace.documents {
		if value, ok := host.documentState(doc, write); ok {
			values = append(values, value)
		}
	}
	if projection.values != nil && projection.reveal == reveal && slices.EqualFunc(projection.values, values, func(a, b legacyDocumentState) bool { return reflect.DeepEqual(a, b) }) {
		projection.scratch = values
		return
	}
	if values == nil {
		values = []legacyDocumentState{}
	}
	clear(projection.values)
	projection.scratch = projection.values[:0]
	projection.values, projection.reveal = values, reveal
	projection.feed.publish(func(cursor string) any {
		var selected *legacyDocumentReveal
		if reveal.Document != "" {
			selected = &reveal
		}
		return map[string]any{"cursor": cursor, "documents": values, "operations": []any{}, "reveal": selected}
	})
}

func (host *legacyFileHost) readDocument(ctx context.Context, name string) (protocol.FileContent, error) {
	out, err := host.state.execute(ctx, protocol.Command{Kind: "read_file", Data: protocol.CommandReadFilePayload{Worktree: host.worktree, Path: name}})
	if err != nil {
		return protocol.FileContent{}, err
	}
	value, ok := detail[protocol.FileContent](out.Data)
	if !ok || out.Kind != "file_content" || value.Path != name {
		return protocol.FileContent{}, pluginNativeFault(protocol.ErrorCodeInternal, "unexpected document content response")
	}
	return value, nil
}

func (host *legacyFileHost) openDocument(ctx context.Context, args []json.RawMessage) (json.RawMessage, error) {
	name, err := pluginNativeArgument[string](args, 0)
	if err != nil || !legacyFilePath(name) {
		return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid document path")
	}
	var options struct{ Line int }
	if len(args) > 1 && (json.Unmarshal(args[1], &options) != nil || options.Line < 0) {
		return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid document line")
	}
	readCtx, cancel := context.WithCancel(ctx)
	defer cancel()
	if _, err := host.state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
		if err := host.check(false); err != nil {
			return nil, err
		}
		if stop := host.documents.openCancel; stop != nil {
			stop()
		}
		host.documents.openCancel = cancel
		return nil, nil
	}); err != nil {
		return nil, err
	}
	content, err := host.readDocument(readCtx, name)
	if err != nil {
		return nil, obsoleteDocumentRead(err)
	}
	lines := strings.Count(content.Text, "\n") + 1
	value, err := host.state.update(readCtx, func(*ui.Context) (json.RawMessage, error) {
		if err := host.check(false); err != nil {
			return nil, err
		}
		project := host.project
		if project.trashBlocksRead("file/" + name) {
			return nil, pluginNativeFault(protocol.ErrorCodeCancelled, "document was removed")
		}
		var doc *legacyDocument
		for _, existing := range host.workspace.documents {
			if existing.path == name {
				doc = existing
				break
			}
		}
		if doc == nil {
			doc = &legacyDocument{id: protocol.NewRequestID().String(), path: name}
			host.workspace.documents = append(host.workspace.documents, doc)
		}
		doc.lines = lines
		project.fileLoaded(name, content)
		project.panel.OpenDocumentEditor(name, options.Line)
		host.workspace.reveal = legacyDocumentReveal{Document: doc.id, Sequence: host.workspace.reveal.Sequence + 1}
		return host.documentReply(doc)
	})
	return value, obsoleteDocumentRead(err)
}

func obsoleteDocumentRead(err error) error {
	if errors.Is(err, context.Canceled) {
		return pluginNativeFault(protocol.ErrorCodeCancelled, "document read was cancelled")
	}
	return err
}

func (host *legacyFileHost) refreshDocuments() {
	if host.documents.refreshing || len(host.workspace.documents) == 0 {
		return
	}
	type source struct {
		doc      *legacyDocument
		revision *string
	}
	var sources []source
	for _, doc := range host.workspace.documents {
		if doc.pending == nil {
			sources = append(sources, source{doc, host.project.files[doc.path].Revision})
		}
	}
	if len(sources) == 0 {
		return
	}
	host.documents.refreshing = host.state.runtime.async(func() {
		ctx := host.state.native.Context
		for _, source := range sources {
			if ctx.Err() != nil {
				return
			}
			content, err := host.readDocument(ctx, source.doc.path)
			if !host.state.post(func(*ui.Context) {
				doc := source.doc
				p := host.project
				if host.check(false) != nil || host.document(doc.id) != doc || doc.pending != nil || p.files[doc.path].Revision != source.revision || p.trashBlocksRead("file/"+doc.path) {
					return
				}
				if err == nil {
					doc.lines = strings.Count(content.Text, "\n") + 1
					p.fileLoaded(doc.path, content)
				} else if faultCode(err) == protocol.ErrorCodeNotFound {
					if _, dirty := p.panel.FileDraft(doc.path); dirty {
						p.draftSource(doc.path, changedFile{missing: true})
					} else {
						p.fileUnavailable(doc.path)
					}
				}
			}) {
				return
			}
		}
		host.state.post(func(*ui.Context) { host.documents.refreshing = false })
	})
}

func (host *legacyFileHost) documentReply(doc *legacyDocument) (json.RawMessage, error) {
	host.publishDocuments(nil)
	value, ok := host.documentState(doc, host.check(true) == nil)
	if !ok {
		return nil, pluginNativeFault(protocol.ErrorCodeNotFound, "document is not open")
	}
	return pluginNativeJSON(value)
}

func (host *legacyFileHost) documentAction(ctx context.Context, args []json.RawMessage) (json.RawMessage, error) {
	id, err := pluginNativeArgument[string](args, 0)
	if err != nil {
		return nil, err
	}
	action, err := pluginNativeArgument[struct {
		Kind, Block string
		Line        int
	}](args, 1)
	if err != nil {
		return nil, err
	}
	mutates := false
	switch action.Kind {
	case "focus", "source", "document", "copy", "find", "wrap":
	case "reveal":
		if action.Line >= 0 {
			break
		}
		return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid document line")
	case "cut", "paste", "undo", "redo", "replace", "bold", "italic", "strike", "inline_code", "link":
		mutates = true
	case "block", "insert":
		if !legacyDocumentBlock(action.Kind, action.Block) {
			return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid document block")
		}
		mutates = true
		action.Kind += ":" + action.Block
	default:
		return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "unknown document action")
	}
	return host.state.update(ctx, func(view *ui.Context) (json.RawMessage, error) {
		if err := host.check(mutates); err != nil {
			return nil, err
		}
		doc := host.document(id)
		if doc == nil {
			return nil, pluginNativeFault(protocol.ErrorCodeNotFound, "document is outside the captured workspace")
		}
		if value, ok := host.documentState(doc, mutates); !ok || mutates && !value.Editable {
			return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "document cannot be edited")
		}
		host.project.panel.DocumentEditorAction(view, doc.path, action.Kind, max(1, action.Line))
		return host.documentReply(doc)
	})
}

func legacyDocumentBlock(kind, block string) bool {
	if kind == "block" {
		return slices.Contains([]string{"paragraph", "heading1", "heading2", "heading3", "heading4", "heading5", "heading6", "bullet", "ordered", "task", "quote", "callout", "code"}, block)
	}
	return slices.Contains([]string{"table", "image", "rule", "code", "mermaid", "math", "chart", "html"}, block)
}

func (host *legacyFileHost) saveDocument(ctx context.Context, args []json.RawMessage) (json.RawMessage, error) {
	id, err := pluginNativeArgument[string](args, 0)
	if err != nil {
		return nil, err
	}
	var result <-chan error
	_, err = host.state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
		if err := host.check(true); err != nil {
			return nil, err
		}
		doc := host.document(id)
		if doc == nil {
			return nil, pluginNativeFault(protocol.ErrorCodeNotFound, "document is outside the captured workspace")
		}
		result, err = host.startDocumentSave(doc)
		return nil, err
	})
	if err != nil {
		return nil, err
	}
	if result != nil {
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case err := <-result:
			if err != nil {
				return nil, err
			}
		}
	}
	return host.state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
		if err := host.check(false); err != nil {
			return nil, err
		}
		if doc := host.document(id); doc != nil {
			return host.documentReply(doc)
		}
		return nil, pluginNativeFault(protocol.ErrorCodeNotFound, "document is no longer open")
	})
}

func (host *legacyFileHost) startDocumentSave(doc *legacyDocument) (<-chan error, error) {
	p := host.project
	value, ok := host.documentState(doc, true)
	if !ok || value.Saving || p.trashBlocksRead("file/"+doc.path) {
		return nil, pluginNativeFault(protocol.ErrorCodeBusy, "document has an active operation")
	}
	if !value.CanSave {
		if !value.Editable {
			return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "document cannot be saved")
		}
		return nil, nil
	}
	checking := doc.pending != nil
	if !checking {
		text, _ := p.panel.FileDraft(doc.path)
		if value.Revision == nil {
			return nil, pluginNativeFault(protocol.ErrorCodeRevisionConflict, "complete file source is not loaded")
		}
		command := protocol.Command{Kind: "write_file", Data: protocol.CommandWriteFilePayload{Worktree: p.owner.ID, Path: doc.path, Text: text, ExpectedRevision: value.Revision}}
		doc.pending = &legacyDocumentSave{request: host.request(command), text: text}
	}
	save := doc.pending
	p.panel.SetFileStatus(doc.path, true, value.Truncated)
	doc.failure = nil
	result := make(chan error, 1)
	if !p.runtime.async(func() {
		var out protocol.Output
		var err error
		if checking {
			out, err = p.runtime.retryAttachmentRequest(p.runtime.ctx, save.request)
		} else {
			out, err = p.runtime.controller.Settle(p.runtime.ctx, save.request)
		}
		written, valid := detail[protocol.FileWritten](out.Data)
		unexpected := err == nil && (!valid || out.Kind != "file_written" || written.Path != doc.path)
		if unexpected {
			err = pluginNativeFault(protocol.ErrorCodeOutcomeUnknown, "unexpected document write response")
		}
		p.runtime.enqueue(func(view *ui.Context) {
			p.panel.SetFileStatus(doc.path, false, value.Truncated)
			doc.failure = legacyFileFault(err)
			if unexpected || !client.Retained(err) && !(checking && err != nil && !client.Released(err) && faultCode(err) == protocol.ErrorCodeUnavailable) {
				doc.pending = nil
			}
			doc.uncertain = doc.pending == nil && faultCode(err) == protocol.ErrorCodeOutcomeUnknown
			if err == nil {
				source := p.files[doc.path]
				source.Text, source.Revision, source.Size = save.text, &written.Revision, written.Size
				p.files[doc.path] = source
				doc.lines = strings.Count(save.text, "\n") + 1
				p.entryFound(doc.path)
				p.clearChanged(doc.path)
				p.panel.SavedFile(doc.path, save.text)
				p.pendingFiles, p.pendingGit = true, true
				kit.Toast(view, kit.Done, i18n.T(view, "workspace_saved"))
			}
			result <- err
		})
	}) {
		p.panel.SetFileStatus(doc.path, false, value.Truncated)
		return nil, resourceOfflineFault()
	}
	return result, nil
}

func (host *legacyFileHost) closeDocument(ctx context.Context, args []json.RawMessage) (json.RawMessage, error) {
	id, err := pluginNativeArgument[string](args, 0)
	if err != nil {
		return nil, err
	}
	var options struct{ Discard, Confirm bool }
	if len(args) > 1 && json.Unmarshal(args[1], &options) != nil {
		return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid document close options")
	}
	var result <-chan bool
	value, err := host.state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
		if err := host.check(false); err != nil {
			return nil, err
		}
		doc := host.document(id)
		if doc == nil {
			return pluginNativeJSON(false)
		}
		state, _ := host.documentState(doc, false)
		if state.Saving || doc.pending != nil {
			return pluginNativeJSON(false)
		}
		if state.Dirty && !options.Discard {
			if !options.Confirm || host.check(true) != nil || host.documents.closing != nil {
				return pluginNativeJSON(false)
			}
			closing := &legacyDocumentClose{ctx: ctx, document: doc, open: true, result: make(chan bool, 1)}
			host.documents.closing, result = closing, closing.result
			return nil, nil
		}
		if state.Dirty && host.check(true) != nil {
			return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "plugin cannot discard document edits")
		}
		host.dropDocument(doc)
		return pluginNativeJSON(true)
	})
	if err != nil || result == nil {
		return value, err
	}
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case closed := <-result:
		return pluginNativeJSON(closed)
	}
}

func (host *legacyFileHost) dropDocument(doc *legacyDocument) {
	host.project.panel.Files.Close(doc.path)
	host.workspace.documents = slices.DeleteFunc(host.workspace.documents, func(value *legacyDocument) bool { return value == doc })
	if host.workspace.reveal.Document == doc.id {
		host.workspace.reveal.Document = ""
	}
	host.publishDocuments(nil)
}

func (host *legacyFileHost) renderDocumentClose(view *ui.Context) {
	closing := host.documents.closing
	if closing == nil {
		return
	}
	if closing.ctx.Err() != nil || host.check(true) != nil || host.document(closing.document.id) == nil || closing.document.pending != nil {
		host.documents.closing = nil
		closing.result <- false
		return
	}
	choice := kit.Confirm(view.Key("plugin-document-close").Key(closing), &closing.open, kit.Confirmation{
		Title: i18n.T(view, "files_discard_title"), Message: closing.document.path + "\n\n" + i18n.T(view, "files_discard_description"),
		Buttons: []string{i18n.T(view, "settings_cancel"), i18n.T(view, "files_discard")}, Destructive: []int{1},
	})
	if !closing.open {
		host.documents.closing = nil
		if choice == 1 {
			host.dropDocument(closing.document)
		}
		closing.result <- choice == 1
	}
}

func (host *legacyFileHost) renderDocument(view *ui.Context, element plugins.NativeElement, _ func(), _ func(plugins.CallbackRef, json.RawMessage)) (ui.Element, error) {
	_, props, err := pluginNativeProps(element)
	if err != nil {
		return ui.Element{}, err
	}
	var id string
	if err := json.Unmarshal(props["document"], &id); err != nil {
		return ui.Element{}, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid document surface handle")
	}
	doc := host.document(id)
	if doc == nil || host.check(false) != nil {
		return ui.Box(view).Fill(), nil
	}
	return ui.Column(view.Key(id)).Fill().MinWidth(0).MinHeight(0).Children(func() {
		host.project.panel.RenderDocumentEditor(view.Key("editor"), doc.path, func() bool {
			return doc.pending == nil && !doc.uncertain && host.check(true) == nil
		}, func() {
			if _, err := host.startDocumentSave(doc); err != nil {
				doc.failure = legacyFileFault(err)
			}
		})
	}), nil
}
