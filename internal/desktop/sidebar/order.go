package sidebar

import (
	"slices"
	"strings"
	"time"
	"unicode"
)

// The sidebar's rules as pure functions. They take the view model's values
// and no UI, so a backend's data goes through the same rules.

// Initial is how many rows a limited group shows before "Show more"; the
// toggle shows all or these.
const Initial = 5

// Window returns how many of n rows a limited group shows, and whether it
// needs the "Show more" toggle.
func Window(n int, expanded bool) (shown int, more bool) {
	more = n > Initial
	if more && !expanded {
		return Initial, more
	}
	return n, more
}

// Pin is a pinned conversation as the pinned group orders it.
type Pin struct {
	ID      string
	Order   *int // nil for a pin from before pins had an order
	LastRun time.Time
	Created int
}

// OrderPins sorts pins as compose does: pins without an order first, by
// their last run, newest first, then by creation, newest first; then the
// ordered ones by their order. The sort is stable.
func OrderPins(pins []Pin) []Pin {
	out := slices.Clone(pins)
	slices.SortStableFunc(out, func(a, b Pin) int {
		switch {
		case (a.Order == nil) != (b.Order == nil):
			if a.Order == nil {
				return -1
			}
			return 1
		case a.Order != nil && *a.Order != *b.Order:
			return *a.Order - *b.Order
		case !a.LastRun.Equal(b.LastRun):
			return b.LastRun.Compare(a.LastRun)
		}
		return b.Created - a.Created
	})
	return out
}

// NextPinOrder is the order of a new pin: one past the largest, 0 when no pin
// has one.
func NextPinOrder(orders []*int) int {
	next := 0
	for _, o := range orders {
		if o != nil && *o >= next {
			next = *o + 1
		}
	}
	return next
}

// MovePin moves source before target, or after it, in ids, the pinned
// group's order. It reports false when nothing moves: an unknown id, or
// the source dropped on itself or where it already is.
func MovePin(ids []string, source, target string, after bool) ([]string, bool) {
	if source == target || !slices.Contains(ids, source) || !slices.Contains(ids, target) {
		return ids, false
	}
	out := slices.DeleteFunc(slices.Clone(ids), func(id string) bool { return id == source })
	at := slices.Index(out, target)
	if after {
		at++
	}
	out = slices.Insert(out, at, source)
	return out, !slices.Equal(out, ids)
}

// Sort is the order of the projects and of the conversations in the chats
// and in each project. Its values are the session_order preference's.
type Sort int

const (
	// SortCreated keeps the order the data comes in: the Node's, newest
	// created first, with a manual order another controller saved on it.
	// Projects come oldest first, so they are reversed.
	SortCreated Sort = iota
	// SortUpdated puts what never ran first, then the last run, newest first.
	SortUpdated
	// SortName compares names without case; a conversation without a title
	// comes first.
	SortName
)

// sortNames are the preference's values, by Sort.
var sortNames = []string{"created", "updated", "name"}

// Preference is the order's value in the session_order preference.
func (s Sort) Preference() string { return sortNames[min(max(s, SortCreated), SortName)] }

// SortOf is the order a session_order preference names; one that names
// none, as a profile that never chose, is the default: SortUpdated.
func SortOf(preference string) Sort {
	if i := slices.Index(sortNames, preference); i >= 0 {
		return Sort(i)
	}
	return SortUpdated
}

// Sorts are the orders as the sort menu lists them, with their copy.
var Sorts = []struct {
	Sort  Sort
	Label string // i18n key
}{{SortUpdated, "session_order_recent"}, {SortCreated, "session_order_created"}, {SortName, "session_order_name"}}

// Entry is a conversation or a project as Sorted orders it: its title or
// name, and its last run, zero when it never ran. A project's last run is
// the latest of its conversations'.
type Entry struct {
	ID      string
	Name    string
	Updated time.Time
}

// Sorted orders entries given in creation order, newest first. The sorts
// are stable, so equal entries keep that order. The pinned group is not
// sorted by it: pins keep their own order (OrderPins).
func Sorted(list []Entry, by Sort) []Entry {
	out := slices.Clone(list)
	switch by {
	case SortUpdated:
		slices.SortStableFunc(out, func(a, b Entry) int {
			// What never ran is new: it leads, as the reference's.
			switch {
			case a.Updated.IsZero() == b.Updated.IsZero():
				return b.Updated.Compare(a.Updated)
			case a.Updated.IsZero():
				return -1
			}
			return 1
		})
	case SortName:
		slices.SortStableFunc(out, func(a, b Entry) int {
			return strings.Compare(strings.ToLower(a.Name), strings.ToLower(b.Name))
		})
	}
	return out
}

// Adjacent returns the conversation after selected in ids, or before it,
// round at either end; with no selection, the first or the last. It returns
// "" for no ids.
func Adjacent(ids []string, selected string, forward bool) string {
	if len(ids) == 0 {
		return ""
	}
	i := slices.Index(ids, selected)
	switch {
	case i < 0 && forward:
		return ids[0]
	case i < 0:
		return ids[len(ids)-1]
	case forward:
		return ids[(i+1)%len(ids)]
	}
	return ids[(i-1+len(ids))%len(ids)]
}

// ValidTitle checks a conversation's new title: trimmed, not empty, without
// control characters.
func ValidTitle(s string) (string, bool) {
	s = strings.TrimSpace(s)
	if s == "" || strings.IndexFunc(s, unicode.IsControl) >= 0 {
		return "", false
	}
	return s, true
}
