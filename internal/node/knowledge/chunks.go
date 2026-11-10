package knowledge

import (
	"math"
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Tokens is the largest chunk in model tokens; consecutive chunks overlap.
const (
	Tokens  = 256
	overlap = 38
)

func invalid(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}

// Chunk is one source-aligned token window with its Markdown heading.
type Chunk struct {
	Start, End          int
	FirstLine, LastLine uint32
	Heading             *string
}

type heading struct {
	offset int
	text   string
}

// Split windows text along its token byte ranges, preferring a paragraph
// boundary that keeps at least half a window.
func Split(text string, offsets [][2]int) ([]Chunk, error) {
	previous := [2]int{}
	for _, offset := range offsets {
		start, end := offset[0], offset[1]
		if start < previous[0] || end < previous[1] || start >= end || end > len(text) || !boundary(text, start) || !boundary(text, end) {
			return nil, invalid("invalid knowledge token offsets")
		}
		previous = offset
	}
	if len(offsets) == 0 {
		return nil, nil
	}
	var lines []int
	for index := 0; index < len(text); index++ {
		if text[index] == '\n' {
			lines = append(lines, index)
		}
	}
	headings := headings(text)
	var chunks []Chunk
	token := 0
	for {
		next := min(token+Tokens, len(offsets))
		if next < len(offsets) {
			first, end := offsets[token][0], offsets[next-1][1]
			if position := strings.LastIndex(text[first:end], "\n\n"); position >= 0 {
				limit := first + position + 2
				window := offsets[token:next]
				count := sort.Search(len(window), func(index int) bool { return window[index][1] > limit })
				if count >= Tokens/2 {
					next = token + count
				}
			}
		}
		start, end := offsets[token][0], offsets[next-1][1]
		first := sort.SearchInts(lines, start) + 1
		last := sort.SearchInts(lines, end-1) + 1
		if first > math.MaxUint32 || last > math.MaxUint32 {
			return nil, invalid("knowledge source has too many lines")
		}
		chunk := Chunk{Start: start, End: end, FirstLine: uint32(first), LastLine: uint32(last)}
		for index := len(headings) - 1; index >= 0; index-- {
			if headings[index].offset <= start {
				chunk.Heading = &headings[index].text
				break
			}
		}
		chunks = append(chunks, chunk)
		if next == len(offsets) {
			return chunks, nil
		}
		token = max(max(next-overlap, 0), token+1)
	}
}

func boundary(text string, offset int) bool {
	return offset == len(text) || utf8.RuneStart(text[offset])
}

func headings(text string) []heading {
	var found []heading
	offset, fenced := 0, false
	for _, line := range strings.SplitAfter(text, "\n") {
		content := strings.TrimSpace(line)
		if strings.HasPrefix(content, "```") || strings.HasPrefix(content, "~~~") {
			fenced = !fenced
		} else if !fenced {
			level := len(content) - len(strings.TrimLeft(content, "#"))
			if level >= 1 && level <= 6 && len(content) > level && content[level] == ' ' {
				title := []rune(strings.TrimSpace(content[level+1:]))
				found = append(found, heading{offset, string(title[:min(len(title), 512)])})
			}
		}
		offset += len(line)
	}
	return found
}
