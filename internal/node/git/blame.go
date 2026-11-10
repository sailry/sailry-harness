package git

import (
	"context"
	"strconv"
	"strings"
	"unicode/utf8"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	fdiff "github.com/go-git/go-git/v5/plumbing/format/diff"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func fmtLine(line uint64) string { return strconv.FormatUint(line, 10) + ": " }
func (s *Service) Blame(ctx context.Context, root string, input protocol.CommandReadGitBlamePayload) (protocol.GitBlame, error) {
	result := protocol.GitBlame{Path: input.Path, Offset: input.Offset, Entries: []protocol.GitBlameLine{}}
	if input.Limit == 0 {
		return result, invalid("Git line page limit must be positive")
	}
	r, err := open(root)
	if err != nil {
		return result, err
	}
	revision := "HEAD"
	if input.Revision != nil {
		revision = *input.Revision
	}
	hash, err := resolve(r, revision)
	if err != nil {
		return result, err
	}
	result.Commit = hash.String()
	value, err := committedFile(r, hash, input.Path)
	if err != nil {
		return result, err
	}
	if !utf8.Valid(value.data) || strings.IndexByte(string(value.data), 0) >= 0 {
		return result, invalid("Git blame requires a UTF-8 text file")
	}
	commit, err := r.CommitObject(hash)
	if err != nil {
		return result, mapError(err)
	}
	blame, err := gogit.Blame(commit, input.Path)
	if err != nil {
		return result, mapError(err)
	}
	result.TotalLines = uint64(len(blame.Lines))
	if input.Offset > result.TotalLines {
		return result, invalid("Git line offset is out of range")
	}
	originals := map[plumbing.Hash]map[uint64]uint64{}
	bytes := 0
	for i := input.Offset; i < min(result.TotalLines, input.Offset+min(input.Limit, 500)); i++ {
		if err := checkContext(ctx); err != nil {
			return result, err
		}
		line := blame.Lines[i]
		budget := maxDiff - bytes
		if len(line.Text) > budget && len(result.Entries) > 0 {
			break
		}
		text, partial := bounded(line.Text, budget)
		author, a := bounded(line.AuthorName, 256)
		email, e := bounded(line.Author, 256)
		mapping := originals[line.Hash]
		if mapping == nil {
			mapping = map[uint64]uint64{}
			original, err := committedFile(r, line.Hash, input.Path)
			if err == nil {
				var old, new uint64
				for _, chunk := range chunks(original.data, value.data) {
					n := uint64(strings.Count(chunk.Content(), "\n"))
					if chunk.Content() != "" && !strings.HasSuffix(chunk.Content(), "\n") {
						n++
					}
					switch chunk.Type() {
					case fdiff.Equal:
						for j := uint64(0); j < n; j++ {
							mapping[new+j] = old + j
						}
						old += n
						new += n
					case fdiff.Delete:
						old += n
					case fdiff.Add:
						new += n
					}
				}
			}
			originals[line.Hash] = mapping
		}
		position, ok := mapping[i]
		if !ok {
			return result, fault(protocol.ErrorCodeUnavailable, "Git blame original line mapping is unavailable")
		}
		result.Entries = append(result.Entries, protocol.GitBlameLine{Line: i + 1, OriginalLine: position + 1, Commit: line.Hash.String(), Author: author, Email: email, Timestamp: line.Date.Unix(), Text: text, Truncated: partial || a || e})
		bytes += len(text)
		result.Truncated = result.Truncated || partial || a || e
		if partial || bytes == maxDiff {
			break
		}
	}
	if input.Offset+uint64(len(result.Entries)) < result.TotalLines {
		result.Next = ptr(input.Offset + uint64(len(result.Entries)))
		result.Truncated = true
	}
	return result, nil
}
