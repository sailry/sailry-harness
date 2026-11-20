package live

import (
	"encoding/json"
	"errors"
	"html"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const documentFormFixture = `<section data-title="Plan"><p>Body</p>` +
	`<form class="brief-form" data-name="decisions">` +
	`<fieldset data-name="approach" data-required=""><legend>Approach</legend>` +
	`<label><input type="radio" name="approach" value="a"> Approach A</label>` +
	`<label><input type="radio" name="approach" value="b"> Approach B</label></fieldset>` +
	`<fieldset data-name="note"><legend>Note</legend><textarea name="note"></textarea></fieldset>` +
	`<button type="submit">Submit</button></form></section>`

func documentFormContent() protocol.ArtifactContent {
	artifact := protocol.DefaultArtifactSummary()
	artifact.ID, artifact.Session, artifact.Revision, artifact.Title = protocol.NewArtifactID(), protocol.SessionID{7}, 3, "Plan %{id}"
	return protocol.ArtifactContent{Artifact: artifact, LatestRevision: 3, Html: documentFormFixture}
}

func documentFormEvent(content protocol.ArtifactContent, kind, form, fields string) documentEvent {
	return documentEvent{Kind: kind, Artifact: content.Artifact.ID.String(), Revision: content.Artifact.Revision,
		Session: content.Artifact.Session.String(), Form: form, Fields: json.RawMessage(fields)}
}

// TestDocumentFrameComposesPackageRenderer covers the frame a renderer's
// document mounts in: its policy and styles in the head, the bridge shim
// before the package's scripts, and the fragment only as mount input.
func TestDocumentFrameComposesPackageRenderer(t *testing.T) {
	t.Parallel()
	content := documentFormContent()
	content.Html = `<p>x</p></script><script>alert(1)</script>`
	content.View = &protocol.ArtifactViewContent{
		Template: `<!doctype html><html><head><title></title></head><body><main id="brief"></main></body></html>`,
		Styles:   []string{"main{color:red}</STYLE><p>bad</p>"},
		Scripts:  []string{"window.first=1", "window.second='</script>'"},
	}
	frame, err := documentFrame(content, documentMount{Theme: map[string]string{"background": "#ffffffff"}, Writable: true})
	if err != nil {
		t.Fatal(err)
	}
	if strings.ContainsAny(frame, `<>"`) {
		t.Fatal("frame HTML is not escaped for its srcdoc attribute")
	}
	page := html.UnescapeString(frame)
	order := []string{"<head>", `content="` + documentPolicy + `"`, "<style>main{color:red}<\\/style>", "<title>", `<main id="brief"></main>`,
		"<script>" + documentShim + "</script>", "<script>window.first=1</script>", `<script>window.second='<\/script>'</script>`, "sailryDocument.mount(", "</body>"}
	at := 0
	for _, part := range order {
		index := strings.Index(page[at:], part)
		if index < 0 {
			t.Fatalf("frame lacks %q in order:\n%s", part, page)
		}
		at += index + len(part)
	}
	if strings.Contains(page, "<script>alert(1)") || strings.Count(page, "</script>") != 4 || strings.Count(strings.ToLower(page), "</style>") != 1 {
		t.Fatalf("fragment or package text escaped its script:\n%s", page)
	}
	input := page[strings.Index(page, "sailryDocument.mount(")+len("sailryDocument.mount(") : strings.Index(page, "));window.sailryDocumentReady.catch")]
	var mount struct {
		HTML     string `json:"html"`
		Writable bool   `json:"writable"`
		Identity struct {
			Artifact, Session string
			Revision          uint32
		} `json:"identity"`
		Drafts []json.RawMessage `json:"drafts"`
	}
	if err := json.Unmarshal([]byte(input), &mount); err != nil || mount.HTML != content.Html || !mount.Writable || mount.Drafts == nil {
		t.Fatalf("mount input %q: %+v %v", input, mount, err)
	}
	// The renderer echoes its identity in each event.
	identity, _ := json.Marshal(map[string]any{"kind": "draft", "artifact": mount.Identity.Artifact, "revision": mount.Identity.Revision, "session": mount.Identity.Session})
	if _, ok := decodeDocumentEvent(string(identity), content.Artifact); !ok {
		t.Fatalf("event with the mounted identity is refused: %s", identity)
	}

	if _, err := documentFrame(protocol.ArtifactContent{Artifact: content.Artifact, View: &protocol.ArtifactViewContent{Template: "<p>no body</p>"}}, documentMount{}); err == nil {
		t.Fatal("a template without head and body composed a page")
	}
	content.View = &protocol.ArtifactViewContent{Template: content.View.Template, Libraries: []protocol.ArtifactLibrary{protocol.ArtifactLibraryEcharts}}
	frame, err = documentFrame(content, documentMount{})
	if err != nil || !strings.Contains(frame, html.EscapeString("<script>"+documentShim+"</script><script>")) || len(frame) < 1<<20 {
		t.Fatalf("declared library is not inlined after the shim: %d bytes, %v", len(frame), err)
	}
}

// TestDocumentEventIdentity covers events of another revision, document or
// conversation, which a page cannot use.
func TestDocumentEventIdentity(t *testing.T) {
	t.Parallel()
	content := documentFormContent()
	event := documentFormEvent(content, "submit", "decisions", "[]")
	for name, change := range map[string]func(*documentEvent){
		"revision": func(e *documentEvent) { e.Revision-- },
		"artifact": func(e *documentEvent) { e.Artifact = protocol.NewArtifactID().String() },
		"session":  func(e *documentEvent) { e.Session = protocol.SessionID{8}.String() },
	} {
		changed := event
		change(&changed)
		raw, _ := json.Marshal(changed)
		if _, ok := decodeDocumentEvent(string(raw), content.Artifact); ok {
			t.Fatalf("event of another %s accepted", name)
		}
	}
	if _, ok := decodeDocumentEvent(`{"kind":"submit"`+strings.Repeat(" ", documentEventLimit)+`}`, content.Artifact); ok {
		t.Fatal("oversized event accepted")
	}
}

// TestDocumentFormReply covers the user message a form sends and the
// answers Desktop refuses.
func TestDocumentFormReply(t *testing.T) {
	t.Parallel()
	content := documentFormContent()
	id := content.Artifact.ID.String()
	answers := `[{"name":"approach","label":"Approach","values":[{"value":"a","label":"Approach A"}]},` +
		`{"name":"note","label":"Note","values":[{"value":"Keep it small","label":"Keep it small"}]}]`
	for lang, header := range map[string]string{
		"en":    "Reply to Plan %{id} (version 3, form decisions) [" + id + "]",
		"zh-CN": "回复「Plan %{id}」（版本 3，表单 decisions） [" + id + "]",
	} {
		reply, err := documentFormReply(lang, content, documentFormEvent(content, "submit", "decisions", answers))
		if want := header + "\n\nApproach:\n- Approach A (a)\n\nNote:\n- Keep it small"; err != nil || reply != want {
			t.Fatalf("%s reply = %q, %v; want %q", lang, reply, err, want)
		}
	}
	for name, test := range map[string]struct {
		form, fields string
		want         error
	}{
		"required unanswered": {"decisions", `[{"name":"approach","label":"Approach","values":[]},{"name":"note","label":"Note","values":[]}]`, errDocumentFormRequired},
		"required omitted":    {"decisions", `[{"name":"note","label":"Note","values":[]}]`, errDocumentFormRequired},
		"undeclared field":    {"decisions", `[{"name":"approach","label":"A","values":[{"value":"a","label":"a"}]},{"name":"execute","label":"Run","values":[]}]`, errDocumentFormInvalid},
		"duplicate field":     {"decisions", `[{"name":"approach","label":"A","values":[{"value":"a","label":"a"}]},{"name":"approach","label":"A","values":[]}]`, errDocumentFormInvalid},
		"undeclared form":     {"other", `[{"name":"approach","label":"A","values":[{"value":"a","label":"a"}]}]`, errDocumentFormInvalid},
		"invalid form name":   {"1decisions", `[{"name":"approach","label":"A","values":[{"value":"a","label":"a"}]}]`, errDocumentFormInvalid},
		"control character":   {"decisions", `[{"name":"approach","label":"A","values":[{"value":"a\u0000","label":"a"}]}]`, errDocumentFormInvalid},
		"oversized answer":    {"decisions", `[{"name":"approach","label":"A","values":[{"value":"` + strings.Repeat("x", documentFormValueBytes+1) + `","label":"a"}]}]`, errDocumentFormInvalid},
		"no fields":           {"decisions", `[]`, errDocumentFormInvalid},
		"not a list":          {"decisions", `{}`, errDocumentFormInvalid},
	} {
		if _, err := documentFormReply("en", content, documentFormEvent(content, "submit", test.form, test.fields)); !errors.Is(err, test.want) {
			t.Fatalf("%s: %v, want %v", name, err, test.want)
		}
	}
}

// TestDocumentFormSubmission covers which revisions and conversations may
// submit, and the latest revision the Node reports.
func TestDocumentFormSubmission(t *testing.T) {
	t.Parallel()
	content := documentFormContent()
	owner := &Runtime{}
	stale := content
	stale.LatestRevision = 4
	if documentWritable(documentSource{content: stale, owner: owner, conversation: content.Artifact.Session}) {
		t.Fatal("an older revision can submit")
	}
	if documentWritable(documentSource{content: content, owner: owner, conversation: protocol.SessionID{9}}) {
		t.Fatal("an inherited document can submit")
	}
	if documentWritable(documentSource{content: content, conversation: content.Artifact.Session}) {
		t.Fatal("a document without its owner can submit")
	}
	page := protocol.ArtifactRevisionPage{Items: []protocol.ArtifactRevision{{ID: content.Artifact.ID, Revision: 4}}}
	if documentLatest(content.Artifact, page) || documentLatest(content.Artifact, protocol.ArtifactRevisionPage{}) {
		t.Fatal("a revision the Node replaced is current")
	}
	page.Items[0].Revision = 3
	if !documentLatest(content.Artifact, page) {
		t.Fatal("the latest revision is stale")
	}
}

// TestDocumentFormDraft covers the drafts a reloaded page restores.
func TestDocumentFormDraft(t *testing.T) {
	t.Parallel()
	content := documentFormContent()
	draft, err := documentFormDraft(documentFormEvent(content, "draft", "decisions", `[{"name":"approach","values":["a"]},{"name":"note","values":["line\nbreak"]}]`))
	if want := `{"fields":[{"name":"approach","values":["a"]},{"name":"note","values":["line\nbreak"]}],"form":"decisions"}`; err != nil || string(draft) != want {
		t.Fatalf("draft = %s, %v", draft, err)
	}
	for _, fields := range []string{`[]`, `[{"name":"a","values":["x"],"extra":1}]`, `[{"name":"a","values":[]},{"name":"a","values":[]}]`, `[{"name":"-a","values":[]}]`} {
		if _, err := documentFormDraft(documentFormEvent(content, "draft", "decisions", fields)); err == nil {
			t.Fatalf("draft %s accepted", fields)
		}
	}
}
