package files

import (
	"context"
	"errors"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
	"io/fs"
	"os"
	"regexp"
	"strings"
	"unicode/utf8"
)

func Search(ctx context.Context, files *core.Files, input protocol.FileSearch) (protocol.SearchResults, error) {
	out := protocol.DefaultSearchResults()
	out.Mode, out.Offset = input.Mode, input.Offset
	if input.Query == "" || len(input.Query) > 1<<20 || input.Limit == 0 {
		return out, invalid("invalid file search pattern or options")
	}
	pattern := input.Query
	if !input.Regex {
		pattern = regexp.QuoteMeta(pattern)
	}
	prefix := "(?m)"
	if !input.CaseSensitive {
		prefix += "(?i)"
	}
	if input.Multiline {
		prefix += "(?s)"
	}
	matcher, e := regexp.Compile(prefix + pattern)
	if e != nil {
		return out, invalid("invalid file search pattern: " + e.Error())
	}
	globs, e := core.CompileFileGlobs(input.Globs)
	if e != nil {
		return out, e
	}
	var fileTypes *core.FileGlobs
	if input.FileType != nil {
		patterns, ok := defaultFileTypes[*input.FileType]
		if *input.FileType == "all" {
			for _, values := range defaultFileTypes {
				patterns = append(patterns, values...)
			}
			ok = true
		}
		if !ok {
			return out, invalid("unknown file type")
		}
		fileTypes, e = core.CompileFileGlobs(patterns)
		if e != nil {
			return out, e
		}
	}
	limit := min(input.Limit, uint64(protocol.MAX_SEARCH_MATCHES))
	position := uint64(0)
	report, e := files.VisitFiles(ctx, input.Path, globs, func(directory *os.Root, name, relative string, metadata fs.FileInfo) (bool, error) {
		if fileTypes != nil && !fileTypes.Match(relative, "") {
			return true, nil
		}
		out.ScannedFiles++
		if metadata.Size() > 64*1024*1024 {
			out.Skipped++
			diagnoseSearch(&out, relative, "file exceeds the 64 MiB search limit; use a narrower path or segmented reads")
			return true, nil
		}
		handle, e := files.Open(relative)
		if e != nil {
			out.Skipped++
			out.Partial = out.Partial || searchPartial(e)
			diagnoseSearch(&out, relative, e.Error())
			return true, nil
		}
		data, e := readBounded(ctx, handle, 64*1024*1024)
		handle.Close()
		if e != nil {
			out.Skipped++
			out.Partial = out.Partial || searchPartial(e)
			diagnoseSearch(&out, relative, e.Error())
			return true, nil
		}
		text, e := core.DecodeText(data)
		if e != nil || strings.ContainsRune(text, 0) {
			out.Skipped++
			return true, nil
		}
		type row struct {
			start, end int
			number     uint64
			matched    bool
		}
		var ranges [][]int
		if input.Multiline {
			const maximumRanges = (64 << 20) / 16
			ranges = matcher.FindAllStringIndex(text, maximumRanges+1)
			if len(ranges) > maximumRanges {
				out.Skipped++
				diagnoseSearch(&out, relative, "file search exceeds the memory limit")
				return true, nil
			}
		}
		contextCount := uint64(0)
		if input.Mode == protocol.SearchModeContent {
			contextCount = input.Context
		}
		// The ring retains only preceding context, never all matches or line offsets.
		capacity := min(contextCount, uint64(len(text)+1), uint64((64<<20)/32))
		initialPosition, initialRows := position, len(out.Matches)
		pending := make([]row, 0, int(capacity))
		head := 0
		emit := func(value row) {
			if position >= input.Offset && position-input.Offset < limit {
				line := strings.TrimSuffix(strings.TrimSuffix(text[value.start:value.end], "\n"), "\r")
				end := min(len(line), 16<<10)
				for !utf8.ValidString(line[:end]) {
					end--
				}
				out.Matches = append(out.Matches, protocol.SearchMatch{Path: relative, LineNumber: value.number, Line: line[:end], Matched: value.matched, Truncated: end < len(line)})
			}
			position++
		}
		matchIndex := 0
		lineNumber := uint64(0)
		afterContext := uint64(0)
		matches := uint64(0)
		for start := 0; start < len(text); {
			if err := ctx.Err(); err != nil {
				return false, err
			}
			end := len(text)
			if newline := strings.IndexByte(text[start:], '\n'); newline >= 0 {
				end = start + newline + 1
			}
			lineNumber++
			line := strings.TrimSuffix(strings.TrimSuffix(text[start:end], "\n"), "\r")
			matched := false
			if input.Multiline {
				for matchIndex < len(ranges) && ranges[matchIndex][1] < start {
					matchIndex++
				}
				for index := matchIndex; index < len(ranges) && ranges[index][0] < end; index++ {
					span := ranges[index]
					if span[1] > start || span[0] == span[1] && span[0] >= start {
						matched = true
						break
					}
				}
			} else {
				matched = matcher.MatchString(line)
			}
			current := row{start, end, lineNumber, matched}
			if matched {
				matches++
				if input.Mode == protocol.SearchModeContent {
					for index := 0; index < len(pending); index++ {
						emit(pending[(head+index)%len(pending)])
					}
					pending = pending[:0]
					head = 0
					emit(current)
					afterContext = contextCount
				}
			}
			if input.Mode == protocol.SearchModeContent && !matched {
				if afterContext > 0 {
					emit(current)
					afterContext--
				} else if capacity > 0 {
					if len(pending) < int(capacity) {
						pending = append(pending, current)
					} else {
						if capacity < contextCount && capacity == uint64((64<<20)/32) {
							position = initialPosition
							out.Matches = out.Matches[:initialRows]
							out.Skipped++
							diagnoseSearch(&out, relative, "file search exceeds the memory limit")
							return true, nil
						}
						pending[head] = current
						head = (head + 1) % len(pending)
					}
				}
			}
			start = end
		}
		if input.Mode != protocol.SearchModeContent && matches > 0 {
			if position >= input.Offset && position-input.Offset < limit {
				if input.Mode == protocol.SearchModeFiles {
					out.Paths = append(out.Paths, relative)
				} else {
					out.Counts = append(out.Counts, protocol.SearchCount{Path: relative, Count: matches})
				}
			}
			position++
		}
		return true, nil
	})
	if e != nil {
		return out, e
	}
	out.Skipped += report.Skipped
	out.Partial = out.Partial || report.Partial
	for _, d := range report.Diagnostics {
		diagnoseSearch(&out, d.Path, d.Message)
	}
	if out.Partial && input.Mode == protocol.SearchModeContent && input.Offset > 0 {
		out.Matches = nil
		diagnoseSearch(&out, input.Path, "an incomplete scan cannot resolve this offset; narrow path before retrying")
	}
	returned := uint64(len(out.Matches))
	if input.Mode == protocol.SearchModeFiles {
		returned = uint64(len(out.Paths))
	} else if input.Mode == protocol.SearchModeCount {
		returned = uint64(len(out.Counts))
	}
	end := input.Offset + returned
	if !out.Partial && end < position {
		out.NextOffset = &end
	}
	out.Truncated = out.Partial || end < position
	for _, row := range out.Matches {
		out.Truncated = out.Truncated || row.Truncated
	}
	return out, nil
}

func diagnoseSearch(out *protocol.SearchResults, path, message string) {
	if len(out.Diagnostics) < 100 {
		out.Diagnostics = append(out.Diagnostics, protocol.SearchDiagnostic{Path: path, Message: message})
	}
}

func searchPartial(err error) bool {
	if errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
		return true
	}
	value := fault(err)
	switch value.Code {
	case protocol.ErrorCodeNotFound, protocol.ErrorCodePermissionDenied, protocol.ErrorCodeCancelled, protocol.ErrorCodeBusy:
		return true
	}
	return false
}
