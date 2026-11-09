// Package exporter renders portable conversation documents on the controller.
package exporter

import (
	"bytes"
	"context"
	"embed"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"image/draw"
	"image/png"
	"io"
	"mime"
	"net/http"
	"path"
	"runtime"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
)

//go:embed assets/*
var resources embed.FS

const maxBytes = 64 * 1024 * 1024
const renderURL = "sailry-export://localhost/index.html"

type Format string

const (
	Markdown Format = "markdown"
	HTML     Format = "html"
	PNG      Format = "png"
)

func (f Format) Extension() string {
	switch f {
	case Markdown:
		return "md"
	case HTML:
		return "html"
	case PNG:
		return "png"
	}
	return ""
}
func exportError(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: message}
}

var protocolOnce sync.Once
var protocolErr error
var pages = struct {
	sync.RWMutex
	values map[*mygo.Page]map[string][]byte
}{values: map[*mygo.Page]map[string][]byte{}}

func serve(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet || r.URL.RawQuery != "" {
		http.NotFound(w, r)
		return
	}
	page := mygo.CallerPage(r.Context())
	pages.RLock()
	body, ok := pages.values[page][r.URL.Path]
	pages.RUnlock()
	if !ok || len(body) > maxBytes {
		http.NotFound(w, r)
		return
	}
	kind := mime.TypeByExtension(path.Ext(r.URL.Path))
	if r.URL.Path == "/index.html" {
		kind = "text/html; charset=utf-8"
	} else if strings.HasSuffix(r.URL.Path, ".js") {
		kind = "text/javascript; charset=utf-8"
	}
	if kind == "" {
		kind = "application/octet-stream"
	}
	w.Header().Set("Content-Type", kind)
	w.Header().Set("X-Content-Type-Options", "nosniff")
	w.Header().Set("Access-Control-Allow-Origin", "*")
	_, _ = w.Write(body)
}

