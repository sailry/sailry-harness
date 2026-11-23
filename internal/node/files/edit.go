// Text edits derive from the reviewed confined file service (Apache-2.0).
package files

import (
	"fmt"
	"slices"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type textEdit struct {
	start, end int
	text       string
}

func PrepareEdits(before string, edits []protocol.FileEdit) (string, error) {
	if len(edits) == 0 || !utf8.ValidString(before) {
		return "", invalid("invalid file edit content or count")
	}
	if len(edits) == 1 && edits[0].OldText != "" && edits[0].OldText == before && nonnull(before) &&
		utf8.ValidString(edits[0].NewText) && nonnull(edits[0].NewText) &&
		len(edits[0].OldText)+len(edits[0].NewText) <= protocol.MAX_EDIT_BYTES {
		// Fast path for whole-file replacement without scanning the entire text twice.
		return edits[0].NewText, nil
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
			if !raw && !edit.ReplaceAll {
				if matchStart, matchEnd, adjustedNew, ok, fuzzyErr := fuzzyLineMatch(haystack, needle, edit.NewText); fuzzyErr != nil {
					return "", fuzzyErr
				} else if ok {
					start, end := positions[matchStart], positions[matchEnd]
					replacement := editReplacement(before, start, end, adjustedNew)
					matches = append(matches, textEdit{start, end, replacement})
					continue
				}
			}
			return "", invalid("old text was not found in the file")
		}
		if !edit.ReplaceAll {
			_, size := utf8.DecodeRuneInString(haystack[found:])
			if strings.Contains(haystack[found+size:], needle) {
				count, lineNums := countMatches(haystack, needle, found)
				return "", invalid(fmt.Sprintf("old text must occur exactly once in the file unless replace_all is true (found %d matches at lines %s)", count, strings.Join(lineNums, ", ")))
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

type lineSpan struct {
	start, end int // byte offsets in LF-normalized text, excluding the '\n'
	hasNL      bool
	text       string
}

func splitLineSpans(text string) []lineSpan {
	var spans []lineSpan
	start := 0
	for i := 0; i < len(text); i++ {
		if text[i] == '\n' {
			spans = append(spans, lineSpan{start: start, end: i, hasNL: true, text: text[start:i]})
			start = i + 1
		}
	}
	if start < len(text) {
		spans = append(spans, lineSpan{start: start, end: len(text), hasNL: false, text: text[start:]})
	}
	return spans
}

func leadingIndent(line string) string {
	i := 0
	for i < len(line) && (line[i] == ' ' || line[i] == '\t') {
		i++
	}
	return line[:i]
}

// fuzzyLineMatch provides controlled single-match fallback for edit_file when
// exact matching fails due to trailing whitespace drift or a uniform
// indentation shift. Ambiguous matches are always rejected with line numbers.
func fuzzyLineMatch(haystack, needle, newText string) (start, end int, adjustedNew string, ok bool, err error) {
	hasTrailingNL := strings.HasSuffix(needle, "\n")
	rawNeedle := strings.TrimSuffix(needle, "\n")
	if rawNeedle == "" {
		return 0, 0, "", false, nil
	}
	fileLines := splitLineSpans(haystack)
	needleLines := strings.Split(rawNeedle, "\n")
	n := len(needleLines)
	if n == 0 || n > len(fileLines) {
		return 0, 0, "", false, nil
	}
	spanBounds := func(idx int) (int, int) {
		s := fileLines[idx].start
		e := fileLines[idx+n-1].end
		if hasTrailingNL && fileLines[idx+n-1].hasNL {
			e++
		}
		return s, e
	}
	formatLines := func(indices []int) string {
		limit := min(len(indices), 5)
		nums := make([]string, 0, limit+1)
		for i := range limit {
			nums = append(nums, strconv.Itoa(indices[i]+1))
		}
		if len(indices) > limit {
			nums = append(nums, "...")
		}
		return strings.Join(nums, ", ")
	}

	// Pass 1: trailing-whitespace-trimmed line match.
	trimmedNeedle := make([]string, n)
	for i, l := range needleLines {
		trimmedNeedle[i] = strings.TrimRight(l, " \t")
	}
	var pass1 []int
	for i := 0; i <= len(fileLines)-n; i++ {
		matched := true
		for j := range n {
			if strings.TrimRight(fileLines[i+j].text, " \t") != trimmedNeedle[j] {
				matched = false
				break
			}
		}
		if matched {
			pass1 = append(pass1, i)
		}
	}
	if len(pass1) == 1 {
		s, e := spanBounds(pass1[0])
		return s, e, newText, true, nil
	}
	if len(pass1) > 1 {
		return 0, 0, "", false, invalid(fmt.Sprintf("old text matched %d locations after ignoring trailing whitespace (lines %s); include more surrounding context", len(pass1), formatLines(pass1)))
	}

	// Pass 2: uniform indentation-shifted match (requires >=2 non-empty lines or >=20 trimmed chars).
	nonEmpty := 0
	trimmedLen := 0
	coreNeedle := make([]string, n)
	for i, l := range needleLines {
		c := strings.Trim(l, " \t")
		coreNeedle[i] = c
		if c != "" {
			nonEmpty++
			trimmedLen += len(c)
		}
	}
	if nonEmpty < 2 && trimmedLen < 20 {
		return 0, 0, "", false, nil
	}
	type indentHit struct {
		index  int
		add    string // prefix present in file but absent in needle
		remove string // prefix present in needle but absent in file
	}
	var pass2 []indentHit
	for i := 0; i <= len(fileLines)-n; i++ {
		matched := true
		deltaSet := false
		var addPrefix, removePrefix string
		for j := range n {
			fileLine := fileLines[i+j].text
			if strings.Trim(fileLine, " \t") != coreNeedle[j] {
				matched = false
				break
			}
			if coreNeedle[j] == "" {
				continue
			}
			fileIndent := leadingIndent(fileLine)
			needleIndent := leadingIndent(needleLines[j])
			var curAdd, curRemove string
			switch {
			case strings.HasSuffix(fileIndent, needleIndent):
				curAdd = fileIndent[:len(fileIndent)-len(needleIndent)]
			case strings.HasSuffix(needleIndent, fileIndent):
				curRemove = needleIndent[:len(needleIndent)-len(fileIndent)]
			default:
				matched = false
			}
			if !matched {
				break
			}
			if !deltaSet {
				addPrefix, removePrefix, deltaSet = curAdd, curRemove, true
			} else if curAdd != addPrefix || curRemove != removePrefix {
				matched = false
				break
			}
		}
		if matched && deltaSet {
			pass2 = append(pass2, indentHit{index: i, add: addPrefix, remove: removePrefix})
		}
	}
	if len(pass2) == 1 {
		hit := pass2[0]
		s, e := spanBounds(hit.index)
		return s, e, shiftIndent(newText, hit.add, hit.remove), true, nil
	}
	if len(pass2) > 1 {
		indices := make([]int, len(pass2))
		for i, hit := range pass2 {
			indices[i] = hit.index
		}
		return 0, 0, "", false, invalid(fmt.Sprintf("old text matched %d locations with different indentation (lines %s); include more surrounding context", len(pass2), formatLines(indices)))
	}
	return 0, 0, "", false, nil
}

func shiftIndent(text, addPrefix, removePrefix string) string {
	if addPrefix == "" && removePrefix == "" {
		return text
	}
	normalized := strings.ReplaceAll(text, "\r\n", "\n")
	lines := strings.Split(normalized, "\n")
	for i, line := range lines {
		if strings.TrimSpace(line) == "" {
			continue
		}
		if addPrefix != "" {
			lines[i] = addPrefix + line
		} else if removePrefix != "" && strings.HasPrefix(line, removePrefix) {
			lines[i] = line[len(removePrefix):]
		}
	}
	return strings.Join(lines, "\n")
}

func countMatches(haystack, needle string, first int) (int, []string) {
	count := 0
	var lines []string
	line := 1 + strings.Count(haystack[:first], "\n")
	prev := first
	for pos := first; pos >= 0; {
		line += strings.Count(haystack[prev:pos], "\n")
		prev = pos
		count++
		if len(lines) < 5 {
			lines = append(lines, strconv.Itoa(line))
		}
		_, size := utf8.DecodeRuneInString(haystack[pos:])
		next := pos + size
		if next >= len(haystack) {
			break
		}
		rel := strings.Index(haystack[next:], needle)
		if rel < 0 {
			break
		}
		pos = next + rel
	}
	if count > len(lines) {
		lines = append(lines, "...")
	}
	return count, lines
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
