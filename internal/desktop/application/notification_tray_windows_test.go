package application

import (
	"fmt"
	"os"
	"testing"

	"github.com/egoist/mygo"
)

const windowsTrayTestEnv = "SAILRY_TEST_WINDOWS_TRAY"

func TestMain(m *testing.M) {
	if os.Getenv(windowsTrayTestEnv) != "1" {
		os.Exit(m.Run())
	}
	mygo.App.SetName("Sailry Tray Fixture")
	code := make(chan int, 1)
	mygo.App.WhenReady(func() {
		go func() {
			code <- m.Run()
			mygo.App.Quit()
		}()
	})
	if err := mygo.App.Run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	select {
	case result := <-code:
		os.Exit(result)
	default:
		os.Exit(1)
	}
}

func TestWindowsNativeTrayLifecycle(t *testing.T) {
	if os.Getenv(windowsTrayTestEnv) != "1" {
		t.Skip("set SAILRY_TEST_WINDOWS_TRAY=1 in an interactive Windows desktop session")
	}
	tray := &windowsNoticeTray{applicationID: "example.desktop", name: mygo.App.Name(), openCenter: func() {}}
	t.Cleanup(func() { mygo.RunOnMain(tray.remove) })
	mygo.RunOnMain(func() {
		if err := tray.configure(true, "en"); err != nil {
			t.Error(err)
			return
		}
		first := tray.native
		if bounds := first.Bounds(); bounds.Width == 0 || bounds.Height == 0 {
			t.Errorf("tray was not registered with Explorer: %+v", bounds)
		}
		if err := tray.configure(true, "zh-CN"); err != nil || tray.native != first {
			t.Errorf("updating the menu changed the tray: %v", err)
		}
		for _, id := range []string{"first", "replacement"} {
			notice := mygo.NotificationOptions{ID: id, Title: "Sailry Tray Fixture", Body: id, Silent: true}
			if err := tray.replace(notice, func(opts mygo.NotificationOptions) error {
				return mygo.NewNotification(opts).ShowContext(t.Context())
			}); err != nil {
				t.Error(err)
				return
			}
			if tray.native != first || tray.logicalID(tray.balloonID()) != id {
				t.Errorf("showing %q changed the tray or lost the click target", id)
			}
		}
		if err := tray.configure(false, "zh-CN"); err != nil {
			t.Error(err)
		}
		if tray.native != nil || tray.logicalID(tray.balloonID()) != "" || first.Bounds() != (mygo.Rectangle{}) {
			t.Error("disabling notifications kept the previous tray or click target")
		}
		if err := tray.configure(true, "en"); err != nil {
			t.Error(err)
			return
		}
		last := tray.native
		if bounds := last.Bounds(); last == first || bounds.Width == 0 || bounds.Height == 0 {
			t.Errorf("re-enabling notifications did not register a new tray: %+v", bounds)
		}
		tray.remove()
		if tray.native != nil || last.Bounds() != (mygo.Rectangle{}) {
			t.Error("shutdown cleanup kept the re-enabled tray")
		}
	})
}
