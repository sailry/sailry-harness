package panel

import (
	"slices"
	"strings"
)

// selection is the file tree's choice of rows, as the files plugin's
// explorer.js keeps it: the rows selected, the cursor and the anchor a
// Shift range starts from. Rows are by path; visible are the rows shown,
// in order.
type selection struct {
	selected        map[string]bool
	current, anchor string
}

// pick selects path as a click with these keys does: alone; added or
// taken away with Cmd (Ctrl elsewhere); with Shift the rows from the
// anchor to it, added to those selected with Cmd too.
func (s *selection) pick(visible []string, path string, shift, additive bool) {
	i := slices.Index(visible, path)
	if i < 0 {
		return
	}
	switch {
	case shift:
		a := max(slices.Index(visible, s.anchor), 0)
		if s.anchor == "" {
			a = i
		}
		if !additive {
			clear(s.selected)
		}
		for _, p := range visible[min(a, i) : max(a, i)+1] {
			s.selected[p] = true
		}
	case additive:
		if s.selected[path] {
			delete(s.selected, path)
		} else {
			s.selected[path] = true
		}
		s.anchor = path
	default:
		clear(s.selected)
		s.selected[path] = true
		s.anchor = path
	}
	s.current = path
}

// all selects every row shown.
func (s *selection) all(visible []string) {
	for _, p := range visible {
		s.selected[p] = true
	}
	if paths := s.paths(visible); len(paths) > 0 {
		s.anchor = paths[0]
	}
}

// context is a right click on path: a row outside the selection becomes
// the selection.
func (s *selection) context(visible []string, path string) {
	if !slices.Contains(s.paths(visible), path) {
		clear(s.selected)
		if path != "" {
			s.selected[path] = true
		}
	}
	s.current, s.anchor = path, path
}

// paths are the rows selected that show, sorted.
func (s *selection) paths(visible []string) []string {
	var out []string
	for _, p := range visible {
		if s.selected[p] {
			out = append(out, p)
		}
	}
	slices.Sort(out)
	return out
}

// targets are the rows an action on path acts on: the selection when it
// holds path, less the rows inside a folder also selected; else path.
func (s *selection) targets(visible []string, path string) []string {
	if path == "" {
		return nil
	}
	paths := s.paths(visible)
	if !slices.Contains(paths, path) {
		return []string{path}
	}
	var out []string
	for _, p := range paths {
		inside := false
		for d := parentDir(p); d != "" && !inside; d = parentDir(d) {
			inside = s.selected[d] && slices.Contains(paths, d)
		}
		if !inside {
			out = append(out, p)
		}
	}
	return out
}

// within reports whether path is dir or inside it.
func within(path, dir string) bool { return path == dir || strings.HasPrefix(path, dir+"/") }
