package application

import (
	"context"
	"fmt"
	"log"
	"os"
	"runtime"
	"strings"

	"github.com/egoist/mygo"
	_ "github.com/egoist/mygo/locales/ja"
	_ "github.com/egoist/mygo/locales/zh-Hans"
	_ "github.com/egoist/mygo/locales/zh-Hant"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/kit/crychic"
	"github.com/sailry/sailry-harness/internal/desktop/live"
	"github.com/sailry/sailry-harness/internal/desktop/picker"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/reader"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/identity"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/computer"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
)

type Options struct {
	Identity identity.Desktop
	Assembly shell.Assembly
	Host     moduleapi.Assembly
	// MyGo uses NativeName for preferences and its instance lock. An empty
	// value keeps the name MyGo derives from the bundle or executable.
	NativeName string
}

type desktopWindow struct {
	window *mygo.Window
	app    *shell.App
	live   *live.Runtime
	save   func()
	// passing is a confirmed close of this window (exitGuard).
	passing     bool
	closeSaving bool
}

// Run starts the Desktop from the main goroutine and returns after shutdown.
func Run(args []string, config Options) error {
	computer.Trampoline()
	if config.Identity.Version != "" {
		link.ProductVersion = config.Identity.Version
	}
	options, err := parseOptions(args)
	if err != nil {
		return err
	}
	configureMyGo(mygo.App, config)
	product := config.Identity
	name := product.Name
	if name == "" {
		name = product.Release.Package.Name
	}
	if name == "" {
		name = mygo.App.Name()
	}
	if options.AcceptanceUI {
		return runAcceptanceUI(options, config.Assembly)
	}
	serveProfiles()
	successor := os.Getenv(successorVariable) != ""
	_ = os.Unsetenv(successorVariable)
	if !options.Preview && options.DataDir == "" && !instanceLock(mygo.App.RequestSingleInstanceLock, successor, successorWait) {
		// The Desktop that owns the default profile was told and raises its window.
		return nil
	}
	if !options.Preview {
		useSystemIcons()
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	backend, err := openBackend(ctx, options, product, config.Host)
	if err != nil {
		return reportStartFailure(name, err)
	}
	defer func() {
		if err := backend.Close(); err != nil {
			log.Printf("Node shutdown: %v", err)
		}
	}()
	mygo.App.SetLanguages(i18n.Languages...)
	crychic.Setup()
	if backend.Node != nil {
		// A schedule that became due during sleep fires at the wake.
		mygo.Power.OnResume(backend.Node.CheckSchedules)
	}
	var notifier *live.Notifier
	if backend.Client != nil {
		notifier = live.NewNotifier(backend.Client, backend.Preferences)
		// Remote hosts' notices take the same channels as the local Node's.
		notifier.SetNetwork(backend.Network)
		defer notifier.Close()
	}

	var windows []*desktopWindow
	windowPrefs := windowPreferences{write: shell.SavePrefs}
	defer windowPrefs.flush()
	var active *desktopWindow
	var speech *reader.Service
	system := systemShortcuts{register: mygo.GlobalShortcut.Register, unregister: mygo.GlobalShortcut.Unregister}
	guard := &exitGuard{
		windows: func() []*desktopWindow { return windows },
		active:  func() *desktopWindow { return active },
		documents: func(entry *desktopWindow) []shell.CloseDocument {
			if entry.live == nil {
				return nil
			}
			return entry.live.UnsavedDocuments()
		},
		save:   func(entry *desktopWindow) { entry.live.SaveDocuments() },
		saving: func(entry *desktopWindow) bool { return entry.live != nil && entry.live.SavingDocuments() },
		flush: func(entry *desktopWindow, done func(bool)) bool {
			return entry.live != nil && !backend.stopped.Load() && entry.live.FlushEdits(done)
		},
		work: func(entry *desktopWindow) bool { return entry.live != nil && entry.live.LocalWork() },
		raise: func(entry *desktopWindow) {
			entry.window.Show()
			entry.window.Focus()
			entry.window.Invalidate()
		},
		// The prompt answers inside the window's frame, which must end
		// before the window goes.
		shut: func(entry *desktopWindow) { go entry.window.Close() },
		quit: mygo.App.Quit,
	}
	// Installation retains pending native input and waits for every
	// window's read-only frame before stopping the Node. Its successful
	// handoff therefore needs no further exit prompt.
	updates := backend.updates(product, func() { mygo.RunOnMain(guard.pass) })
	if updates != nil {
		defer updates.Service.Close()
	}
	var restart *profileRestart
	if backend.Client != nil {
		restart = newProfileRestart(backend, func() {
			guard.passing = true
			mygo.App.Relaunch()
		})
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
	var noticeTray *windowsNoticeTray
	applyPreferences := func() {
		if backend.Preferences != nil {
			mygo.App.SetLocale(preferenceLocale(backend.Preferences.Data.Language))
			if noticeTray != nil && ctx.Err() == nil {
				if err := noticeTray.configure(backend.Preferences.Data.Notifications[0], i18n.Match(mygo.App.Locale())); err != nil {
					log.Printf("notification tray: %v", err)
				}
			}
		}
		refreshCommands()
	}
	var openWindow func(string) error
	openWindow = func(sessionID string) error {
		entry := &desktopWindow{}
		if options.Preview {
			entry.app = shell.NewWithAssembly(preview.Load(), config.Assembly)
		} else {
			runtime, err := live.NewWithAssembly(ctx, backend.Client, backend.Preferences, config.Assembly)
			if err != nil {
				return err
			}
			entry.live = runtime
			runtime.SetNetwork(backend.Network)
			runtime.SetLogFolder(logging.Directory())
			entry.app = entry.live.App
		}
		app := entry.app
		applyProduct(app, product, name)
		app.OnMode = func(mode theme.Mode) {
			mygo.Theme.SetSource(map[theme.Mode]mygo.ThemeSource{theme.System: mygo.ThemeSystem, theme.Light: mygo.ThemeLight, theme.Dark: mygo.ThemeDark}[mode])
		}
		if windowPrefs.directory != "" {
			app.SetPrefs(windowPrefs.load())
		}
		if sessionID == "" && product.DefaultEntry != "" {
			app.Navigate(shell.Page(product.DefaultEntry))
		}
		entry.save = func() {
			if windowPrefs.directory != "" {
				windowPrefs.save(app.Prefs())
			}
		}
		present := nativeConfirmations(entry, backend.reports.pause)
		entry.window = mygo.NewWindow(mygo.WindowOptions{
			Title: name, Width: 1280, Height: 820, MinWidth: 1024, MinHeight: 768,
			StateKey: "main", TitleBarStyle: mygo.TitleBarHidden,
			// Windows' caption buttons fill the header's height and Linux's
			// sit centered in it, as macOS's lights do at (12, 16).
			TitleBarHeight: int(shell.HeaderHeight), TrafficLightPosition: &mygo.Point{X: 12, Y: 16}, Vibrancy: mygo.VibrancySidebar, Content: ui.View(func(c *ui.Context) {
				kit.BindConfirmations(c, present)
				app.View(c)
			}),
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
				speech.SetSynthesizer(backend.Client.SynthesizeSpeech)
				speech.OnStarted(runtime.CountReadAloud)
			}
			bindConversation(ctx, entry, speech, backend.reports.pause)
			bindFiles(entry, backend.Client.Target(), backend.reports.pause)
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
			runtime.SetRestartProfile(func(restored string, refused func(error)) {
				restart.run(restored, refused, func(proceed func()) {
					if !guard.ask(entry, restartProfile, proceed) {
						proceed()
					}
				})
			})
			if updates != nil {
				runtime.SetUpdates(updates, func() bool { return active == entry }, func() error {
					// MyGo defers hidden/occluded windows' requested frames.
					// CapturePage is its public way to build that pending frame;
					// installation needs the editor lock applied before shutdown.
					// The image is discarded and never stored or published.
					_, err := entry.window.CapturePage()
					return err
				})
			}
			runtime.SetPickKeyFile(func(selected func(string)) {
				resume := backend.reports.pause()
				paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{Parent: entry.window, Title: i18n.In(i18n.Match(mygo.App.Locale()), "ssh_key_file")})
				resume()
				if err != nil {
					runtime.Report(err)
					return
				}
				if len(paths) > 0 {
					selected(paths[0])
				}
			})
			runtime.SetPickDirectory(func(selected func(string)) {
				resume := backend.reports.pause()
				paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{Parent: entry.window, Title: i18n.In(i18n.Match(mygo.App.Locale()), "directory_title"), Directory: true})
				resume()
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
		windows = append(windows, entry)
		active = entry
		backend.reports.watch(entry.window, app)
		app.OnFeatureMenuChanged = func() {
			if active == entry {
				refreshCommands()
			}
		}
		entry.window.OnFocus(func() {
			active = entry
			refreshCommands()
			if notifier != nil {
				notifier.Returned()
			}
		})
		entry.window.OnBlur(entry.save)
		entry.window.OnClose(func(e *mygo.CloseEvent) {
			entry.save()
			if guard.closing(entry) {
				e.PreventDefault()
			}
		})
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
	raiseWindow := func() *desktopWindow {
		if backend.stopped.Load() || guard.passing {
			return nil
		}
		if active == nil && !options.Preview {
			if err := openWindow(""); err != nil {
				log.Printf("desktop composition: %v", err)
			}
		}
		if active != nil {
			guard.raise(active)
		}
		return active
	}
	if runtime.GOOS == "windows" && notifier != nil {
		noticeTray = &windowsNoticeTray{applicationID: product.Release.Package.ID, name: name, openCenter: func() {
			if entry := raiseWindow(); entry != nil {
				entry.app.Navigate(shell.NotificationsPage)
				entry.window.Invalidate()
			}
		}}
	}
	var composition error
	mygo.App.WhenReady(func() {
		windowPrefs.directory, _ = mygo.App.Path(mygo.PathUserData)
		if backend.Preferences != nil {
			mygo.App.SetLocale(preferenceLocale(backend.Preferences.Data.Language))
		}
		if err := openWindow(""); err != nil {
			// Without its first window nothing could quit the application.
			composition = fmt.Errorf("desktop composition: %w", err)
			mygo.App.Quit()
			return
		}
		if notifier != nil {
			notifier.SetLocale(mygo.App.Locale)
			// A focused window shows the notice itself; the system
			// notification is for an application in the background.
			notifier.SetPresent(func(notice *viewmodel.Notice) bool {
				for _, candidate := range windows {
					if candidate.live != nil && candidate.window.IsFocused() {
						candidate.live.PresentNotice(notice)
						return true
					}
				}
				return false
			})
			show := func(opts mygo.NotificationOptions) error { return mygo.NewNotification(opts).ShowContext(ctx) }
			notifier.SetNotify(func(id, title, body, session string) {
				opts := mygo.NotificationOptions{ID: id, Title: title, Body: body, Group: session}
				result := func(err error) { completeSystemNotification(ctx, id, err, notifier.Undelivered) }
				if noticeTray != nil {
					result(noticeTray.replace(opts, show))
				} else {
					go func() { result(show(opts)) }()
				}
			})
			notifier.SetUpdate(mygo.App.WhenReady)
			if noticeTray != nil {
				mygo.App.OnQuit(func() {
					cancel()
					noticeTray.remove()
				})
			}
		}
		if updates != nil {
			updates.Service.Startup()
		}
		// Another launch of the default profile hands over to this one.
		mygo.App.OnSecondInstance(func([]string, string) { raiseWindow() })
		mygo.App.OnNotificationClick(func(id string) {
			if noticeTray != nil {
				id = noticeTray.logicalID(id)
				if id == "" {
					return
				}
			}
			if entry := raiseWindow(); entry != nil && entry.live != nil {
				if noticeTray != nil {
					// A notice without a session opens the inbox rather than the previous page.
					entry.app.Navigate(shell.NotificationsPage)
				}
				entry.live.OpenNotice(id)
			}
		})
		mygo.App.OnBeforeQuit(func(e *mygo.QuitEvent) {
			if guard.beforeQuit(e.SessionEnding) {
				e.PreventDefault()
			}
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
			windowPrefs.flush()
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

func configureMyGo(app *mygo.Application, config Options) {
	if config.NativeName != "" {
		app.SetName(config.NativeName)
	}
	app.SetVersion(config.Identity.Version)
}

func defaultWelcomeMark(product identity.Desktop) string {
	if mark := strings.TrimSpace(preferences.CleanWelcomeMark(product.WelcomeMark)); mark != "" {
		return mark
	}
	switch product.ID {
	case "harness":
		return "SAILRY HARNESS"
	case "code":
		return "SAILRY CODE"
	default:
		return ""
	}
}

func applyProduct(app *shell.App, product identity.Desktop, name string) {
	app.Settings.ProductName, app.Settings.Version = name, product.Version
	app.Website, app.Feedback = product.Website, product.Feedback
	app.Messages.DefaultMark = defaultWelcomeMark(product)
}

func preferenceLocale(language *string) string {
	if language == nil || *language == "system" {
		return i18n.MatchPreferred(mygo.App.PreferredLanguages()...)
	}
	return map[string]string{"english": "en", "chinese": "zh-CN", "traditional_chinese": "zh-TW", "japanese": "ja", "korean": "ko", "french": "fr", "german": "de", "spanish": "es", "portuguese_brazil": "pt-BR", "russian": "ru"}[*language]
}

// useSystemIcons gives the file picker the file manager's icons. A remote
// folder shows a plain local folder's icon: the temporary directory is one
// on every desktop system.
func useSystemIcons() {
	picker.SystemIcons.File = mygo.Shell.FileIcon
	picker.SystemIcons.Type = mygo.Shell.TypeIcon
	picker.SystemIcons.Folder = func(px int) ([]byte, error) { return mygo.Shell.FileIcon(os.TempDir(), px) }
}
