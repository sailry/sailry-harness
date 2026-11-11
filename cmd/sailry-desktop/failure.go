package main

import (
	"errors"
	"time"

	"github.com/egoist/mygo"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
)

// reportStartFailure shows why the Desktop cannot start in a native message,
// the only thing this process shows, and returns the failure for the log.
func reportStartFailure(failure error) error {
	mygo.App.SetLanguages(i18n.Languages...)
	mygo.App.WhenReady(func() {
		message, detail := startFailureCopy(i18n.MatchPreferred(mygo.App.PreferredLanguages()...), failure)
		_, _ = mygo.Dialog.Message(mygo.MessageOptions{Type: mygo.MessageError, Message: message, Detail: detail})
		mygo.App.Quit()
	})
	return errors.Join(failure, mygo.App.Run())
}

// startFailureCopy names a profile another process owns; any other failure
// shows its own reason.
func startFailureCopy(lang string, failure error) (message, detail string) {
	var owned *core.ProfileInUseError
	if errors.As(failure, &owned) {
		return i18n.In(lang, "start_profile_in_use"), owned.Path
	}
	return i18n.In(lang, "start_failed"), failure.Error()
}

// successorVariable marks a Desktop started by the one it replaces: the
// updater starts the new application before the old one has quit.
const successorVariable = "SAILRY_DESKTOP_SUCCESSOR"

// A successor waits this long for the Desktop it replaces to quit: that
// Desktop's Node is stopped already, so only its windows remain.
const (
	successorAttempts = 100
	successorPause    = 100 * time.Millisecond
)

func successorWait() { time.Sleep(successorPause) }

// instanceLock takes the default profile's single-instance lock. A launch
// beside a running Desktop is handed to that Desktop and reports false. A
// successor would be handed to the Desktop that is quitting and exit with
// it, leaving none: it asks again until that one has quit, and that one
// ignores the hand-overs (desktopBackend.stopped).
func instanceLock(request func() bool, successor bool, pause func()) bool {
	if request() {
		return true
	}
	for attempt := 0; successor && attempt < successorAttempts; attempt++ {
		pause()
		if request() {
			return true
		}
	}
	return false
}
