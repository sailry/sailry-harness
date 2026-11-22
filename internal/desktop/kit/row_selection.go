package kit

import (
	"slices"

	"github.com/egoist/mygo/ui"
)

// RowSelection applies list selection gestures to rows rendered by custom layouts.
// The caller supplies the current visible order, including any filtering or sorting.
type RowSelection[K comparable] struct {
	ui.Selection[K]
	anchor   K
	anchored bool
}

func (s *RowSelection[K]) Reset() { *s = RowSelection[K]{} }

func (s *RowSelection[K]) Click(key K, order []K, mods ui.Modifiers) {
	toggle := mods&(ui.Ctrl|ui.Cmd) != 0
	if mods&ui.Shift != 0 && s.anchored {
		a, b := slices.Index(order, s.anchor), slices.Index(order, key)
		if a >= 0 && b >= 0 {
			if !toggle {
				s.Clear()
			}
			for _, k := range order[min(a, b) : max(a, b)+1] {
				s.Add(k)
			}
			return
		}
	}
	s.anchor, s.anchored = key, true
	if !toggle {
		s.Clear()
	}
	if toggle && s.Has(key) {
		s.Remove(key)
	} else {
		s.Add(key)
	}
}

func (s *RowSelection[K]) Context(key K, order []K) []K {
	if !s.Has(key) {
		s.Click(key, order, 0)
	}
	seen := make(map[K]bool, s.Len())
	var selected []K
	for _, k := range order {
		if s.Has(k) && !seen[k] {
			selected = append(selected, k)
			seen[k] = true
		}
	}
	return selected
}
