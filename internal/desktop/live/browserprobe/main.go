//go:build integration

// The browser probe exercises the panel browser's and the session documents'
// WebViews on the current platform, against a page it serves itself.
package main

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/live"
)

func main() {
	directory, err := filepath.Abs(".runtime/b10/browser")
	if err != nil {
		panic(err)
	}
	if err = os.MkdirAll(directory, 0700); err != nil {
		panic(err)
	}
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
		writer.Header().Set("Content-Type", "text/html; charset=utf-8")
		fmt.Fprint(writer, `<!doctype html><html><head><title>Browser probe</title></head><body><h1>Browser probe</h1><a href="/next">Next</a><button>Press</button></body></html>`)
	}))
	defer server.Close()
	mygo.App.SetName("Sailry browser probe")
	mygo.App.SetPath(mygo.PathUserData, filepath.Join(directory, "mygo"))
	result := make(chan error, 1)
	mygo.App.WhenReady(func() {
		var shown *mygo.WebView
		window := mygo.NewWindow(mygo.WindowOptions{Title: "Browser probe", Width: 960, Height: 768, Content: ui.View(func(c *ui.Context) {
			if shown != nil && !shown.IsDestroyed() {
				ui.WebView(c, shown).Grow(1)
			}
		})})
		go func() {
			defer mygo.App.Quit()
			defer window.Close()
			ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
			defer cancel()
			result <- live.BrowserProbe(ctx, window, func(view *mygo.WebView) { window.Update(func() { shown = view }) }, server.URL+"/")
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
	fmt.Println("Browser probe passed")
}
