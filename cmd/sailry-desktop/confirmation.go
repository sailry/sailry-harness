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
			options, indexes := nativeConfirmationOptions(request)
			options.Parent = entry.window
			result, err := mygo.Dialog.Message(options)
			choice := request.Cancel
			if err != nil {
				log.Printf("native confirmation: %v", err)
			} else if result.Button >= 0 && result.Button < len(indexes) {
				choice = indexes[result.Button]
			}
			entry.window.Update(func() {
				if !entry.window.IsDestroyed() {
					answer(choice)
				}
			})
		}()
	}
}

func nativeConfirmationOptions(request kit.Confirmation) (mygo.MessageOptions, []int) {
	options := mygo.MessageOptions{Type: mygo.MessageQuestion, Title: request.Title, Message: request.Title, Detail: request.Message,
		DefaultButton: -1, CancelButton: -1}
	indexes := make([]int, 0, len(request.Buttons))
	if request.Default >= 0 && request.Default < len(request.Buttons) {
		indexes = append(indexes, request.Default)
	}
	for index := range request.Buttons {
		if index != request.Default {
			indexes = append(indexes, index)
		}
	}
	for native, original := range indexes {
		options.Buttons = append(options.Buttons, request.Buttons[original])
		if original == request.Default {
			options.DefaultButton = native
		}
		if original == request.Cancel {
			options.CancelButton = native
		}
		for _, destructive := range request.Destructive {
			// The chosen action is deliberately the default, even for Delete.
			// AppKit's destructive marker can suppress its Return key binding;
			// keep that marker only on alternate actions such as Discard.
			if original == destructive && original != request.Default {
				options.DestructiveButtons = append(options.DestructiveButtons, native)
			}
		}
	}
	return options, indexes
}
