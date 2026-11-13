package kit

import (
	"slices"

	"github.com/egoist/mygo/ui"
)

// Confirmation is a choice presented by the operating system. Button indexes
// keep the caller's action contract; dismissing the dialog chooses Cancel.
type Confirmation struct {
	Title, Message string
	Buttons        []string
	Default        int
	Cancel         int
	Destructive    []int
}

// ConfirmationPresenter presents a native dialog without blocking construction
// of a UI frame. The owner must deliver answer on the window's UI thread.
type ConfirmationPresenter func(Confirmation, func(answer int))

type confirmationKey struct{}

// BindConfirmations binds the native window's dialog service before its view.
// A headless harness leaves this unbound and drives MyGo's in-memory dialog.
func BindConfirmations(c *ui.Context, present ConfirmationPresenter) {
	*ui.Local(c.Root(), confirmationKey{}, func() ConfirmationPresenter { return nil }) = present
}

type confirmationState struct {
	request Confirmation
	pending bool
	ready   bool
	answer  int
	ticket  *bool
}

// Close invalidates a response after the owning UI element leaves the frame.
// MyGo closes element-local values on unmount, including a window's shutdown.
func (state *confirmationState) Close() error {
	*state = confirmationState{}
	return nil
}

// Confirm keeps AlertDialog's frame-based contract while native windows show an
// OS dialog. A changed request cannot consume the answer to an older prompt.
func Confirm(c *ui.Context, open *bool, request Confirmation) int {
	present := *ui.Local(c.Root(), confirmationKey{}, func() ConfirmationPresenter { return nil })
	if present == nil {
		if *open {
			NoteModal(c)
		}
		answer := ui.AlertDialog(c, open, request.Title, request.Message, request.Buttons...)
		if answer == ui.AlertDismissed {
			return request.Cancel
		}
		return answer
	}
	state := ui.Local(ui.Box(c).Absolute().Size(0, 0), "native-confirmation", func() confirmationState { return confirmationState{} })
	if !*open {
		*state = confirmationState{}
		return -1
	}
	NoteModal(c)
	if state.pending || state.ready {
		if state.request.Title != request.Title || state.request.Message != request.Message ||
			state.request.Default != request.Default || state.request.Cancel != request.Cancel ||
			!slices.Equal(state.request.Buttons, request.Buttons) || !slices.Equal(state.request.Destructive, request.Destructive) {
			*state = confirmationState{}
		}
	}
	if state.ready {
		answer := state.answer
		*state = confirmationState{}
		*open = false
		c.Invalidate()
		return answer
	}
	if !state.pending {
		request.Buttons = slices.Clone(request.Buttons)
		request.Destructive = slices.Clone(request.Destructive)
		ticket := new(bool)
		state.request, state.pending, state.ticket = request, true, ticket
		present(request, func(answer int) {
			if state.ticket != ticket || !state.pending {
				return
			}
			if answer < 0 || answer >= len(request.Buttons) {
				answer = request.Cancel
			}
			state.answer, state.ready, state.pending = answer, true, false
		})
	}
	return -1
}
