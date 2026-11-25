package viewmodel

import (
	"fmt"
	"regexp"
	"strconv"
	"strings"
)

var workspaceHunkHeader = regexp.MustCompile(`^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@`)

func ParseDiff(patch string) []Hunk {
	result := []Hunk{}
	old, new := 0, 0
	for _, line := range strings.Split(patch, "\n") {
		if match := workspaceHunkHeader.FindStringSubmatch(line); match != nil {
			old, _ = strconv.Atoi(match[1])
			new, _ = strconv.Atoi(match[2])
			result = append(result, Hunk{Header: line})
			continue
		}
		if len(result) == 0 || line == "" {
			continue
		}
		kind := Context
		entry := DiffLine{Text: line[1:]}
		switch line[0] {
		case '+':
			kind = Added
			entry.New = new
			new++
		case '-':
			kind = Removed
			entry.Old = old
			old++
		case ' ':
			entry.Old, entry.New = old, new
			old++
			new++
		default:
			continue
		}
		entry.Kind = kind
		index := len(result) - 1
		result[index].Lines = append(result[index].Lines, entry)
	}
	return result
}
func DiffHunkFingerprint(hunk Hunk) string {
	var result strings.Builder
	for _, line := range hunk.Lines {
		if line.Kind == Added || line.Kind == Removed {
			fmt.Fprintf(&result, "%d/%d/%d:%s\n", line.Kind, line.Old, line.New, line.Text)
		}
	}
	return result.String()
}
