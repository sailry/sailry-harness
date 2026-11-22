package application

import (
	"errors"
	"os"

	"github.com/egoist/mygo"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
)

// The Windows shell slot belongs to the process, not to any one window or
// inbox entry. All its operations run on the UI thread.
type windowsNoticeTray struct {
	applicationID string
	name          string
	native        *mygo.Tray
	language      string
	latest        string
	openCenter    func()
}

func (t *windowsNoticeTray) balloonID() string { return t.applicationID + ".notification" }

func (t *windowsNoticeTray) menu(language string) *mygo.Menu {
	return mygo.NewMenu([]*mygo.MenuItem{
		{Label: i18n.In(language, "notification_center"), Click: func(*mygo.MenuItem, *mygo.Window) { t.openCenter() }},
		{Label: i18n.In(language, "menu_quit", "app", t.name), Role: mygo.RoleQuit},
	})
}

func (t *windowsNoticeTray) configure(enabled bool, language string) error {
	if !enabled {
		t.remove()
		return nil
	}
	if t.native != nil {
		if language != t.language {
			t.native.SetMenu(t.menu(language))
			t.language = language
		}
		return nil
	}

	exe, err := os.Executable()
	if err != nil {
		return err
	}
	icon, err := mygo.Shell.FileIcon(exe, 32)
	if err != nil {
		return err
	}
	tray, err := mygo.NewTray(mygo.TrayOptions{ID: t.applicationID + ".tray", ToolTip: t.name, Menu: t.menu(language)})
	if err != nil {
		return err
	}
	if err := tray.SetIcon(icon, false); err != nil {
		tray.Destroy()
		return err
	}
	tray.OnClick(t.openCenter)
	// MyGo normally posts the icon registration. Bounds registers it now,
	// before a queued notification can allocate its own temporary icon.
	tray.Bounds()
	t.native, t.language = tray, language
	return nil
}

func (t *windowsNoticeTray) remove() {
	if t.native != nil {
		mygo.NewNotification(mygo.NotificationOptions{ID: t.balloonID()}).Close()
		t.native.Destroy()
		t.native, t.latest = nil, ""
	}
}

func (t *windowsNoticeTray) replace(opts mygo.NotificationOptions, show func(mygo.NotificationOptions) error) error {
	if t.native == nil {
		return errors.New("notification tray is unavailable")
	}
	id := opts.ID
	opts.ID = t.balloonID()
	// Windows delivers synchronously. Reusing the shell ID frees the old
	// balloon before showing this one, even if the replacement fails.
	t.latest = ""
	if err := show(opts); err != nil {
		return err
	}
	t.latest = id
	return nil
}

func (t *windowsNoticeTray) logicalID(systemID string) string {
	if systemID == t.balloonID() {
		return t.latest
	}
	return ""
}
