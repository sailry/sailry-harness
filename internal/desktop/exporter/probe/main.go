//go:build integration

// The export probe exercises native offline rendering on the current platform.
package main

import (
	"bytes"
	"context"
	"flag"
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"strings"

	"github.com/egoist/mygo"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/exporter"
)

func main() {
	previewOnly := flag.Bool("preview", false, "Capture an opaque authored preview")
	flag.Parse()
	destination := ".runtime/b4/export"
	if *previewOnly {
		destination = ".runtime/b5/preview"
	}
	directory, err := filepath.Abs(destination)
	if err != nil {
		panic(err)
	}
	if err = os.MkdirAll(directory, 0700); err != nil {
		panic(err)
	}
	mygo.App.SetName("Sailry export probe")
	mygo.App.SetPath(mygo.PathUserData, filepath.Join(directory, "mygo"))
	result := make(chan error, 1)
	mygo.App.WhenReady(func() {
		parent := mygo.NewWindow(mygo.WindowOptions{Title: "Export probe", URL: "about:blank", Hidden: true, Width: 960, Height: 768, Page: mygo.PageOptions{Untrusted: true, Session: mygo.NewEphemeralSession()}})
		go func() {
			defer mygo.App.Quit()
			defer parent.Close()
			if *previewOnly {
				source := `<!doctype html><html><head><style>body{margin:0;background:#164e63;color:white;font:32px system-ui;padding:40px}#ready{font-size:20px}</style></head><body><h1>Workspace document</h1><p id="ready">Loading</p><script>document.getElementById('ready').textContent='Opaque preview rendered';</script></body></html>`
				data, e := exporter.PreviewImage(context.Background(), parent, source)
				if e != nil {
					result <- e
					return
				}
				config, e := png.DecodeConfig(bytes.NewReader(data))
				if e != nil || config.Width < 960 || config.Width%960 != 0 || config.Height != 768*(config.Width/960) {
					result <- fmt.Errorf("invalid preview image dimensions: %+v %v", config, e)
					return
				}
				name := filepath.Join(directory, "document.png")
				if e = os.WriteFile(name, data, 0600); e != nil {
					result <- e
					return
				}
				fmt.Printf("Rendered %s (%d bytes)\n", name, len(data))
				result <- nil
				return
			}

			document := client.ExportDocument{Content: `<!doctype html><html><head><meta charset="utf-8"><title>Offline export</title></head><body><section class="message user"><p>Render the complete answer</p></section><section class="message assistant"><h1>Offline export</h1><p>Formula <span class="math inline">\(x^2 + y^2\)</span></p><pre><code class="language-mermaid">flowchart LR
A[Question] --&gt; B[Answer]</code></pre><pre><code class="language-chart">{"type":"bar","data":[{"label":"A","value":3},{"label":"B","value":7}]}</code></pre><table><thead><tr><th>Name</th><th>Value</th></tr></thead><tbody><tr><td>Complete</td><td>Yes</td></tr></tbody></table></section></body></html>`}
			for _, format := range []exporter.Format{exporter.HTML, exporter.PNG} {
				data, e := exporter.Render(context.Background(), parent, document, "sailry-assets-export", nil, format)
				if e != nil {
					result <- e
					return
				}
				name := filepath.Join(directory, "conversation."+format.Extension())
				if e = os.WriteFile(name, data, 0600); e != nil {
					result <- e
					return
				}
				fmt.Printf("Rendered %s (%d bytes)\n", name, len(data))
			}
			document.Content = strings.Replace(document.Content, "</body>", strings.Repeat("<p>Long conversation: every paragraph remains in the exported image.</p>", 100)+"<p>End of exported answer</p></body>", 1)
			data, e := exporter.Render(context.Background(), parent, document, "sailry-assets-export", nil, exporter.PNG)
			if e != nil {
				result <- e
				return
			}
			dimensions, e := png.DecodeConfig(bytes.NewReader(data))
			if e != nil {
				result <- e
				return
			}
			if dimensions.Height <= 4096 {
				result <- fmt.Errorf("long export did not exercise multiple tiles: %d", dimensions.Height)
				return
			}
			name := filepath.Join(directory, "conversation-long.png")
			if e = os.WriteFile(name, data, 0600); e != nil {
				result <- e
				return
			}
			fmt.Printf("Rendered %s (%d x %d, %d bytes)\n", name, dimensions.Width, dimensions.Height, len(data))
			result <- nil
		}()
	})
	if err = mygo.App.Run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err = <-result; err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
