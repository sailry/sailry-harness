package live

import (
	"bytes"
	"encoding/json"
	"errors"
	"strings"

	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/net/html"
	"golang.org/x/net/html/atom"
)

// Form answers follow the bounds the Brief renderer's forms.js enforces;
// Desktop checks them again because the page is untrusted.
const (
	documentFormFields     = 32
	documentFormValues     = 64
	documentFormLabelBytes = 512
	documentFormValueBytes = 4096
	documentFormBytes      = 16 << 10
	documentReplyBytes     = 32 << 10
)

var (
	errDocumentFormInvalid  = errors.New("invalid document form")
	errDocumentFormRequired = errors.New("document form misses a required answer")
)

// documentEvent is one event a document's renderer posts. It names the
// document revision it was rendered for.
type documentEvent struct {
	Kind     string          `json:"kind"`
	Artifact string          `json:"artifact"`
	Revision uint32          `json:"revision"`
	Session  string          `json:"session"`
	Form     string          `json:"form"`
	Fields   json.RawMessage `json:"fields"`
}

// decodeDocumentEvent reads an event posted for exactly this revision.
func decodeDocumentEvent(raw string, artifact protocol.ArtifactSummary) (documentEvent, bool) {
	var event documentEvent
	if len(raw) > documentEventLimit || json.Unmarshal([]byte(raw), &event) != nil {
		return event, false
	}
	return event, event.Artifact == artifact.ID.String() && event.Revision == artifact.Revision && event.Session == artifact.Session.String()
}

// documentName is a form or field name: a Latin letter, then letters,
// digits, '_', '.' or '-', up to 64 bytes.
func documentName(value string) bool {
	if value == "" || len(value) > 64 {
		return false
	}
	for index, b := range []byte(value) {
		letter := b >= 'a' && b <= 'z' || b >= 'A' && b <= 'Z'
		if !letter && (index == 0 || !(b >= '0' && b <= '9' || b == '_' || b == '.' || b == '-')) {
			return false
		}
	}
	return true
}

// documentText is answer text of at most limit bytes without control
// characters other than tabs and line breaks.
func documentText(value string, limit int) bool {
	return len(value) <= limit && strings.IndexFunc(value, func(r rune) bool {
		return r < 0x20 && r != '\t' && r != '\n' && r != '\r' || r == 0x7f
	}) < 0
}

// documentFormFieldset is a question the sanitized fragment declares.
type documentFormFieldset struct {
	required bool
}

// documentFormDeclaration finds the fieldsets of the named form in the
// stored fragment, which the Node sanitized: it, not the page, says what
// the form asks and what is required.
func documentFormDeclaration(fragment, form string) (map[string]documentFormFieldset, bool) {
	nodes, err := html.ParseFragment(strings.NewReader(fragment), &html.Node{Type: html.ElementNode, DataAtom: atom.Div, Data: "div"})
	if err != nil {
		return nil, false
	}
	attribute := func(node *html.Node, key string) (string, bool) {
		for _, value := range node.Attr {
			if value.Key == key {
				return value.Val, true
			}
		}
		return "", false
	}
	var found *html.Node
	var find func(*html.Node)
	find = func(node *html.Node) {
		if found != nil {
			return
		}
		if node.Type == html.ElementNode && node.DataAtom == atom.Form {
			if name, _ := attribute(node, "data-name"); name == form {
				found = node
				return
			}
		}
		for child := node.FirstChild; child != nil; child = child.NextSibling {
			find(child)
		}
	}
	for _, node := range nodes {
		find(node)
	}
	if found == nil {
		return nil, false
	}
	fieldsets := map[string]documentFormFieldset{}
	var collect func(*html.Node)
	collect = func(node *html.Node) {
		if node.Type == html.ElementNode && node.DataAtom == atom.Fieldset {
			if name, ok := attribute(node, "data-name"); ok {
				_, required := attribute(node, "data-required")
				fieldsets[name] = documentFormFieldset{required: required}
			}
		}
		for child := node.FirstChild; child != nil; child = child.NextSibling {
			collect(child)
		}
	}
	collect(found)
	return fieldsets, len(fieldsets) > 0
}

// documentFormReply composes the user message a submitted form sends: the
// localized reply line naming the document, its revision and the form, then
// each question with its answers. Only questions the stored fragment
// declares are accepted, and each required one needs an answer.
func documentFormReply(lang string, content protocol.ArtifactContent, event documentEvent) (string, error) {
	type answer struct {
		Value string `json:"value"`
		Label string `json:"label"`
	}
	type field struct {
		Name   string   `json:"name"`
		Label  string   `json:"label"`
		Values []answer `json:"values"`
	}
	var fields []field
	if !documentName(event.Form) || len(event.Fields) > documentFormBytes || json.Unmarshal(event.Fields, &fields) != nil ||
		len(fields) == 0 || len(fields) > documentFormFields {
		return "", errDocumentFormInvalid
	}
	declared, ok := documentFormDeclaration(content.Html, event.Form)
	if !ok {
		return "", errDocumentFormInvalid
	}
	answered := map[string]bool{}
	var body strings.Builder
	for _, field := range fields {
		if _, known := declared[field.Name]; !known || answered[field.Name] || !documentText(field.Label, documentFormLabelBytes) || len(field.Values) > documentFormValues {
			return "", errDocumentFormInvalid
		}
		answered[field.Name] = len(field.Values) > 0
		title := strings.TrimSpace(field.Label)
		if title == "" {
			title = field.Name
		}
		body.WriteString("\n\n" + title + ":")
		for _, value := range field.Values {
			if !documentText(value.Value, documentFormValueBytes) || !documentText(value.Label, documentFormValueBytes) {
				return "", errDocumentFormInvalid
			}
			if value.Label == value.Value {
				body.WriteString("\n- " + value.Label)
			} else {
				body.WriteString("\n- " + value.Label + " (" + value.Value + ")")
			}
		}
	}
	for name, fieldset := range declared {
		if fieldset.required && !answered[name] {
			return "", errDocumentFormRequired
		}
	}
	// The title goes last so that its text is never read as a placeholder.
	reply := i18n.In(lang, "artifact_form_reply", "id", content.Artifact.ID.String(), "revision", content.Artifact.Revision,
		"form", event.Form, "title", content.Artifact.Title) + body.String()
	if len(reply) > documentReplyBytes {
		return "", errDocumentFormInvalid
	}
	return reply, nil
}

// documentFormDraft is the canonical draft of a form's unsent answers, as
// the renderer's restore takes it back.
func documentFormDraft(event documentEvent) (json.RawMessage, error) {
	type field struct {
		Name   string   `json:"name"`
		Values []string `json:"values"`
	}
	var fields []field
	decoder := json.NewDecoder(bytes.NewReader(event.Fields))
	decoder.DisallowUnknownFields()
	if !documentName(event.Form) || len(event.Fields) > documentFormBytes || decoder.Decode(&fields) != nil ||
		len(fields) == 0 || len(fields) > documentFormFields {
		return nil, errDocumentFormInvalid
	}
	seen := map[string]bool{}
	for _, field := range fields {
		if !documentName(field.Name) || seen[field.Name] || len(field.Values) > documentFormValues {
			return nil, errDocumentFormInvalid
		}
		seen[field.Name] = true
		for _, value := range field.Values {
			if !documentText(value, documentFormValueBytes) {
				return nil, errDocumentFormInvalid
			}
		}
	}
	return json.Marshal(map[string]any{"form": event.Form, "fields": fields})
}