// Render performs no save or user feedback. files contains authenticated asset
// bytes keyed by client.ExportAttachmentName. Call it off the UI thread.
func Render(ctx context.Context, parent *mygo.Window, document client.ExportDocument, assets string, files map[string][]byte, format Format) ([]byte, error) {
	if ctx.Err() != nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "conversation export was cancelled"}
	}
	if len(document.Content) > maxBytes {
		return nil, exportError("conversation export exceeds 64 MiB")
	}
	if format == Markdown {
		return []byte(document.Content + "\n---\n\n[Sailry](https://sailry.dev)\n"), nil
	}
	if format != HTML && format != PNG {
		return nil, exportError("export format is invalid")
	}
	if runtime.GOOS != "darwin" && runtime.GOOS != "windows" {
		return nil, exportError("native conversation export is unavailable on this platform")
	}
	if assets == "" || strings.ContainsAny(assets, "\\:<>") || strings.Contains(assets, "..") || strings.HasPrefix(assets, "/") {
		return nil, exportError("export asset directory is invalid")
	}
	protocolOnce.Do(func() { protocolErr = mygo.Protocol.HandleFunc("sailry-export", serve) })
	if protocolErr != nil {
		return nil, protocolErr
	}
	local := map[string][]byte{}
	entries, err := resources.ReadDir("assets")
	if err != nil {
		return nil, err
	}
	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}
		body, e := resources.ReadFile("assets/" + entry.Name())
		if e != nil {
			return nil, e
		}
		local["/"+assets+"/"+entry.Name()] = body
	}
	for _, attachment := range document.Attachments {
		name := client.ExportAttachmentName(attachment)
		data, ok := files[name]
		if !ok {
			return nil, exportError("export attachment could not be read")
		}
		if len(data) > maxBytes {
			return nil, exportError("export attachment exceeds the supported size")
		}
		local["/"+assets+"/"+name] = bytes.Clone(data)
	}
	content, err := decorate(document.Content, assets, format)
	if err != nil {
		return nil, err
	}
	local["/index.html"] = []byte(content)
	ctx, cancel := context.WithTimeout(ctx, 90*time.Second)
	defer cancel()
	view := mygo.NewWindow(mygo.WindowOptions{Title: "Conversation export", Parent: parent, Hidden: true, Width: 960, Height: 768, UseContentSize: true, Page: mygo.PageOptions{Untrusted: true, Session: mygo.NewEphemeralSession(), DevTools: mygo.DevToolsDisabled, BackgroundThrottling: mygo.ThrottlingOff}})
	if view == nil {
		return nil, exportError("native conversation renderer could not be created")
	}
	defer view.Close()
	page := view.Page()
	if page == nil {
		return nil, exportError("native conversation renderer has no page")
	}
	pages.Lock()
	pages.values[page] = local
	pages.Unlock()
	defer func() { pages.Lock(); delete(pages.values, page); pages.Unlock() }()
	offNavigation := page.OnWillNavigate(func(e *mygo.NavigateEvent) { e.PreventDefault() })
	defer offNavigation()
	offClose := view.OnClosed(cancel)
	defer offClose()
	if parent != nil {
		off := parent.OnClosed(cancel)
		defer off()
	}
	ready := make(chan error, 1)
	send := func(err error) {
		select {
		case ready <- err:
		default:
		}
	}
	offReady := page.OnDidFinishLoad(func() {
		if page.URL() == renderURL || page.URL() == "http://sailry-export.localhost/index.html" {
			send(nil)
		}
	})
	defer offReady()
	offFailed := page.OnDidFailLoad(func(e *mygo.LoadError) { send(fmt.Errorf("conversation renderer failed to load: %v", e)) })
	defer offFailed()
	if err = page.LoadURL(renderURL); err != nil {
		return nil, err
	}
	select {
	case err = <-ready:
		if err != nil {
			return nil, err
		}
	case <-ctx.Done():
		return nil, renderContextError(ctx.Err())
	}
	value, err := page.EvalContext(ctx, "return await window.sailryExport.result")
	if err != nil {
		if ctx.Err() != nil {
			return nil, renderContextError(ctx.Err())
		}
		return nil, err
	}
	encoded, err := json.Marshal(value)
	if err != nil {
		return nil, err
	}
	return decode(encoded, format)
}
func renderContextError(err error) error {
	if errors.Is(err, context.Canceled) {
		return &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "conversation export was cancelled"}
	}
	return exportError("conversation export timed out")
}
func decorate(content, assets string, format Format) (string, error) {
	if !strings.Contains(content, "</head>") || !strings.Contains(content, "</body>") {
		return "", exportError("export document is invalid")
	}
	var scripts strings.Builder
	for _, name := range []string{"bridge.js", "config.js", "mathjax.js", "fonts.js", "mermaid.js", "echarts.js", "prepare.js"} {
		scripts.WriteString(`<script crossorigin="anonymous" src="` + assets + `/` + name + `"></script>`)
	}
	logo, _ := resources.ReadFile("assets/sailry-mark.svg")
	qr, _ := resources.ReadFile("assets/sailry-site-qr.svg")
	footer := `<footer style="margin-top:40px;padding:28px;background:#f3f4f6;display:flex;align-items:center;gap:18px"><img alt="Sailry" width="60" height="60" src="data:image/svg+xml;base64,` + base64.StdEncoding.EncodeToString(logo) + `"><div style="flex:1"><strong style="font-size:24px">Sailry</strong><br><a href="https://sailry.dev" style="color:#596273;text-decoration:none">sailry.dev</a></div><img alt="https://sailry.dev" width="112" height="112" style="background:#fff" src="data:image/svg+xml;base64,` + base64.StdEncoding.EncodeToString(qr) + `"></footer>`
	capture := `<script crossorigin="anonymous" src="` + assets + `/static.js"></script>`
	if format == PNG {
		capture = `<script crossorigin="anonymous" src="` + assets + `/html2canvas.js"></script><script crossorigin="anonymous" src="` + assets + `/capture.js"></script>`
	}
	content = strings.Replace(content, "</body>", footer+capture+"</body>", 1)
	return strings.Replace(content, "</head>", `<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; img-src 'self' data:; font-src data:; connect-src 'self'; frame-src 'self'; base-uri 'none'; object-src 'none'"><style>html{background:#fff;color:#20242b}body{max-width:900px;margin:0 auto;padding:32px;font:18px/1.7 system-ui}pre{white-space:pre-wrap;overflow-wrap:anywhere}table{width:100%;font-size:15px}.export-visual{max-width:100%;height:auto}mjx-container{max-width:100%;overflow:auto}</style>`+scripts.String()+"</head>", 1), nil
}
func decode(data []byte, format Format) ([]byte, error) {
	if len(data) > maxBytes {
		return nil, exportError("conversation export exceeds 64 MiB")
	}
	var diagnostic struct {
		Error string `json:"error"`
	}
	if json.Unmarshal(data, &diagnostic) == nil && diagnostic.Error != "" {
		return nil, exportError("conversation export failed: " + diagnostic.Error)
	}
	if format == HTML {
		var doc struct {
			HTML string `json:"html"`
		}
		decoder := json.NewDecoder(bytes.NewReader(data))
		decoder.DisallowUnknownFields()
		if decoder.Decode(&doc) != nil || len(doc.HTML) > maxBytes || !strings.HasPrefix(doc.HTML, "<!doctype html>") {
			return nil, exportError("rendered conversation document is invalid")
		}
		return []byte(doc.HTML), nil
	}
	var value struct {
		Height uint32   `json:"height"`
		Tiles  []string `json:"tiles"`
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if decoder.Decode(&value) != nil || value.Height == 0 || value.Height > 32768 || len(value.Tiles) != int((value.Height+2047)/2048) {
		return nil, exportError("rendered conversation image is invalid")
	}
	output := image.NewRGBA(image.Rect(0, 0, 960, int(value.Height)))
	y := 0
	for _, encoded := range value.Tiles {
		text, ok := strings.CutPrefix(encoded, "data:image/png;base64,")
		if !ok {
			return nil, exportError("rendered image tile is invalid")
		}
		data, err := base64.StdEncoding.DecodeString(text)
		if err != nil {
			return nil, err
		}
		dimensions, err := png.DecodeConfig(bytes.NewReader(data))
		height := min(2048, int(value.Height)-y)
		if err != nil || dimensions.Width != 960 || dimensions.Height != height {
			return nil, exportError("rendered image tile size is invalid")
		}
		tile, err := png.Decode(bytes.NewReader(data))
		if err != nil {
			return nil, err
		}
		draw.Draw(output, image.Rect(0, y, 960, y+height), tile, tile.Bounds().Min, draw.Src)
		y += height
	}
	var out bytes.Buffer
	if err := png.Encode(&out, output); err != nil {
		return nil, err
	}
	return io.ReadAll(&out)
}
