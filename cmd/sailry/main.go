// Sailry is the desktop app's window in native UI, drawn by MyGo, over the
// in-memory preview dataset.
package main

import (
	"log"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/preview"
	"github.com/sailry/sailry-go/internal/shell"
)

func main() {
	mygo.App.SetLanguages(i18n.Languages...)
	app := shell.New(preview.Load())
	mygo.App.WhenReady(func() {
		mygo.NewWindow(mygo.WindowOptions{
			Title:                i18n.In(i18n.Match(mygo.App.Locale()), "app"),
			Width:                1280,
			Height:               820,
			MinWidth:             760,
			MinHeight:            560,
			StateKey:             "main",
			TitleBarStyle:        mygo.TitleBarHidden,
			TrafficLightPosition: &mygo.Point{X: 12, Y: 16},
			Vibrancy:             mygo.VibrancySidebar,
			Content:              ui.View(app.View),
		})
	})
	if err := mygo.App.Run(); err != nil {
		log.Fatal(err)
	}
}
