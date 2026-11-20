package live

import (
	"encoding/json"
	"errors"
	"fmt"
	"html"
	"regexp"
	"strconv"
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/exporter"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
	nethtml "golang.org/x/net/html"
)

// documentEventLimit bounds one event a document's frame relays; a form's
// answers are at most 16 KiB.
const documentEventLimit = 32 << 10

// documentPolicy confines a document's frame to inline code and data.
const documentPolicy = `default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; connect-src 'none'; form-action 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'`

// documentRelay is the wrapper's own script, outside the document's frame.
// It keeps the strings the frame posts until Desktop takes them with
// sailryHost.next(), and posts Desktop's replies into the frame. Desktop
// alone calls it, by evaluation; the frame's opaque origin cannot reach it.
var documentRelay = `(()=>{const frame=document.getElementById('document'),queue=[];let waiting=null;` +
	`addEventListener('message',event=>{if(event.source!==frame.contentWindow||typeof event.data!=='string'||event.data.length>` + strconv.Itoa(documentEventLimit) + `)return;` +
	`if(queue.length<64)queue.push(event.data);if(waiting){const resolve=waiting;waiting=null;resolve(queue.splice(0));}});` +
	`window.sailryHost=Object.freeze({next:()=>queue.length?queue.splice(0):new Promise(resolve=>{waiting=resolve;}),` +
	`send:value=>frame.contentWindow.postMessage(value,'*')});})();`

// documentShim is the renderer's link to Desktop inside the frame: events
// leave as strings through the parent, and only Desktop's acknowledgement,
// form availability and theme come back.
const documentShim = `(()=>{const host=window.parent;` +
	`window.sailryDocumentPost=event=>{try{host.postMessage(JSON.stringify(event),'*');}catch(_){}};` +
	`addEventListener('message',event=>{if(event.source!==host||typeof event.data!=='string')return;let value;try{value=JSON.parse(event.data);}catch(_){return;}` +
	`const page=window.sailryDocument;if(!page||!value)return;` +
	`if(value.kind==='submitted'&&typeof value.form==='string')page.submitted(value.form,value.accepted===true);` +
	`else if(value.kind==='writable')page.setWritable(value.writable===true);` +
	`else if(value.kind==='theme'&&value.theme&&typeof value.theme==='object')page.retheme(value.theme);});})();`

// documentPage wraps a document's frame HTML in a frame of an opaque origin:
// its scripts run, without a network, a parent to reach or a place to
// navigate. Only the relay outside the frame hears what it posts. It names
// its encoding, which a copy opened from disk has no other way to declare.
func documentPage(frame string, plain bool, mount documentMount) string {
	// The page shows on the app's background in its light or dark scheme
	// while its frame loads, and a page without a renderer keeps it.
	page, scheme := "white", "light"
	if value := mount.Theme["background"]; value != "" {
		page = value
	}
	if mount.Dark {
		scheme = "dark"
	}
	// A renderer's frame keeps its own scheme: an iframe whose scheme
	// differs from its page's paints an opaque backdrop behind it.
	background, inner := "transparent", "normal"
	if plain {
		background, inner = page, scheme
	}
	return `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; frame-src about: 'self'; img-src data: blob:; font-src data:; connect-src 'none'; form-action 'none'; object-src 'none'; base-uri 'none'"><style>html{background:` + page + `;color-scheme:` + scheme + `}html,body{margin:0;width:100%;height:100%;overflow:hidden}iframe{border:0;width:100%;height:100%;color-scheme:` + inner + `;background:` +
		background + `}</style></head><body><iframe id="document" sandbox="allow-scripts" srcdoc="` + frame + `"></iframe><script>` + documentRelay + `</script></body></html>`
}

// documentMount is what the renderer's mount receives besides the fragment.
type documentMount struct {
	Theme    map[string]string
	Labels   map[string]string
	Writable bool
	Drafts   []json.RawMessage
	// Dark is the app's scheme, which a page without a renderer takes.
	Dark bool
}

