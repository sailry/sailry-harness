package settings

import "context"

// WindowBinding belongs to one registered settings contribution in a live
// window. Mount owns only the selected Node while the contribution is visible.
type WindowBinding struct {
	Administrator bool
	KeepOnOutage  bool
	// AlsoVisible supports a contribution used by another surface in the window.
	AlsoVisible func() bool
	// Show owns the window-local part of a visible page even if its Node is offline.
	Show    func(context.Context) PageBinding
	Mount   func(PageTarget) PageBinding
	Blocked func(status string)
	Close   func()
}

// PageTarget's context ends when the page hides, changes Node or loses access.
// Current also checks the live selection and access before the next frame.
type PageTarget struct {
	Node    string
	Context context.Context
	Current func() bool
}

// PageBinding follows the shown Node. Close releases presentation state and
// must not cancel an already admitted save.
type PageBinding struct {
	Follow  func()
	Suspend func(status string)
	Recover func()
	Close   func()
}

// BindWindow connects a contribution to its narrow live host interface. Preview
// windows never bind a live host.
func BindWindow[Host any](bind func(Host) WindowBinding) func(any) WindowBinding {
	return func(host any) WindowBinding {
		if value, ok := host.(Host); ok {
			return bind(value)
		}
		return WindowBinding{}
	}
}
