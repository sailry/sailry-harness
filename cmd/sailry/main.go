// Sailry is the desktop app's window in native UI, drawn by MyGo, over the
// in-memory preview dataset.
package main

import (
	"log"

	"github.com/egoist/mygo"
	// MyGo's own strings (menus, dialogs, controls) in Chinese.
	_ "github.com/egoist/mygo/locales/zh-Hans"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/preview"
	"github.com/sailry/sailry-go/internal/shell"
	"github.com/sailry/sailry-go/internal/theme"
)

func main() {
	mygo.App.SetLanguages(i18n.Languages...)
	app := shell.New(preview.Load())
	app.OnMode = func(m theme.Mode) {
		mygo.Theme.SetSource(map[theme.Mode]mygo.ThemeSource{
			theme.System: mygo.ThemeSystem, theme.Light: mygo.ThemeLight, theme.Dark: mygo.ThemeDark,
		}[m])
	}
	mygo.App.WhenReady(func() {
		// The UI preferences come back from the user data folder, beside
		// MyGo's window-state.json, and are written as the window closes
		// or loses the focus, and as the app quits.
		dir, err := mygo.App.Path(mygo.PathUserData)
		if err == nil {
			app.SetPrefs(shell.LoadPrefs(dir))
		}
		save := func() {
			if dir != "" {
				if err := shell.SavePrefs(dir, app.Prefs()); err != nil {
					log.Printf("ui preferences: %v", err)
				}
			}
		}
		mygo.App.OnWillQuit(func(*mygo.QuitEvent) { save() })
		win := mygo.NewWindow(mygo.WindowOptions{
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
		win.OnBlur(save)
		win.OnClose(func(*mygo.CloseEvent) { save() })
	})
	if err := mygo.App.Run(); err != nil {
		log.Fatal(err)
	}
}
