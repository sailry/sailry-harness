package sidebar

import (
	"slices"
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

type sidebarSelection struct {
	kit.RowSelection[string]
	anchor   sessionPosition
	current  string
	archived bool
}

type sessionPosition struct {
	key  string
	kind rowKind
}

type sessionRow struct {
	sessionPosition
	conversation *preview.Conversation
}

func selectionKey(cv *preview.Conversation) string {
	return strconv.Itoa(len(cv.Node)) + ":" + cv.Node + cv.ID
}

func selectingRows(mods ui.Modifiers) bool { return mods&(ui.Shift|ui.Ctrl|ui.Cmd) != 0 }

func (s *State) syncRows(current *preview.Conversation) {
	var key string
	if current != nil {
		key = selectionKey(current)
	}
	if s.rows.current == key && s.rows.archived == s.Archived {
		return
	}
	s.rows.Reset()
	s.rows.anchor = sessionPosition{}
	s.rows.current, s.rows.archived = key, s.Archived
	if current != nil && current.Archived == s.Archived {
		s.rows.Click(key, nil, 0)
		s.rows.anchor = sessionPosition{key, rowAny}
	}
}

func (s *State) clickRow(cv *preview.Conversation, position sessionPosition, mods ui.Modifiers) {
	if !selectingRows(mods) {
		s.show(cv)
		s.rows.current = position.key
	}
	if mods&ui.Shift != 0 {
		rows := s.visibleSessionRows()
		if anchor, clicked := rowIndex(rows, s.rows.anchor), rowIndex(rows, position); anchor >= 0 && clicked >= 0 {
			key, order := rangeOrder(rows, anchor, clicked)
			s.rows.Click(key, order, mods)
			return
		}
	}
	s.rows.Click(position.key, nil, mods)
	s.rows.anchor = position
}

func rowIndex(rows []sessionRow, position sessionPosition) int {
	if at := slices.IndexFunc(rows, func(row sessionRow) bool { return row.sessionPosition == position }); at >= 0 {
		return at
	}
	return slices.IndexFunc(rows, func(row sessionRow) bool { return row.key == position.key })
}

// Both copies of a pinned session have one selection key. Keep each range
// endpoint at the position clicked, so the selector's first key match works
// even when the same conversation also appears elsewhere.
func rangeOrder(rows []sessionRow, anchor, clicked int) (string, []string) {
	start := rows[anchor].key
	for anchor != clicked && rows[clicked].key == start {
		// If both endpoints name the same session, that session is already
		// included at the anchor. End at the next row inside the interval.
		if clicked > anchor {
			clicked--
		} else {
			clicked++
		}
	}
	end := rows[clicked].key
	first, last := min(anchor, clicked), max(anchor, clicked)
	order := make([]string, 0, last-first+1)
	for i := first; i <= last; i++ {
		key := rows[i].key
		if (key == start && i != anchor) || (key == end && i != clicked) {
			continue
		}
		order = append(order, key)
	}
	return end, order
}

func (s *State) menuTargets(cv *preview.Conversation) []*preview.Conversation {
	s.syncRows(s.Current())
	key := selectionKey(cv)
	if !s.rows.Has(key) {
		s.rows.anchor = sessionPosition{key, rowAny}
	}
	rows := s.visibleSessionRows()
	order := make([]string, 0, len(rows))
	byKey := make(map[string]*preview.Conversation, len(rows))
	for _, row := range rows {
		order = append(order, row.key)
		byKey[row.key] = row.conversation
	}
	selected := s.rows.Context(key, order)
	targets := make([]*preview.Conversation, 0, len(selected))
	for _, key := range selected {
		targets = append(targets, byKey[key])
	}
	return targets
}

func (s *State) visibleSessionRows() []sessionRow {
	var rows []sessionRow
	add := func(group string, kind rowKind, conversations []*preview.Conversation) {
		n, _ := Window(len(conversations), s.more[group])
		for _, cv := range conversations[:n] {
			rows = append(rows, sessionRow{sessionPosition{selectionKey(cv), kind}, cv})
		}
	}
	if *s.opened("pinned") {
		add("pinned", rowPinned, s.pinned())
	}
	if *s.opened("unassigned") {
		add("unassigned", rowPlain, s.sessions(s.Data.Unassigned))
	}
	if *s.opened("projects") {
		for _, project := range s.projects() {
			if *s.opened("project/" + project.ID) {
				add(project.Host+"/"+project.ID, rowTree, s.sessions(project.Conversations))
			}
		}
	}
	return rows
}
