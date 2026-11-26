// Codex patch parsing and matching derive from openai/codex
// 451787127c0e70247034c33e358536ffd2b89cf8 and af71942309704997f4bdc86c020717646423c080 (Apache-2.0).
package files

import (
	"context"
	"fmt"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
	"slices"
	"strings"
	"unicode"
)

type PatchHunk struct {
	Kind, Path, Contents string
	Destination          *string
	Chunks               []PatchChunk
}
type PatchChunk struct {
	Context   *string
	Old, New  []string
	Kept      [][2]int
	EndOfFile bool
}

func ParsePatch(input string) ([]PatchHunk, error) {
	if len(input) > protocol.MAX_CHANGE_BYTES {
		return nil, invalid("patch exceeds the input limit")
	}
	lines := strings.Split(strings.TrimSpace(input), "\n")
	for i := range lines {
		lines[i] = strings.TrimSuffix(lines[i], "\r")
	}
	if len(lines) >= 4 && (lines[0] == "<<EOF" || lines[0] == "<<'EOF'" || lines[0] == `<<"EOF"`) && strings.HasSuffix(lines[len(lines)-1], "EOF") {
		lines = lines[1 : len(lines)-1]
	}
	if len(lines) < 2 || strings.TrimSpace(lines[0]) != "*** Begin Patch" {
		return nil, invalid("invalid patch: The first line of the patch must be '*** Begin Patch'")
	}
	if strings.TrimSpace(lines[len(lines)-1]) != "*** End Patch" {
		return nil, invalid("invalid patch: The last line of the patch must be '*** End Patch'")
	}
	var out []PatchHunk
	seen := map[string]bool{}
	for cursor := 1; cursor < len(lines)-1; {
		header := lines[cursor]
		cursor++
		entry := PatchHunk{}
		switch {
		case strings.HasPrefix(header, "*** Add File: "):
			entry.Kind, entry.Path = "add", strings.TrimPrefix(header, "*** Add File: ")
		case strings.HasPrefix(header, "*** Delete File: "):
			entry.Kind, entry.Path = "delete", strings.TrimPrefix(header, "*** Delete File: ")
		case strings.HasPrefix(header, "*** Update File: "):
			entry.Kind, entry.Path = "update", strings.TrimPrefix(header, "*** Update File: ")
		default:
			return nil, invalid(fmt.Sprintf("invalid hunk at line %d, invalid hunk header: %s", cursor, header))
		}
		if entry.Path == "" {
			return nil, invalid("invalid patch path")
		}
		if entry.Kind == "add" {
			var contents strings.Builder
			for cursor < len(lines)-1 && !strings.HasPrefix(lines[cursor], "*** ") {
				line := lines[cursor]
				if !strings.HasPrefix(line, "+") {
					return nil, invalid("invalid add hunk: every line must start with '+'")
				}
				contents.WriteString(line[1:])
				contents.WriteByte('\n')
				cursor++
			}
			entry.Contents = contents.String()
		} else if entry.Kind == "update" {
			if cursor < len(lines)-1 && strings.HasPrefix(lines[cursor], "*** Move to: ") {
				destination := strings.TrimPrefix(lines[cursor], "*** Move to: ")
				if destination == "" {
					return nil, invalid("invalid patch move destination")
				}
				entry.Destination = &destination
				cursor++
			}
			var chunk *PatchChunk
			for cursor < len(lines)-1 {
				line := lines[cursor]
				if strings.HasPrefix(line, "*** ") && line != "*** End of File" {
					break
				}
				cursor++
				if line == "@@" || strings.HasPrefix(line, "@@ ") {
					if chunk != nil && len(chunk.Old)+len(chunk.New) == 0 {
						return nil, invalid("update hunk does not contain any lines")
					}
					entry.Chunks = append(entry.Chunks, PatchChunk{})
					chunk = &entry.Chunks[len(entry.Chunks)-1]
					if line != "@@" {
						anchor := line[3:]
						chunk.Context = &anchor
					}
					continue
				}
				if line == "*** End of File" {
					if chunk == nil {
						return nil, invalid("end of file marker requires an update chunk")
					}
					chunk.EndOfFile = true
					if cursor < len(lines)-1 && !strings.HasPrefix(lines[cursor], "*** ") {
						return nil, invalid("unexpected line after end of file marker")
					}
					break
				}
				if chunk == nil {
					entry.Chunks = append(entry.Chunks, PatchChunk{})
					chunk = &entry.Chunks[0]
				}
				if line == "" {
					line = " "
				}
				switch line[0] {
				case ' ':
					chunk.Kept = append(chunk.Kept, [2]int{len(chunk.Old), len(chunk.New)})
					chunk.Old = append(chunk.Old, line[1:])
					chunk.New = append(chunk.New, line[1:])
				case '-':
					chunk.Old = append(chunk.Old, line[1:])
				case '+':
					chunk.New = append(chunk.New, line[1:])
				default:
					return nil, invalid(fmt.Sprintf("invalid update hunk line: %s", line))
				}
			}
			if len(entry.Chunks) == 0 && entry.Destination == nil {
				return nil, invalid("update file hunk is empty")
			}
			if len(entry.Chunks) > 0 {
				last := entry.Chunks[len(entry.Chunks)-1]
				if len(last.Old)+len(last.New) == 0 {
					return nil, invalid("update hunk does not contain any lines")
				}
			}
		}
		if seen[entry.Path] {
			return nil, invalid(entry.Path + " appears in more than one patch entry")
		}
		seen[entry.Path] = true
		out = append(out, entry)
	}
	return out, nil
}

