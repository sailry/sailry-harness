package main

import (
	"log"

	"github.com/egoist/mygo"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// nativeConfirmations attaches every confirmation to its owning native window.
// Dialog.Message waits on the system dialog, so only its result returns to the
// UI thread; the frame and the asynchronous operation stay independently alive.
func nativeConfirmations(entry *desktopWindow) kit.ConfirmationPresenter {
	return func(request kit.Confirmation, answer func(int)) {
		go func() {
			result, err := mygo.Dialog.Message(mygo.MessageOptions{
				Parent: entry.window, Type: mygo.MessageQuestion, Title: request.Title,
				Message: request.Title, Detail: request.Message, Buttons: request.Buttons,
				DefaultButton: request.Default, CancelButton: request.Cancel, DestructiveButtons: request.Destructive,
			})
			choice := result.Button
			if err != nil {
				log.Printf("native confirmation: %v", err)
				choice = request.Cancel
			}
			entry.window.Update(func() {
				if !entry.window.IsDestroyed() {
					answer(choice)
				}
			})
		}()
	}
}
