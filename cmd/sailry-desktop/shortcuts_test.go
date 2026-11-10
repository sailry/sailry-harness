package main

import (
	"errors"
	"maps"
	"reflect"
	"slices"
	"testing"
)

// The system's registrations follow the bound keys: registered at launch,
// moved on a rebind, removed on a clear, and a refused key reported once.
func TestSystemShortcutRegistration(t *testing.T) {
	held := map[string]func(){}
	refused := map[string]bool{}
	var calls []string
	system := systemShortcuts{
		register: func(accelerator string, run func()) error {
			if refused[accelerator] {
				calls = append(calls, "refuse "+accelerator)
				return errors.New("taken by another application")
			}
			if held[accelerator] != nil {
				t.Fatalf("%s registered over its own registration", accelerator)
			}
			calls = append(calls, "register "+accelerator)
			held[accelerator] = run
			return nil
		},
		unregister: func(accelerator string) {
			calls = append(calls, "unregister "+accelerator)
			delete(held, accelerator)
		},
	}
	var ran []string
	conflicts := 0
	apply := func(shortcuts map[string]string) []string {
		calls = nil
		system.apply(shortcuts, func(command string) { ran = append(ran, command) }, func() { conflicts++ })
		return calls
	}
	keys := func() []string { return slices.Sorted(maps.Keys(held)) }

	// Launch registers the bound key; pressing it sends its command.
	if got := apply(map[string]string{"Ctrl+Alt+Space": "system.activate"}); !reflect.DeepEqual(got, []string{"register Ctrl+Alt+Space"}) {
		t.Fatalf("launch: %v", got)
	}
	held["Ctrl+Alt+Space"]()
	if !reflect.DeepEqual(ran, []string{"system.activate"}) {
		t.Fatalf("pressed key ran %v", ran)
	}
	// A rebind releases the old key and registers the new one.
	apply(map[string]string{"Ctrl+Alt+K": "system.activate", "Ctrl+Alt+S": "system.selection"})
	if got := keys(); !reflect.DeepEqual(got, []string{"Ctrl+Alt+K", "Ctrl+Alt+S"}) {
		t.Fatalf("after a rebind the system holds %v", got)
	}
	held["Ctrl+Alt+S"]()
	if ran[len(ran)-1] != "system.selection" {
		t.Fatalf("pressed key ran %v", ran)
	}
	// An unchanged key is registered again without a second registration
	// beside the first.
	if got := apply(map[string]string{"Ctrl+Alt+K": "system.activate", "Ctrl+Alt+S": "system.selection"}); len(got) != 4 || len(held) != 2 {
		t.Fatalf("refresh: %v, held %v", got, keys())
	}
	// A cleared key is unregistered.
	if got := apply(map[string]string{"Ctrl+Alt+K": "system.activate"}); !reflect.DeepEqual(keys(), []string{"Ctrl+Alt+K"}) {
		t.Fatalf("clear: %v, held %v", got, keys())
	}
	// A key the system refuses is reported once, however often the keys
	// are applied, and again only after it was unbound.
	refused["Ctrl+Alt+S"] = true
	taken := map[string]string{"Ctrl+Alt+K": "system.activate", "Ctrl+Alt+S": "system.selection"}
	apply(taken)
	apply(taken)
	if conflicts != 1 || !reflect.DeepEqual(keys(), []string{"Ctrl+Alt+K"}) {
		t.Fatalf("refused key: %d reports, held %v", conflicts, keys())
	}
	apply(map[string]string{"Ctrl+Alt+K": "system.activate"})
	if apply(taken); conflicts != 2 {
		t.Fatalf("a key refused again after it was unbound: %d reports", conflicts)
	}
	// Once the other application lets go, the key registers.
	delete(refused, "Ctrl+Alt+S")
	if apply(taken); conflicts != 2 || !reflect.DeepEqual(keys(), []string{"Ctrl+Alt+K", "Ctrl+Alt+S"}) {
		t.Fatalf("freed key: %d reports, held %v", conflicts, keys())
	}
	if apply(nil); len(held) != 0 {
		t.Fatalf("no window left %v registered", keys())
	}
}
