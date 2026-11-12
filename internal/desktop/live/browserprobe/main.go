//go:build integration

// The browser probe exercises the panel browser's and the session documents'
// WebViews on the current platform, against pages it serves itself: first
// the WebViews alone, then a browser tab of the real App over a real Node.
package main

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/live"
)

func main() {
	directory, err := filepath.Abs(".runtime/browserprobe")
	if err != nil {
		panic(err)
	}
	if err = os.MkdirAll(directory, 0700); err != nil {
		panic(err)
	}
	// The profile and the project are this probe's own; a run starts clean.
	profile, project := filepath.Join(directory, "profile"), filepath.Join(directory, "project")
	if err = os.RemoveAll(profile); err != nil {
		panic(err)
	}
	if err = os.MkdirAll(project, 0700); err != nil {
		panic(err)
	}
	var mu sync.Mutex
	fetched := map[string]int{}
	requests := func(path string) int {
		mu.Lock()
		defer mu.Unlock()
		return fetched[path]
	}
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		mu.Lock()
		fetched[request.URL.Path]++
		mu.Unlock()
		writer.Header().Set("Content-Type", "text/html; charset=utf-8")
		writer.Header().Set("Cache-Control", "no-store")
		switch request.URL.Path {
		case "/slow":
			// Long enough for the chrome to show the load.
			time.Sleep(1500 * time.Millisecond)
			fmt.Fprint(writer, `<!doctype html><html><head><title>Browser probe slow</title></head><body><h1>Slow page</h1></body></html>`)
			return
		case "/wide":
			fmt.Fprint(writer, `<!doctype html><html><head><title>Browser probe wide</title></head><body style="margin:0"><div style="width:3000px;height:40px;background:linear-gradient(90deg,#06c,#c06)"></div><button id="pick" style="position:fixed;left:40px;top:80px;width:160px;height:40px">Pick me</button></body></html>`)
			return
		}
		if request.URL.Path == "/next" {
			fmt.Fprint(writer, `<!doctype html><html><head><title>Browser probe next</title></head><body><h1>Next page</h1><a href="/">First</a></body></html>`)
			return
		}
		fmt.Fprint(writer, `<!doctype html><html><head><title>Browser probe</title></head><body><h1>Browser probe</h1><a href="/next">Next</a><button>Press</button></body></html>`)
	}))
	defer server.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 150*time.Second)
	defer cancel()
	tab, err := live.StartBrowserTabProbe(ctx, profile, project)
	if err != nil {
		panic(err)
	}
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
		app := mygo.NewWindow(mygo.WindowOptions{Title: "Browser tab probe", Width: 1280, Height: 820, Content: ui.View(tab.Runtime.App.View)})
		go func() {
			defer mygo.App.Quit()
			defer window.Close()
			defer app.Close()
			defer tab.Close()
			err := live.BrowserProbe(ctx, window, func(view *mygo.WebView) { window.Update(func() { shown = view }) }, server.URL+"/")
			if err == nil {
				fmt.Println("WebView probe passed")
				if err = tab.Run(ctx, app, server.URL, requests); err != nil {
					err = fmt.Errorf("browser tab: %w", err)
				} else {
					fmt.Printf("Browser tab probe passed: %d fetches of / and %d of /next\n", requests("/"), requests("/next"))
					// The reported page, when the network reaches it.
					fmt.Println("Outside page:", tab.Visit(ctx, "https://www.baidu.com/"))
				}
			}
			result <- err
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
