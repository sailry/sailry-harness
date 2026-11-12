package sidebar

import (
	"slices"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/picker"
)

// projectPlaces are the directories of the projects registered on host, for
// the sidebar of its directory picker: the most recently used first, by their
// conversations, and eight at most. The preview offers only those its sample
// tree holds.
func (s *State) projectPlaces(host string) []picker.Location {
	type place struct {
		picker.Location
		used time.Time
	}
	var places []place
	for _, pr := range s.Data.Projects {
		if pr.Host != host || pr.Path == "" {
			continue
		}
		path := pr.Path
		if catalog := s.Catalog(host); catalog != nil {
			// The sample projects write their directories under "~".
			path = catalog.Expand(path)
			if _, problem := catalog.List(path); problem != "" {
				continue
			}
		}
		p := place{Location: picker.Location{Name: pr.Name, Path: path}}
		for _, cv := range pr.Conversations {
			if cv.Updated.After(p.used) {
				p.used = cv.Updated
			}
		}
		places = append(places, p)
	}
	slices.SortStableFunc(places, func(a, b place) int { return b.used.Compare(a.used) })
	var out []picker.Location
	for _, p := range places[:min(len(places), 8)] {
		out = append(out, p.Location)
	}
	return out
}
