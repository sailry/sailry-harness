package files

import (
	udiff "github.com/go-git/go-git/v5/utils/diff"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sergi/go-diff/diffmatchpatch"
	"unicode/utf8"
)

// ChangedExcerpt reports bounded numbered regions from the exact prepared text.
func ChangedExcerpt(before, after string) *protocol.FileExcerpt {
	out := &protocol.FileExcerpt{Hunks: []protocol.FileHunk{}}
	if after == "" {
		return out
	}
	starts := []int{0}
	for i := range after {
		if after[i] == '\n' && i+1 < len(after) {
			starts = append(starts, i+1)
		}
	}
	lineAt := func(position int) int {
		left, right := 0, len(starts)
		for left < right {
			middle := (left + right) / 2
			if starts[middle] <= position {
				left = middle + 1
			} else {
				right = middle
			}
		}
		return max(0, left-1)
	}
	type span struct{ first, last int }
	var spans []span
	position := 0
	for _, d := range udiff.Do(before, after) {
		if d.Type == diffmatchpatch.DiffEqual {
			position += len(d.Text)
			continue
		}
		end := position
		if d.Type == diffmatchpatch.DiffInsert {
			end += len(d.Text)
		}
		first, last := lineAt(min(position, len(after)-1)), lineAt(min(max(position, end-1), len(after)-1))
		current := span{max(0, first-3), min(len(starts)-1, last+3)}
		if len(spans) > 0 && current.first <= spans[len(spans)-1].last+1 {
			spans[len(spans)-1].last = max(spans[len(spans)-1].last, current.last)
		} else {
			spans = append(spans, current)
		}
		position = end
	}
	budget := 12 * 1024
	for index, s := range spans {
		if index == 16 || budget == 0 {
			out.Omitted = uint64(len(spans) - index)
			break
		}
		hunk := protocol.FileHunk{Lines: []protocol.FileLine{}, Shortened: s.last-s.first+1 > 60}
		for number := s.first; number <= min(s.last, s.first+59); number++ {
			end := len(after)
			if number+1 < len(starts) {
				end = starts[number+1]
			}
			full := after[starts[number]:end]
			limit := min(len(full), 1024, budget)
			for limit > 0 && !utf8.ValidString(full[:limit]) {
				limit--
			}
			if limit == 0 && full != "" {
				hunk.Shortened = true
				break
			}
			hunk.Shortened = hunk.Shortened || limit < len(full)
			budget -= limit
			hunk.Lines = append(hunk.Lines, protocol.FileLine{Number: uint64(number + 1), Text: full[:limit]})
		}
		if len(hunk.Lines) == 0 {
			out.Omitted = uint64(len(spans) - index)
			break
		}
		out.Hunks = append(out.Hunks, hunk)
	}
	return out
}