type patchLine struct{ text, ending string }

func patchLines(text string) ([]patchLine, string) {
	var lines []patchLine
	ending := ""
	start := 0
	for offset := 0; offset < len(text); offset++ {
		if text[offset] != '\n' && text[offset] != '\r' {
			continue
		}
		end := offset
		style := text[offset : offset+1]
		if text[offset] == '\r' && offset+1 < len(text) && text[offset+1] == '\n' {
			offset++
			style = "\r\n"
		}
		if ending == "" {
			ending = style
		}
		lines = append(lines, patchLine{text[start:end], style})
		start = offset + 1
	}
	if start < len(text) {
		lines = append(lines, patchLine{text[start:], ""})
	}
	if ending == "" {
		ending = "\n"
	}
	return lines, ending
}
func normalizePatchLine(line string) string {
	return strings.Map(func(r rune) rune {
		switch {
		case r >= '\u2010' && r <= '\u2015', r == '\u2212':
			return '-'
		case r >= '\u2018' && r <= '\u201b':
			return '\''
		case r >= '\u201c' && r <= '\u201f':
			return '"'
		case r == '\u00a0', r >= '\u2002' && r <= '\u200a', r == '\u202f', r == '\u205f', r == '\u3000':
			return ' '
		default:
			return r
		}
	}, strings.TrimSpace(line))
}