// documentFrame is a document's escaped frame HTML: the package's template
// with its styles, the declared libraries and scripts, and the mount of the
// sanitized fragment. A document without a renderer shows its fragment.
func documentFrame(content protocol.ArtifactContent, mount documentMount) (string, error) {
	view := content.View
	if view == nil || content.Artifact.Kind != protocol.ArtifactKindDocument {
		page := content.Html
		// Without a declared scheme of its own a page keeps its light
		// defaults on a dark app. The rule has no specificity, so any the
		// page sets wins, and comes last, so the doctype still leads.
		if mount.Dark {
			page += `<style>:where(:root){color-scheme:dark}</style>`
		}
		return html.EscapeString(page), nil
	}
	var head strings.Builder
	head.WriteString(`<meta http-equiv="Content-Security-Policy" content="` + documentPolicy + `">`)
	for _, style := range view.Styles {
		head.WriteString("<style>" + styleClose.ReplaceAllString(style, `<\/style`) + "</style>")
	}
	drafts := mount.Drafts
	if drafts == nil {
		drafts = []json.RawMessage{}
	}
	input, err := json.Marshal(map[string]any{
		"html": content.Html, "theme": mount.Theme, "labels": mount.Labels, "writable": mount.Writable, "drafts": drafts,
		"identity": map[string]any{"artifact": content.Artifact.ID, "revision": content.Artifact.Revision, "title": content.Artifact.Title, "session": content.Artifact.Session},
	})
	if err != nil {
		return "", err
	}
	before, after, err := documentInject(view.Template, head.String())
	if err != nil {
		return "", err
	}
	// The shim comes first, then the libraries, whose escaped text every
	// document shares, then the package's scripts and the mount.
	var frame strings.Builder
	frame.WriteString(html.EscapeString(before + documentScript(documentShim)))
	for _, library := range view.Libraries {
		script, err := documentLibrary(library)
		if err != nil {
			return "", err
		}
		frame.WriteString(script)
	}
	var tail strings.Builder
	for _, script := range view.Scripts {
		tail.WriteString(documentScript(script))
	}
	// encoding/json escapes <, > and &, so the input cannot close its script.
	tail.WriteString(documentScript(`window.sailryDocumentReady=Promise.resolve().then(()=>window.sailryDocument.mount(` + string(input) + `));` +
		`window.sailryDocumentReady.catch(()=>window.sailryDocumentPost?.({kind:'render_error'}));`))
	frame.WriteString(html.EscapeString(tail.String() + after))
	return frame.String(), nil
}

var scriptClose = regexp.MustCompile(`(?i)</script`)
var styleClose = regexp.MustCompile(`(?i)</style`)

// documentScript is one inline script element; text that would end it
// early keeps its meaning as an escaped slash.
func documentScript(source string) string {
	return "<script>" + scriptClose.ReplaceAllString(source, `<\/script`) + "</script>"
}

var documentLibraries sync.Map // protocol.ArtifactLibrary -> escaped script element

func documentLibrary(library protocol.ArtifactLibrary) (string, error) {
	if script, ok := documentLibraries.Load(library); ok {
		return script.(string), nil
	}
	source, ok := exporter.DocumentLibrary(library)
	if !ok {
		return "", fmt.Errorf("document library %q is unavailable", library)
	}
	script, _ := documentLibraries.LoadOrStore(library, html.EscapeString(documentScript(source)))
	return script.(string), nil
}

// documentInject puts head after the template's <head> tag and splits the
// result before its </body> tag, where the scripts go.
func documentInject(template, head string) (string, string, error) {
	tokens := nethtml.NewTokenizer(strings.NewReader(template))
	offset, start, end := 0, -1, -1
	for end < 0 {
		kind := tokens.Next()
		if kind == nethtml.ErrorToken {
			break
		}
		size := len(tokens.Raw())
		name, _ := tokens.TagName()
		switch {
		case kind == nethtml.StartTagToken && start < 0 && string(name) == "head":
			start = offset + size
		case kind == nethtml.EndTagToken && string(name) == "body":
			end = offset
		}
		offset += size
	}
	if start < 0 || end < start {
		return "", "", errors.New("document template needs head and body tags")
	}
	return template[:start] + head + template[start:end], template[end:], nil
}

// documentTheme is the palette as the renderer's theme values.
func documentTheme(palette *theme.Palette, fontSize float32) map[string]string {
	values := map[string]string{
		"font_family":      "system-ui, -apple-system, sans-serif",
		"mono_font_family": "ui-monospace, SFMono-Regular, Menlo, monospace",
		"font_size":        fmt.Sprintf("%gpx", fontSize),
		"radius":           fmt.Sprintf("%gpx", palette.Radius),
	}
	for name, value := range map[string]ui.Color{
		"background": palette.Background, "foreground": palette.Foreground,
		"primary": palette.Primary, "primary_foreground": palette.PrimaryForeground,
		"secondary": palette.Secondary, "secondary_foreground": palette.SecondaryForeground,
		"muted": palette.Muted, "muted_foreground": palette.MutedForeground, "accent": palette.Hover,
		"border": palette.Border, "ring": palette.Ring, "success": palette.Success, "warning": palette.Warning, "danger": palette.Danger,
	} {
		values[name] = fmt.Sprintf("#%02x%02x%02x%02x", value.R, value.G, value.B, value.A)
	}
	return values
}

// documentFileMount is what a document's page opened in a browser or saved
// mounts with: the app's theme and labels, its forms read-only.
func documentFileMount(c *ui.Context) documentMount {
	return documentMount{Theme: documentTheme(theme.Of(c), c.Theme().FontSize), Dark: theme.Of(c).Dark,
		Labels: map[string]string{"contents": i18n.T(c, "artifact_contents"), "submit": i18n.T(c, "send"), "close": i18n.T(c, "artifact_close")}}
}

// documentFile is a revision as a page of its own: a document composed in
// its package's renderer as the panel shows it, or the HTML itself for a
// preview or a document without a renderer. The fragment alone would show
// none of the renderer's layout, charts or diagrams.
func documentFile(content protocol.ArtifactContent, mount documentMount) string {
	if content.View == nil || content.Artifact.Kind != protocol.ArtifactKindDocument {
		return content.Html
	}
	frame, err := documentFrame(content, mount)
	if err != nil {
		return content.Html
	}
	return documentPage(frame, false, mount)
}
