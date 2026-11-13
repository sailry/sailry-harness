package main

import (
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

func TestNativeConfirmationDefaultAndCancelMapping(t *testing.T) {
	for _, test := range []struct {
		name    string
		request kit.Confirmation
		buttons []string
		indexes []int
		cancel  int
		danger  []int
	}{
		{"delete", kit.Confirmation{Buttons: []string{"Cancel", "Delete"}, Default: 1, Cancel: 0, Destructive: []int{1}}, []string{"Delete", "Cancel"}, []int{1, 0}, 1, nil},
		{"save", kit.Confirmation{Buttons: []string{"Cancel", "Discard", "Save"}, Default: 2, Cancel: 0, Destructive: []int{1}}, []string{"Save", "Cancel", "Discard"}, []int{2, 0, 1}, 1, []int{2}},
		{"unwritable", kit.Confirmation{Buttons: []string{"Cancel", "Discard"}, Default: 1, Cancel: 0, Destructive: []int{1}}, []string{"Discard", "Cancel"}, []int{1, 0}, 1, nil},
	} {
		t.Run(test.name, func(t *testing.T) {
			test.request.Title, test.request.Message = "Confirm", "Named consequence"
			options, indexes := nativeConfirmationOptions(test.request)
			if !slices.Equal(options.Buttons, test.buttons) || !slices.Equal(indexes, test.indexes) || options.DefaultButton != 0 || options.CancelButton != test.cancel || !slices.Equal(options.DestructiveButtons, test.danger) {
				t.Fatalf("native choices changed meaning: options=%+v indexes=%v", options, indexes)
			}
			if indexes[options.DefaultButton] != test.request.Default || indexes[options.CancelButton] != test.request.Cancel {
				t.Fatal("Return or Escape mapped to another action")
			}
			for native, original := range indexes {
				if options.Buttons[native] != test.request.Buttons[original] {
					t.Fatal("native button selection changed the caller's action index")
				}
			}
			if options.Title != test.request.Title || options.Message != test.request.Title || options.Detail != test.request.Message {
				t.Fatal("native confirmation lost its title or consequence")
			}
		})
	}
}
