package application

import (
	"errors"
	"testing"

	"github.com/egoist/mygo"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
)

func TestWindowsTrayReplacesBalloonNotInboxID(t *testing.T) {
	tray := &windowsNoticeTray{applicationID: "example.desktop", native: &mygo.Tray{}}
	const balloonID = "example.desktop.notification"
	slots := map[string]string{}
	show := func(opts mygo.NotificationOptions) error {
		slots[opts.ID] = opts.Title
		return nil
	}
	for _, id := range []string{"home", "remote", "last"} {
		notice := mygo.NotificationOptions{ID: id, Title: id}
		if err := tray.replace(notice, show); err != nil {
			t.Fatal(err)
		}
		if len(slots) != 1 || slots[balloonID] != id || tray.logicalID(balloonID) != notice.ID {
			t.Fatalf("after %s: system slots %v, click target %q", id, slots, tray.logicalID(balloonID))
		}
	}
	if tray.logicalID("foreign") != "" {
		t.Fatal("an unrelated notification opened the current notice")
	}
	refused := errors.New("balloon refused")
	if err := tray.replace(mygo.NotificationOptions{ID: "refused"}, func(mygo.NotificationOptions) error { return refused }); !errors.Is(err, refused) || tray.logicalID(balloonID) != "" {
		t.Fatalf("failed replacement: %v, click target %q", err, tray.logicalID(balloonID))
	}
	tray = &windowsNoticeTray{}
	if err := tray.replace(mygo.NotificationOptions{ID: "no-tray"}, func(mygo.NotificationOptions) error {
		t.Fatal("delivery without the tray would create a temporary icon")
		return nil
	}); err == nil {
		t.Fatal("notice was not returned to the in-app channel")
	}
}

func TestWindowsTrayMenu(t *testing.T) {
	opened := 0
	tray := &windowsNoticeTray{name: "ExampleDesktop", openCenter: func() { opened++ }}
	for _, language := range []string{"en", "zh-CN"} {
		items := tray.menu(language).Items()
		if len(items) != 2 || items[0].Label != i18n.In(language, "notification_center") || items[0].Click == nil || items[1].Label != i18n.In(language, "menu_quit", "app", "ExampleDesktop") || items[1].Role != mygo.RoleQuit {
			t.Fatalf("wrong notification center or quit action for %s: %v", language, items)
		}
		items[0].Click(items[0], nil)
	}
	if opened != 2 {
		t.Fatalf("notification center opened %d times", opened)
	}
}
