package output

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const fileBytes = 32 * 1024
const fileLineBytes = 2 * 1024

type fileRange struct{ first, last uint64 }
type fileReads map[string][]fileRange

func filePreview(original map[string]any, source map[string]any, reads *fileReads) (string, bool) {
	var page protocol.FileLines
	if json.Unmarshal([]byte(encoded(original["data"])), &page) != nil {
		return "", false
	}
	key := page.Path + "\x00" + page.Revision
	if reads != nil && !page.Truncated && len(page.Lines) > 0 {
		for _, previous := range (*reads)[key] {
			first, last := page.Lines[0].Number, page.Lines[len(page.Lines)-1].Number
			if previous.first <= first && last <= previous.last {
				return unchangedFile(page, first, last, source), true
			}
		}
	}
	body := ""
	bytes, lines := 0, 0
	shortened := false
	var shown *fileRange
	next := page.NextOffset
	for index, line := range page.Lines {
		if index >= MaxLines {
			break
		}
		if line.Number == 0 {
			return "", false
		}
		following := page.NextOffset
		if index+1 < len(page.Lines) {
			if page.Lines[index+1].Number == 0 {
				return "", false
			}
			n := page.Lines[index+1].Number - 1
			following = &n
		}
		row, omitted := fileRow(line, page.Truncated)
		rowBytes := len(encoded(row)) - 2
		rowLines := lineCount(row)
		rangeValue := &fileRange{first: page.Lines[0].Number, last: line.Number}
		metadata := renderFile(page, "", rangeValue, shortened || omitted, following, source)
		if bytes+rowBytes+len(encoded(metadata)) > fileBytes || lines+rowLines+lineCount(metadata) > MaxLines {
			break
		}
		body += row
		bytes += rowBytes
		lines += rowLines
		shortened = shortened || omitted
		shown = rangeValue
		next = following
	}
	if len(page.Lines) > 0 && shown == nil {
		return "", false
	}
	projected := renderFile(page, body, shown, shortened, next, source)
	if len(encoded(projected)) > fileBytes {
		return "", false
	}
	if reads != nil && !shortened && !page.Truncated && shown != nil {
		if *reads == nil {
			*reads = fileReads{}
		}
		(*reads)[key] = append((*reads)[key], *shown)
	}
	return projected, true
}
func fileRow(line protocol.FileLine, truncated bool) (string, bool) {
	content, ending := line.Text, " [no trailing newline]\n"
	if truncated {
		ending = " [source line truncated]\n"
	} else if strings.HasSuffix(content, "\r\n") {
		content = strings.TrimSuffix(content, "\r\n")
		ending = "\r\n"
	} else if strings.HasSuffix(content, "\n") {
		content = strings.TrimSuffix(content, "\n")
		ending = "\n"
	}
	end := len(content)
	if end > fileLineBytes {
		end = headBytes(content, fileLineBytes)
	}
	row := fmt.Sprintf("%d\t%s", line.Number, content[:end])
	if end < len(content) {
		row += fmt.Sprintf(" [... %d content bytes omitted from this line ...]", len(content)-end)
	}
	return row + ending, end < len(content)
}
func renderFile(page protocol.FileLines, body string, shown *fileRange, shortened bool, next *uint64, source map[string]any) string {
	value := ""
	if shown != nil {
		value = fmt.Sprintf("%s\nLines %d–%d; %d bytes; revision %s\n", page.Path, shown.first, shown.last, page.Size, page.Revision)
	} else {
		value = fmt.Sprintf("%s\nNo lines at offset=%d; %d bytes; revision %s\n", page.Path, page.Offset, page.Size, page.Revision)
	}
	value += body
	if page.Truncated {
		value += "The source line exceeds the read_file byte limit; its omitted suffix is not in the saved tool output\n"
		if shortened {
			value += "For the saved line prefix, use read_tool_output with output, offset=0 and limit\n"
		}
	} else if shortened {
		value += "For omitted line text, use read_tool_output with output, offset=0 and limit for the original result JSON\n"
	}
	if next != nil {
		value += fmt.Sprintf("Next file page: read_file with offset=%d\n", *next)
	}
	value += encoded(map[string]any{"output": source, "next_offset": next, "truncated": page.Truncated})
	return value
}
func unchangedFile(page protocol.FileLines, first, last uint64, source map[string]any) string {
	value := fmt.Sprintf("%s\nLines %d–%d are unchanged since an earlier read_file result in this conversation; revision %s\n", page.Path, first, last, page.Revision)
	if page.NextOffset != nil {
		value += fmt.Sprintf("Next file page: read_file with offset=%d\n", *page.NextOffset)
	}
	return value + encoded(map[string]any{"output": source, "next_offset": page.NextOffset})
}
func pagePreview(value any) (string, bool) {
	fields := object(value)
	if len(fields) != 6 || fields["kind"] != nil {
		return "", false
	}
	var page protocol.ToolOutputOutputPage
	if json.Unmarshal([]byte(encoded(value)), &page) != nil || page.Offset > ^uint64(0)-uint64(len(page.Text)) {
		return "", false
	}
	return fmt.Sprintf("Result bytes %d–%d of %d\n%s\n%s", page.Offset, page.Offset+uint64(len(page.Text)), page.TotalBytes, page.Text, encoded(map[string]any{"output": page.Output, "next_offset": page.NextOffset, "read": "read_tool_output with the same output and next_offset"})), true
}
func editPreview(original map[string]any) (string, bool) {
	var written protocol.FileWritten
	if json.Unmarshal([]byte(encoded(original["data"])), &written) != nil || written.Excerpt == nil {
		return "", false
	}
	return fmt.Sprintf("%s\nEdited; %d bytes; revision %s\n", written.Path, written.Size, written.Revision) + renderExcerpt(*written.Excerpt), true
}
func renderExcerpt(excerpt protocol.FileExcerpt) string {
	value := ""
	for _, hunk := range excerpt.Hunks {
		if len(hunk.Lines) == 0 {
			continue
		}
		value += fmt.Sprintf("Lines %d–%d\n", hunk.Lines[0].Number, hunk.Lines[len(hunk.Lines)-1].Number)
		for _, line := range hunk.Lines {
			content := line.Text
			if strings.HasSuffix(content, "\n") {
				content = strings.TrimSuffix(strings.TrimSuffix(content, "\n"), "\r")
			}
			value += fmt.Sprintf("%d\t%s\n", line.Number, content)
		}
		if hunk.Shortened {
			value += "[Some lines or text in this region are omitted]\n"
		}
	}
	if excerpt.Omitted > 0 {
		value += fmt.Sprintf("%d more edited regions omitted; read_file shows them\n", excerpt.Omitted)
	}
	return value
}
func changesPreview(original map[string]any) (string, bool) {
	var changes protocol.FileChanges
	if json.Unmarshal([]byte(encoded(original["data"])), &changes) != nil {
		return "", false
	}
	applied := false
	for _, file := range changes.Files {
		applied = applied || file.Status == protocol.ChangeStatusApplied
	}
	value := ""
	switch changes.Status {
	case protocol.ChangesStatusApplied:
		value = "All changes applied\n"
	case protocol.ChangesStatusRejected:
		value = "No changes applied; no file content was changed\n"
	case protocol.ChangesStatusPartial:
		if applied {
			value = "Changes partially applied; writing stopped at the first failure and earlier files were not rolled back\n"
		} else {
			value = "Outcome unknown; writing stopped at an uncertain entry\n"
		}
	}
	for _, file := range changes.Files {
		value += file.Path
		if file.Destination != nil {
			value += " -> " + *file.Destination
		}
		value += ": "
		switch file.Status {
		case protocol.ChangeStatusApplied:
			switch file.Kind {
			case protocol.ChangeKindAdded:
				value += "created"
			case protocol.ChangeKindModified:
				value += "modified"
			case protocol.ChangeKindRenamed:
				value += "moved"
			case protocol.ChangeKindDeleted:
				value += "moved to Trash"
			}
			if file.Size != nil {
				value += fmt.Sprintf("; %d bytes", *file.Size)
			}
			if file.Revision != nil {
				value += "; revision " + *file.Revision
			}
			value += "\n"
		case protocol.ChangeStatusFailed, protocol.ChangeStatusUnknown:
			label := "failed"
			if file.Status == protocol.ChangeStatusUnknown {
				label = "outcome unknown"
			}
			code, reason := "", ""
			if file.Error != nil {
				code = string(file.Error.Code)
				reason = file.Error.Message
			}
			value += fmt.Sprintf("%s (%s): %s\n", label, code, reason)
			if file.Status == protocol.ChangeStatusUnknown {
				value += "Inspect this file before retrying\n"
			}
		case protocol.ChangeStatusNotAttempted:
			value += "not attempted\n"
		}
		if file.Excerpt != nil {
			value += renderExcerpt(*file.Excerpt)
		}
	}
	return value, true
}
