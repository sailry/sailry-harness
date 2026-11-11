// Sailry composes the native desktop with one local Node or an isolated preview.
package main

import (
	"context"
	"fmt"
	"log"
	"os"

	"github.com/egoist/mygo"
	_ "github.com/egoist/mygo/locales/ja"
	_ "github.com/egoist/mygo/locales/zh-Hans"
	_ "github.com/egoist/mygo/locales/zh-Hant"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/live"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/reader"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/node/computer"
	"github.com/sailry/sailry-harness/internal/node/logging"
)

type desktopWindow struct {
	window *mygo.Window
	app    *shell.App
	live   *live.Runtime
	save   func()
}

func main() {
	// Exiting here lets run's deferred shutdown finish first.
	if err := run(); err != nil {
		log.Fatal(err)
	}
}

func run() error {
	computer.Trampoline()
	options, err := parseOptions(os.Args[1:])
	if err != nil {
		return err
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	backend, err := openBackend(ctx, options)
	if err != nil {
		return err
	}
	defer func() {
		if err := backend.Close(); err != nil {
			log.Printf("Node shutdown: %v", err)
		}
	}()
	mygo.App.SetLanguages(i18n.Languages...)
	var notifier *live.Notifier
	var notificationWindow *mygo.Window
	if backend.Client != nil {
		notifier = live.NewNotifier(backend.Client, backend.Preferences)
		defer notifier.Close()
	}

	var windows []*desktopWindow
	var active *desktopWindow
	var speech *reader.Service
	system := systemShortcuts{register: mygo.GlobalShortcut.Register, unregister: mygo.GlobalShortcut.Unregister}
	var updates *live.Updates
	if backend.Client != nil && backend.Preferences != nil {
		updates = live.NewUpdates(backend.Preferences, backend.Path, backend.Close, mygo.App.Quit)
		defer updates.Service.Close()
	}
	var refreshCommands func()
	refreshCommands = func() {
		if active == nil {
			return
		}
		entry := active
		lang := i18n.Match(mygo.App.Locale())
		mygo.App.SetMenu(mygo.NewMenu(entry.app.Menu(lang, func(id string) { entry.app.Do(id); entry.window.Invalidate() })))
		system.apply(entry.app.SystemShortcuts(), func(command string) {
			if active == nil {
				return
			}
			current := active
			if command == "system.selection" && current.live != nil {
				// The selection belongs to the app in front, so it is
				// read before this window takes its place.
				current.live.CaptureSelection(func() { current.window.Show(); current.window.Focus() })
				return
			}
			current.window.Show()
			current.window.Focus()
			current.app.Do(command)
			current.window.Invalidate()
		}, func() {
			if entry.live != nil {
				entry.live.ReportShortcutConflict()
			}
		})
	}
	applyPreferences := func() {
		if backend.Preferences != nil {
			mygo.App.SetLocale(preferenceLocale(backend.Preferences.Data.Language))
		}
		refreshCommands()
	}
	var openWindow func(string) error
	openWindow = func(sessionID string) error {
		entry := &desktopWindow{}
		if options.Preview {
			entry.app = shell.New(preview.Load())
		} else {
			runtime, err := live.NewWithPreferences(ctx, backend.Client, backend.Preferences)
			if err != nil {
				return err
			}
			entry.live = runtime
			runtime.SetNetwork(backend.Network)
			runtime.SetLogFolder(logging.Directory())
			entry.app = entry.live.App
		}
		app := entry.app
		app.OnMode = func(mode theme.Mode) {
			mygo.Theme.SetSource(map[theme.Mode]mygo.ThemeSource{theme.System: mygo.ThemeSystem, theme.Light: mygo.ThemeLight, theme.Dark: mygo.ThemeDark}[mode])
		}
		directory, pathErr := mygo.App.Path(mygo.PathUserData)
		if pathErr == nil {
			app.SetPrefs(shell.LoadPrefs(directory))
		}
		entry.save = func() {
			if directory != "" {
				if err := shell.SavePrefs(directory, app.Prefs()); err != nil {
					log.Printf("window preferences: %v", err)
				}
			}
		}
		entry.window = mygo.NewWindow(mygo.WindowOptions{
			Title: i18n.In(i18n.Match(mygo.App.Locale()), "app"), Width: 1280, Height: 820, MinWidth: 760, MinHeight: 560,
			StateKey: "main", TitleBarStyle: mygo.TitleBarHidden, TrafficLightPosition: &mygo.Point{X: 12, Y: 16}, Vibrancy: mygo.VibrancySidebar, Content: ui.View(app.View),
		})
		if entry.live != nil {
			runtime := entry.live
			runtime.SetInvalidate(entry.window.Invalidate)
			runtime.SetUpdate(entry.window.Update)
			if speech == nil {
				speech = reader.New(entry.window.Update, func() {
					for _, candidate := range windows {
						candidate.window.Invalidate()
					}
				})
			}
			bindConversation(ctx, entry, speech)
			bindFiles(entry, backend.Client.Target())
			runtime.SetWebViewWindow(entry.window)
			runtime.SetOpenService(func(address string) {
				if err := mygo.Shell.OpenExternal(address); err != nil {
					runtime.Report(err)
				}
			})
			runtime.SetOpenWindow(func(id string) {
				if err := openWindow(id); err != nil {
					log.Printf("session window: %v", err)
					runtime.ReportWindowFailure()
				}
			})
			runtime.SetPreferencesChanged(applyPreferences)
			if updates != nil {
				runtime.SetUpdates(updates, func() bool { return active == entry })
			}
			runtime.SetPickKeyFile(func(selected func(string)) {
				paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{Parent: entry.window, Title: i18n.In(i18n.Match(mygo.App.Locale()), "ssh_key_file")})
				if err != nil {
					runtime.Report(err)
					return
				}
				if len(paths) > 0 {
					selected(paths[0])
				}
			})
			runtime.SetPickDirectory(func(selected func(string)) {
				paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{Parent: entry.window, Title: i18n.In(i18n.Match(mygo.App.Locale()), "directory_title"), Directory: true})
				if err != nil {
					runtime.Report(err)
					return
				}
				if len(paths) > 0 {
					selected(paths[0])
				}
			})

			if sessionID != "" {
				runtime.OpenSession(sessionID)
			}
		}
		if notifier != nil && notificationWindow == nil {
			notificationWindow = entry.window
			notifier.SetUpdate(notificationWindow.Update)
			notifier.SetLocale(mygo.App.Locale)
			notifier.SetNotify(func(id, title, body, session string) {
				notification := mygo.NewNotification(mygo.NotificationOptions{ID: id, Title: title, Body: body, Group: session})
				go func() {
					if err := notification.ShowContext(ctx); err != nil {
						log.Printf("desktop notification: %v", err)
					}
				}()
			})
		}
		windows = append(windows, entry)
		active = entry
		entry.window.OnFocus(func() { active = entry; refreshCommands() })
		entry.window.OnBlur(entry.save)
		entry.window.OnClose(func(*mygo.CloseEvent) { entry.save() })
		entry.window.OnClosed(func() {
			if entry.live != nil {
				if err := entry.live.Close(); err != nil {
					log.Printf("desktop close: %v", err)
				}
			}
			for index, candidate := range windows {
				if candidate == entry {
					windows = append(windows[:index], windows[index+1:]...)
					break
				}
			}
			if active == entry {
				active = nil
				if len(windows) > 0 {
					active = windows[len(windows)-1]
				}
			}
			refreshCommands()
		})
		applyPreferences()
		return nil
	}
	var composition error
	mygo.App.WhenReady(func() {
		if backend.Preferences != nil {
			mygo.App.SetLocale(preferenceLocale(backend.Preferences.Data.Language))
		}
		if err := openWindow(""); err != nil {
			// Without its first window nothing could quit the application.
			composition = fmt.Errorf("desktop composition: %w", err)
			mygo.App.Quit()
			return
		}
		if updates != nil {
			updates.Service.Startup()
		}
		mygo.App.OnNotificationClick(func(id string) {
			if active == nil && !options.Preview {
				if err := openWindow(""); err != nil {
					log.Printf("desktop composition: %v", err)
				}
			}
			if active == nil || active.live == nil {
				return
			}
			active.window.Show()
			active.window.Focus()
			active.live.OpenNotice(id)
		})
		mygo.App.OnWillQuit(func(*mygo.QuitEvent) {
			if speech != nil {
				if err := speech.Close(); err != nil {
					log.Printf("reader shutdown: %v", err)
				}
			}
			if notifier != nil {
				notifier.Close()
			}
			for _, entry := range windows {
				entry.save()
				if entry.live != nil {
					if err := entry.live.Close(); err != nil {
						log.Printf("desktop close: %v", err)
					}
				}
			}
			if err := backend.Close(); err != nil {
				log.Printf("Node shutdown: %v", err)
			}
		})
	})
	if err := mygo.App.Run(); err != nil {
		return err
	}
	return composition
}

func preferenceLocale(language *string) string {
	if language == nil || *language == "system" {
		return i18n.MatchPreferred(mygo.App.PreferredLanguages()...)
	}
	return map[string]string{"english": "en", "chinese": "zh-CN", "traditional_chinese": "zh-TW", "japanese": "ja", "korean": "ko", "french": "fr", "german": "de", "spanish": "es", "portuguese_brazil": "pt-BR", "russian": "ru"}[*language]
}
