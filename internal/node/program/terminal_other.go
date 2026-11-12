//go:build !darwin && !linux

package program

// Terminal reports whether StartTerminal works on this system.
const Terminal = false

// StartTerminal is Start for a program that needs a terminal instead of
// pipes. This system has none to give.
func StartTerminal(Spec, uint16, uint16) (*Process, error) { return nil, ErrNoTerminal }
