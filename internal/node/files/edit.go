// Text edits derive from the reviewed confined file service (Apache-2.0).
package files

import (
	"github.com/sailry/sailry-harness/internal/protocol"
	"slices"
	"strings"
	"unicode/utf8"
)

type textEdit struct {
	start, end int
	text       string
}

func PrepareEdits(before string, edits []protocol.FileEdit) (string, error) {
	if len(edits) == 0 || !utf8.ValidString(before) {
		return "", invalid("invalid file edit content or count")
	}
	var matches []textEdit
	budget := 0
	normalized, positions := normalizedText(before)
	for _, edit := range edits {
		budget += len(edit.OldText) + len(edit.NewText)
		if !utf8.ValidString(edit.OldText) || !utf8.ValidString(edit.NewText) || edit.OldText == "" || !nonnull(edit.OldText) || !nonnull(edit.NewText) || budget > protocol.MAX_EDIT_BYTES {
			return "", invalid("invalid file edit content or count")
		}
		raw := strings.Contains(strings.ReplaceAll(edit.OldText, "\r\n", ""), "\r")
		haystack, needle := normalized, strings.ReplaceAll(edit.OldText, "\r\n", "\n")
		if raw {
			haystack, needle = before, edit.OldText
		}
		found := strings.Index(haystack, needle)
		if found < 0 {
			return "", invalid("old text was not found in the file")
		}
		if !edit.ReplaceAll {
			_, size := utf8.DecodeRuneInString(haystack[found:])
			if strings.Contains(haystack[found+size:], needle) {
				return "", invalid("old text must occur exactly once in the file unless replace_all is true")
			}
		}
		for found >= 0 {
			start, end := found, found+len(needle)
			if !raw {
				start, end = positions[start], positions[end]
			}
			replacement := edit.NewText
			if !raw {
				replacement = editReplacement(before, start, end, replacement)
			}
			matches = append(matches, textEdit{start, end, replacement})
			if !edit.ReplaceAll {
				break
			}
			next := found + len(needle)
			offset := strings.Index(haystack[next:], needle)
			if offset < 0 {
				break
			}
			found = next + offset
		}
	}
	slices.SortFunc(matches, func(a, b textEdit) int {
		if a.start < b.start {
			return -1
		}
		if a.start > b.start {
			return 1
		}
		return a.end - b.end
	})
	var after strings.Builder
	offset := 0
	for _, match := range matches {
		if match.start < offset {
			return "", invalid("file edit matches overlap")
		}
		if after.Len()+match.start-offset+len(match.text) > protocol.MAX_EDIT_BYTES {
			return "", invalid("edited file exceeds the text limit")
		}
		after.WriteString(before[offset:match.start])
		after.WriteString(match.text)
		offset = match.end
	}
	if after.Len()+len(before)-offset > protocol.MAX_EDIT_BYTES {
		return "", invalid("edited file exceeds the text limit")
	}
	after.WriteString(before[offset:])
	return after.String(), nil
}
func normalizedText(text string) (string, []int) {
	var out strings.Builder
	positions := make([]int, 0, len(text)+1)
	for i := 0; i < len(text); i++ {
		positions = append(positions, i)
		if text[i] == '\r' && i+1 < len(text) && text[i+1] == '\n' {
			i++
		}
		out.WriteByte(text[i])
	}
	positions = append(positions, len(text))
	return out.String(), positions
}
func editReplacement(before string, start, end int, text string) string {
	matched := before[start:end]
	var endings []string
	for offset := 0; offset < len(matched); offset++ {
		if matched[offset] == '\n' {
			ending := "\n"
			if offset > 0 && matched[offset-1] == '\r' {
				ending = "\r\n"
			}
			endings = append(endings, ending)
		}
	}
	nearby := "\n"
	if len(endings) > 0 {
		nearby = endings[len(endings)-1]
	} else {
		offset := strings.IndexByte(before[start:], '\n')
		if offset >= 0 {
			offset += start
		} else {
			offset = strings.LastIndexByte(before[:start], '\n')
		}
		if offset > 0 && before[offset-1] == '\r' {
			nearby = "\r\n"
		}
	}
	lines := strings.Split(strings.ReplaceAll(text, "\r\n", "\n"), "\n")
	var out strings.Builder
	for index, line := range lines {
		if index > 0 {
			ending := nearby
			if index-1 < len(endings) {
				ending = endings[index-1]
			}
			out.WriteString(ending)
		}
		out.WriteString(line)
	}
	return out.String()
}
