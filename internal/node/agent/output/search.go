package output

import (
	"encoding/json"
	"fmt"
	"unicode"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func searchPreview(original map[string]any, source map[string]any) (string, bool) {
	var page protocol.SearchResults
	if json.Unmarshal([]byte(encoded(original["data"])), &page) != nil {
		return "", false
	}
	total := searchTotal(page)
	body, path := "", ""
	shown := 0
	shortened := false
	for index := 0; index < total; index++ {
		candidate := body
		omitted := false
		switch page.Mode {
		case protocol.SearchModeContent:
			row := page.Matches[index]
			if path != row.Path {
				candidate += searchLabel(row.Path) + "\n"
			}
			line, cut := searchLine(row.Line)
			marker := "-"
			if row.Matched {
				marker = ":"
			}
			candidate += fmt.Sprintf("%d%s\t%s", row.LineNumber, marker, line)
			if cut {
				candidate += " [preview shortened]"
			}
			if row.Truncated {
				candidate += " [source line truncated]"
			}
			candidate += "\n"
			omitted = cut || row.Truncated
		case protocol.SearchModeFiles:
			candidate += searchLabel(page.Paths[index]) + "\n"
		case protocol.SearchModeCount:
			row := page.Counts[index]
			candidate += fmt.Sprintf("%d\t%s\n", row.Count, searchLabel(row.Path))
		}
		rendered := renderSearch(page, candidate, index+1, shortened || omitted, source, "")
		if len(encoded(rendered)) > 32*1024 || lineCount(rendered) > MaxLines {
			break
		}
		body = candidate
		shown = index + 1
		shortened = shortened || omitted
		if page.Mode == protocol.SearchModeContent {
			path = page.Matches[index].Path
		}
	}
	diagnostics := ""
	for _, diagnostic := range page.Diagnostics {
		message, cut := searchLine(diagnostic.Message)
		suffix := ""
		if cut {
			suffix = " [preview shortened]"
		}
		row := fmt.Sprintf("Diagnostic %s: %s%s\n", searchLabel(diagnostic.Path), message, suffix)
		candidate := diagnostics + row
		rendered := renderSearch(page, body, shown, shortened || cut, source, candidate)
		if len(encoded(rendered)) > 32*1024 || lineCount(rendered) > MaxLines {
			shortened = true
			break
		}
		diagnostics = candidate
		shortened = shortened || cut
	}
	rendered := renderSearch(page, body, shown, shortened, source, diagnostics)
	return rendered, len(encoded(rendered)) <= 32*1024
}
func searchTotal(page protocol.SearchResults) int {
	switch page.Mode {
	case protocol.SearchModeContent:
		return len(page.Matches)
	case protocol.SearchModeFiles:
		return len(page.Paths)
	case protocol.SearchModeCount:
		return len(page.Counts)
	}
	return 0
}
func searchLabel(path string) string {
	for _, r := range path {
		if unicode.IsControl(r) {
			return encoded(path)
		}
	}
	return path
}
func searchLine(value string) (string, bool) {
	count := 0
	for i := range value {
		if count == 500 {
			return value[:i], true
		}
		count++
	}
	return value, false
}
func renderSearch(page protocol.SearchResults, body string, shown int, shortened bool, source map[string]any, diagnostics string) string {
	total := searchTotal(page)
	var next *uint64
	if !page.Partial && shown > 0 {
		if shown < total {
			if page.Offset <= ^uint64(0)-uint64(shown) {
				n := page.Offset + uint64(shown)
				next = &n
			}
		} else {
			next = page.NextOffset
		}
	}
	value := fmt.Sprintf("Search %s; offset=%d; %d rows; scanned %d; skipped %d\n", page.Mode, page.Offset, shown, page.ScannedFiles, page.Skipped)
	if page.Mode == protocol.SearchModeContent {
		value += "':' marks a match; '-' marks context\n"
	}
	value += body + diagnostics
	if shortened || shown < total || len(page.Diagnostics) > 0 {
		value += "Use read_tool_output with output, offset=0 and limit for the original result JSON; read_file for source text beyond a truncated search line\n"
	}
	if page.Partial {
		value += "Incomplete scan: narrow path before retrying; no continuation\n"
	} else if next != nil {
		value += fmt.Sprintf("Next search page: repeat search_files with the same options and offset=%d, or narrow the query\n", *next)
	}
	return value + encoded(map[string]any{"output": source, "next_offset": next, "partial": page.Partial, "truncated": page.Truncated || shortened || shown < total})
}
