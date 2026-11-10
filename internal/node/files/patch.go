// Codex patch parsing and exact matching derive from openai/codex
// 451787127c0e70247034c33e358536ffd2b89cf8 (Apache-2.0).
package files

import (
	"context"
	"fmt"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
	"slices"
	"strings"
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
			for cursor < len(lines)-1 && !strings.HasPrefix(lines[cursor], "*** ") {
				line := lines[cursor]
				if !strings.HasPrefix(line, "+") {
					return nil, invalid("invalid add hunk: every line must start with '+'")
				}
				entry.Contents += line[1:] + "\n"
				cursor++
			}
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
		paths := []string{entry.Path}
		if entry.Destination != nil {
			paths = append(paths, *entry.Destination)
		}
		for _, path := range paths {
			key := strings.ToLower(path)
			if seen[key] {
				return nil, invalid(path + " appears in more than one patch entry")
			}
			seen[key] = true
		}
		out = append(out, entry)
		if len(out) > protocol.MAX_CHANGES {
			return nil, invalid("a patch can change at most 16 files; a moved and edited file counts twice")
		}
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
func ApplyPatchChunks(before string, chunks []PatchChunk) (string, error) {
	lines, ending := patchLines(before)
	type replacement struct {
		start, count int
		new          []patchLine
	}
	type located struct {
		start, from int
		pattern     []string
		fixed       bool
	}
	var replacements []replacement
	var positions []located
	cursor := 0
	matches := func(pattern []string, start int) bool {
		if start < 0 || start+len(pattern) > len(lines) {
			return false
		}
		for i, text := range pattern {
			if lines[start+i].text != text {
				return false
			}
		}
		return true
	}
	seek := func(pattern []string, start int, eof bool) int {
		if eof {
			at := len(lines) - len(pattern)
			if at >= start && matches(pattern, at) {
				return at
			}
			return -1
		}
		for at := start; at+len(pattern) <= len(lines); at++ {
			if matches(pattern, at) {
				return at
			}
		}
		return -1
	}
	for index, chunk := range chunks {
		previous := cursor
		anchor := -1
		if chunk.Context != nil {
			anchor = seek([]string{*chunk.Context}, cursor, false)
			if anchor < 0 {
				return "", invalid(fmt.Sprintf("hunk %d: @@ context line was not found", index+1))
			}
			cursor = anchor + 1
		}
		old, new := chunk.Old, chunk.New
		if len(old) == 0 {
			at := len(lines)
			if anchor >= 0 && !chunk.EndOfFile {
				at = cursor
				positions = append(positions, located{anchor, previous, []string{*chunk.Context}, false})
			}
			var inserted []patchLine
			for _, text := range new {
				inserted = append(inserted, patchLine{text, ending})
			}
			replacements = append(replacements, replacement{at, 0, inserted})
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
		start := at
		if anchor >= 0 {
			start = anchor
		}
		positions = append(positions, located{start, cursor, old, chunk.EndOfFile})
		var inserted []patchLine
		kept := map[int]int{}
		for _, pair := range chunk.Kept {
			kept[pair[1]] = pair[0]
		}
		for i, text := range new {
			style := ending
			if original, ok := kept[i]; ok && original < len(old) && lines[at+original].ending != "" {
				style = lines[at+original].ending
			}
			inserted = append(inserted, patchLine{text, style})
		}
		replacements = append(replacements, replacement{at, len(old), inserted})
		cursor = at + len(old)
	}
	for index, p := range positions {
		if p.fixed {
			continue
		}
		bound := len(lines)
		if index+1 < len(positions) {
			bound = positions[index+1].start
		}
		count := 0
		for at := p.from; at+len(p.pattern) <= bound; at++ {
			if matches(p.pattern, at) {
				count++
			}
		}
		if count > 1 {
			return "", invalid("patch hunk matches multiple locations; add context lines or an @@ line")
		}
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
func ResolvePatch(ctx context.Context, root, input string, access core.FileAccess) ([]protocol.FileChange, error) {
	return ResolvePatchWithRevisions(ctx, root, input, nil, access)
}
func ResolvePatchWithRevisions(ctx context.Context, root, input string, observed map[string]string, access core.FileAccess) ([]protocol.FileChange, error) {
	hunks, e := ParsePatch(input)
	if e != nil {
		return nil, e
	}
	if len(hunks) == 0 {
		return nil, invalid("patch contains no file changes")
	}
	f, e := core.OpenFiles(root, access)
	if e != nil {
		return nil, e
	}
	defer f.Close()
	var changes []protocol.FileChange
	budget := 0
	for _, hunk := range hunks {
		if e := f.ValidatePath(hunk.Path, true); e != nil {
			return nil, e
		}
		if hunk.Destination != nil {
			if e := f.ValidatePath(*hunk.Destination, true); e != nil {
				return nil, e
			}
		}
		if hunk.Kind == "add" {
			hunk.Contents, e = PrepareCreation(hunk.Path, hunk.Contents)
			if e != nil {
				return nil, e
			}
			if len(hunk.Contents) > protocol.MAX_FILE_BYTES {
				return nil, invalid("added file exceeds the content limit")
			}
			changes = append(changes, protocol.FileChange{Kind: "create", Data: protocol.FileChangeCreatePayload{Path: hunk.Path, Text: hunk.Contents}})
			budget += len(hunk.Contents)
			if budget > protocol.MAX_CHANGE_BYTES || len(changes) > protocol.MAX_CHANGES {
				return nil, invalid("patch exceeds the combined text or file limit")
			}
			continue
		}
		expected, ok := observed[hunk.Path]
		if !ok {
			return nil, conflict("the current revision of this file has not been read; read the file again")
		}
		before, e := f.CompleteText(ctx, hunk.Path)
		if e != nil {
			return nil, e
		}
		if revision([]byte(before)) != expected {
			return nil, conflict("file changed since it was last read; read the file again")
		}
		budget += len(before)
		if budget > protocol.MAX_CHANGE_BYTES || len(changes) >= protocol.MAX_CHANGES {
			return nil, invalid("patch exceeds the combined text or file limit")
		}
		if hunk.Kind == "delete" {
			changes = append(changes, protocol.FileChange{Kind: "delete", Data: protocol.FileChangeDeletePayload{Path: hunk.Path, ExpectedRevision: expected}})
			continue
		}
		if len(hunk.Chunks) == 0 {
			changes = append(changes, protocol.FileChange{Kind: "move", Data: protocol.FileChangeMovePayload{Path: hunk.Path, Destination: *hunk.Destination, ExpectedRevision: expected}})
			continue
		}
		after, e := ApplyPatchChunks(before, hunk.Chunks)
		if e != nil {
			return nil, e
		}
		budget += len(after)
		if after == before {
			if hunk.Destination == nil {
				return nil, invalid("patch does not change this file")
			}
			changes = append(changes, protocol.FileChange{Kind: "move", Data: protocol.FileChangeMovePayload{Path: hunk.Path, Destination: *hunk.Destination, ExpectedRevision: expected}})
		} else if hunk.Destination != nil {
			changes = append(changes, protocol.FileChange{Kind: "create", Data: protocol.FileChangeCreatePayload{Path: *hunk.Destination, Text: after}}, protocol.FileChange{Kind: "delete", Data: protocol.FileChangeDeletePayload{Path: hunk.Path, ExpectedRevision: expected}})
		} else if before == "" {
			changes = append(changes, protocol.FileChange{Kind: "write", Data: protocol.FileChangeWritePayload{Path: hunk.Path, Text: after, ExpectedRevision: expected}})
		} else {
			changes = append(changes, protocol.FileChange{Kind: "edit", Data: protocol.FileChangeEditPayload{Path: hunk.Path, ExpectedRevision: expected, Edits: []protocol.FileEdit{{OldText: before, NewText: after}}}})
		}
		if budget > protocol.MAX_CHANGE_BYTES || len(changes) > protocol.MAX_CHANGES {
			return nil, invalid("patch exceeds the combined text or file limit")
		}
	}
	return changes, nil
}
