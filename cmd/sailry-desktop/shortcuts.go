package main

import "log"

// systemShortcuts keeps the operating system's registrations equal to the
// active window's system shortcuts. register and unregister are
// mygo.GlobalShortcut's; a test replaces them.
type systemShortcuts struct {
	register   func(accelerator string, run func()) error
	unregister func(accelerator string)
	registered map[string]bool
	conflicted map[string]bool
}

// apply registers shortcuts, accelerator to command, and unregisters the
// keys no longer bound. run receives the command of a pressed key. conflict
// runs once for each key the system refuses, until that key is unbound.
func (s *systemShortcuts) apply(shortcuts map[string]string, run func(command string), conflict func()) {
	if s.registered == nil {
		s.registered, s.conflicted = map[string]bool{}, map[string]bool{}
	}
	for accelerator := range s.registered {
		if _, exists := shortcuts[accelerator]; !exists {
			s.unregister(accelerator)
			delete(s.registered, accelerator)
		}
	}
	for accelerator := range s.conflicted {
		if _, exists := shortcuts[accelerator]; !exists {
			delete(s.conflicted, accelerator)
		}
	}
	for accelerator, command := range shortcuts {
		if s.registered[accelerator] {
			s.unregister(accelerator)
		}
		if err := s.register(accelerator, func() { run(command) }); err != nil {
			log.Printf("system shortcut %s: %v", accelerator, err)
			delete(s.registered, accelerator)
			if !s.conflicted[accelerator] {
				conflict()
			}
			s.conflicted[accelerator] = true
		} else {
			s.registered[accelerator], s.conflicted[accelerator] = true, false
		}
	}
}