func ApplyPatchChunks(before string, chunks []PatchChunk) (string, error) {
	lines, ending := patchLines(before)
	type replacement struct {
		start, count int
		new          []patchLine
	}
	var replacements []replacement
	cursor := 0
	matchLine := func(a, b string, mode uint8) bool {
		switch mode {
		case 1:
			return strings.TrimRightFunc(a, unicode.IsSpace) == strings.TrimRightFunc(b, unicode.IsSpace)
		case 2:
			return strings.TrimSpace(a) == strings.TrimSpace(b)
		case 3:
			return normalizePatchLine(a) == normalizePatchLine(b)
		default:
			return a == b
		}
	}
	matchesMode := func(pattern []string, start int, mode uint8) bool {
		if start < 0 || start+len(pattern) > len(lines) {
			return false
		}
		for i, text := range pattern {
			if !matchLine(lines[start+i].text, text, mode) {
				return false
			}
		}
		return true
	}
	seek := func(pattern []string, start int, eof bool) int {
		if len(pattern) == 0 {
			return start
		}
		if eof {
			start = max(start, len(lines)-len(pattern))
		}
		for mode := uint8(0); mode <= 3; mode++ {
			for at := start; at+len(pattern) <= len(lines); at++ {
				if matchesMode(pattern, at, mode) {
					return at
				}
			}
		}
		return -1
	}
	for index, chunk := range chunks {
		if chunk.Context != nil {
			anchor := seek([]string{*chunk.Context}, cursor, false)
			if anchor < 0 {
				return "", invalid(fmt.Sprintf("hunk %d: @@ context line was not found", index+1))
			}
			cursor = anchor + 1
		}
		old, new := chunk.Old, chunk.New
		if len(old) == 0 {
			var inserted []patchLine
			for _, text := range new {
				inserted = append(inserted, patchLine{text, ending})
			}
			replacements = append(replacements, replacement{len(lines), 0, inserted})
			continue
		}
		at := seek(old, cursor, chunk.EndOfFile)
		if at < 0 && len(old) > 0 && old[len(old)-1] == "" {
			old = old[:len(old)-1]
			if len(new) > 0 && new[len(new)-1] == "" {
				new = new[:len(new)-1]
			}
			at = seek(old, cursor, chunk.EndOfFile)
		}
		if at < 0 {
			return "", invalid(fmt.Sprintf("hunk %d: expected lines were not found", index+1))
		}
		var inserted []patchLine
		kept := map[int]int{}
		for _, pair := range chunk.Kept {
			kept[pair[1]] = pair[0]
		}
		for i, text := range new {
			style := ending
			if original, ok := kept[i]; ok && original < len(old) {
				text = lines[at+original].text
				if lines[at+original].ending != "" {
					style = lines[at+original].ending
				}
			}
			inserted = append(inserted, patchLine{text, style})
		}
		replacements = append(replacements, replacement{at, len(old), inserted})
		cursor = at + len(old)
	}
	slices.SortStableFunc(replacements, func(a, b replacement) int { return a.start - b.start })
	var out strings.Builder
	position := 0
	appendLine := func(line patchLine) {
		out.WriteString(line.text)
		if line.ending == "" {
			out.WriteString(ending)
		} else {
			out.WriteString(line.ending)
		}
	}
	for _, change := range replacements {
		if change.start < position {
			return "", invalid("patch hunks overlap")
		}
		for _, line := range lines[position:change.start] {
			appendLine(line)
		}
		for _, line := range change.new {
			appendLine(line)
		}
		position = change.start + change.count
	}
	for _, line := range lines[position:] {
		appendLine(line)
	}
	if out.Len() > protocol.MAX_EDIT_BYTES {
		return "", invalid("patched file exceeds the text limit")
	}
	return out.String(), nil
}

