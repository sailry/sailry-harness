package main

import (
	"errors"

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