// PreparePatch captures each file as it is matched so the approval preview
// and the revision checked at execution describe the same contents.
func PreparePatch(ctx context.Context, root, input string, worktree protocol.WorktreeID, access core.FileAccess) (*Prepared, error) {
	hunks, err := ParsePatch(input)
	if err != nil {
		return nil, err
	}
	if len(hunks) == 0 {
		return nil, invalid("patch contains no file changes")
	}
	f, err := core.OpenFiles(root, access)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	command := &protocol.CommandApplyChangesPayload{Worktree: worktree}
	p := &Prepared{Command: protocol.Command{Kind: "apply_changes", Data: command}, Access: access}
	// A later hunk can refer to a path written by an earlier move.
	contents := map[string]*string{}
	read := func(path string) (*string, error) {
		if text, ok := contents[path]; ok {
			return text, nil
		}
		if err := f.ValidatePath(path, true); err != nil {
			return nil, err
		}
		text, err := f.CompleteText(ctx, path)
		if err != nil {
			if fault(err).Code == protocol.ErrorCodeNotFound {
				return nil, nil
			}
			return nil, err
		}
		return &text, nil
	}
	budget := 0
	appendChange := func(entry PreparedChange, change protocol.FileChange) error {
		if entry.Before != nil {
			budget += len(*entry.Before)
		}
		if entry.After != nil {
			budget += len(*entry.After)
			if !nonnull(*entry.After) || len(*entry.After) > protocol.MAX_EDIT_BYTES || entry.Before == nil && len(*entry.After) > protocol.MAX_WRITE_BYTES {
				return invalid(entry.Path + ": file text exceeds the content limit or contains a NUL byte")
			}
		}
		if budget > protocol.MAX_CHANGE_BYTES {
			return invalid("patch exceeds the combined text limit")
		}
		contents[entry.Path] = entry.After
		if entry.Destination != nil {
			contents[*entry.Destination] = entry.Before
		}
		command.Changes = append(command.Changes, change)
		p.Changes = append(p.Changes, entry)
		p.Preview.Files = append(p.Preview.Files, preview(entry))
		return nil
	}
	record := func(path string, before, after *string) error {
		entry := PreparedChange{Path: path, Before: before, After: after}
		var change protocol.FileChange
		if before != nil {
			expected := revision([]byte(*before))
			entry.ExpectedRevision = &expected
		}
		switch {
		case after == nil:
			entry.Operation, entry.Kind = "delete", protocol.ChangeKindDeleted
			change = protocol.FileChange{Kind: "delete", Data: protocol.FileChangeDeletePayload{Path: path, ExpectedRevision: *entry.ExpectedRevision}}
		case before == nil:
			entry.Operation, entry.Kind = "create", protocol.ChangeKindAdded
			change = protocol.FileChange{Kind: "create", Data: protocol.FileChangeCreatePayload{Path: path, Text: *after}}
		default:
			entry.Operation, entry.Kind = "write", protocol.ChangeKindModified
			change = protocol.FileChange{Kind: "write", Data: protocol.FileChangeWritePayload{Path: path, Text: *after, ExpectedRevision: *entry.ExpectedRevision}}
		}
		return appendChange(entry, change)
	}
	for _, hunk := range hunks {
		before, err := read(hunk.Path)
		if err != nil {
			return nil, err
		}
		if hunk.Kind == "add" {
			if err := record(hunk.Path, before, &hunk.Contents); err != nil {
				return nil, err
			}
			continue
		}
		if before == nil {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: hunk.Path + ": file does not exist"}
		}
		if hunk.Kind == "delete" {
			if err := record(hunk.Path, before, nil); err != nil {
				return nil, err
			}
			continue
		}
		after := *before
		if len(hunk.Chunks) > 0 {
			after, err = ApplyPatchChunks(*before, hunk.Chunks)
			if err != nil {
				return nil, invalid(hunk.Path + ": " + fault(err).Message)
			}
		}
		if hunk.Destination != nil && *hunk.Destination != hunk.Path {
			dst := *hunk.Destination
			current, err := read(dst)
			if err != nil {
				return nil, err
			}
			sameEntry := false
			if current != nil {
				sameEntry, err = f.SameFileEntry(hunk.Path, dst)
				if err != nil {
					return nil, err
				}
			}
			if sameEntry || current == nil && after == *before {
				// Rename an alias before writing through its new spelling.
				expected := revision([]byte(*before))
				entry := PreparedChange{Operation: "move", Kind: protocol.ChangeKindRenamed, Path: hunk.Path, Destination: &dst, Before: before, ExpectedRevision: &expected, CreateParents: current == nil}
				change := protocol.FileChange{Kind: "move", Data: protocol.FileChangeMovePayload{Path: hunk.Path, Destination: dst, ExpectedRevision: expected}}
				if err := appendChange(entry, change); err != nil {
					return nil, err
				}
				if after != *before {
					if err := record(dst, before, &after); err != nil {
						return nil, err
					}
				}
				continue
			}
			if err := record(dst, current, &after); err != nil {
				return nil, err
			}
			if err := record(hunk.Path, before, nil); err != nil {
				return nil, err
			}
		} else if err := record(hunk.Path, before, &after); err != nil {
			return nil, err
		}
	}
	return p, nil
}
