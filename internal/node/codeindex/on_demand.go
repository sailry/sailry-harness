package codeindex

import (
	"context"
	"errors"
	"io/fs"
	"os"
	"regexp"
	"slices"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	queryFiles = 64
	queryRows  = 25_000
	queryBytes = 32 << 20
	queryTime  = 8 * time.Second
	queryHints = 32
)

// liveFiles owns a bounded, query-local source selection. It does not open a
// worktree database or start a watch.
type liveFiles struct {
	ctx      context.Context
	service  *Service
	files    *core.Files
	parse    func(context.Context, string, string) (Parsed, error)
	parsed   []Parsed
	seen     map[string]bool
	report   report
	bytes    int64
	rows     int
	attempts int
}

func (s *Service) liveFiles(ctx context.Context, id protocol.WorktreeID, root string, references bool) (context.Context, *liveFiles, func(), error) {
	ctx, _, end, err := s.begin(ctx, id, root, false)
	if err != nil {
		return nil, nil, nil, err
	}
	files, err := core.OpenFiles(root, core.RestrictedFiles)
	if err != nil {
		end()
		return nil, nil, nil, err
	}
	bounded, cancel := context.WithTimeout(ctx, queryTime)
	release, err := s.acquireReader(bounded)
	if err != nil {
		cancel()
		_ = files.Close()
		end()
		return nil, nil, nil, err
	}
	close := func() { cancel(); _ = files.Close(); end(); release() }
	parse := s.parser.ParseOutline
	if references {
		parse = s.parser.ParseIndexed
	}
	return ctx, &liveFiles{ctx: bounded, service: s, files: files, parse: parse, seen: map[string]bool{}, report: report{problems: map[string]problem{}}}, close, nil
}

func (scan *liveFiles) read(path string, match *regexp.Regexp) (*Parsed, error) {
	var parsed *Parsed
	_, err := scan.service.inspectText(scan.files, scan.ctx, path, MaxFileBytes, core.FileStamp{}, func(source *string) error {
		if source == nil {
			file := Omitted(path, StatusTooLarge)
			parsed = &file
			return nil
		}
		if match != nil && !matchesSource(match, path) && !matchesSource(match, *source) {
			return nil
		}
		scan.attempts++
		file, err := scan.parse(scan.ctx, path, *source)
		parsed = &file
		return err
	})
	return parsed, err
}

func matchesSource(match *regexp.Regexp, text string) bool {
	return match.MatchString(text) || strings.IndexFunc(text, func(r rune) bool { return r >= utf8.RuneSelf }) >= 0 && match.MatchString(knowledge.Field(text))
}

func expression(values []string, identifiers bool) *regexp.Regexp {
	parts := make([]string, 0, len(values))
	for _, value := range values {
		if value = strings.TrimSpace(strings.ToValidUTF8(value, "\uFFFD")); value != "" {
			parts = append(parts, regexp.QuoteMeta(value))
		}
	}
	if len(parts) == 0 {
		return nil
	}
	names := "(?:" + strings.Join(parts, "|") + ")"
	if identifiers {
		const separator = `[^\p{L}\p{N}\p{M}_$]`
		return regexp.MustCompile("(?:^|" + separator + ")" + names + "(?:$|" + separator + ")")
	}
	return regexp.MustCompile("(?i)" + names)
}

func (scan *liveFiles) cut() {
	scan.report.partial, scan.report.truncated = true, true
}

func (scan *liveFiles) walk(scope string, match *regexp.Regexp) error {
	walked, err := scan.files.VisitFiles(scan.ctx, scope, nil, func(_ *os.Root, _, path string, metadata fs.FileInfo) (bool, error) {
		if scan.seen[path] || !Candidate(path) || !IndexedSource(path) {
			return true, nil
		}
		cost := min(metadata.Size(), MaxFileBytes)
		if scan.attempts >= queryFiles || scan.rows >= queryRows || cost > queryBytes-scan.bytes {
			scan.cut()
			return false, nil
		}
		scan.bytes += cost
		file, err := scan.read(path, match)
		if err != nil {
			if scan.ctx.Err() != nil {
				return false, scan.ctx.Err()
			}
			scan.seen[path] = true
			value := stateOf(new(Omitted(path, StatusInvalid)))
			scan.report.record(path, &value)
			return true, nil
		}
		if file != nil {
			scan.seen[path] = true
			value := stateOf(file)
			scan.report.record(path, &value)
			if file.Status == StatusParsed {
				scan.rows += len(file.Symbols) + len(file.References) + len(file.Imports)
				if scan.rows > queryRows {
					scan.cut()
					return false, nil
				}
				scan.parsed = append(scan.parsed, *file)
			}
		}
		return true, nil
	})
	scan.report.visited += walked.Visited
	scan.report.skipped += walked.Skipped
	if scan.ctx.Err() != nil || walked.Partial {
		scan.cut()
	}
	if scan.ctx.Err() != nil {
		return nil // The admitting context below distinguishes timeout and cancel.
	}
	var fault *protocol.Fault
	if scope != "" && errors.As(err, &fault) && fault.Code == protocol.ErrorCodeNotFound {
		return nil
	}
	return failure(err)
}

func (scan *liveFiles) index(ctx context.Context) (*index, error) {
	i, err := newTransientIndex(ctx)
	if err != nil {
		return nil, err
	}
	if _, _, err := i.replace(ctx, scan.parsed, Fingerprint); err != nil {
		_ = i.close()
		return nil, err
	}
	return i, nil
}

// OutlineOnDemand parses exactly one file and retains its content revision for
// subsequent range reads, without opening a derived database.
func (s *Service) OutlineOnDemand(ctx context.Context, id protocol.WorktreeID, root, path string) (protocol.CodeIndexOutline, error) {
	out := protocol.DefaultCodeIndexOutline()
	if err := validate(root, path); err != nil {
		return out, err
	}
	_, scan, close, err := s.liveFiles(ctx, id, root, false)
	if err != nil {
		return out, err
	}
	defer close()
	file, err := scan.read(path, nil)
	if err != nil {
		return out, failure(err)
	}
	value := stateOf(file)
	out.Path, out.Language, out.Status, out.Capabilities = path, value.Language, value.Status, value.Capabilities
	if file.Hash != "" {
		out.FileRevision = &file.Hash
	}
	out.Partial, out.Truncated = value.Partial, value.Truncated || len(file.Symbols) > maxOutlineSymbols
	out.Symbols = file.Symbols[:min(len(file.Symbols), maxOutlineSymbols)]
	return out, nil
}

// SearchOnDemand lexically selects candidate files before parsing. The same
// bounded full-text ranking used by persistent indexes runs in memory.
func (s *Service) SearchOnDemand(ctx context.Context, id protocol.WorktreeID, root string, query protocol.CodeIndexSearch) (protocol.CodeIndexSearchPage, error) {
	out := protocol.DefaultCodeIndexSearchPage()
	if query.Limit == 0 || len(query.Query) > 4096 || len(deref(query.Name)) > 512 || len(deref(query.Language)) > 128 {
		return out, fault(protocol.ErrorCodeInvalidRequest, "invalid code search limits")
	}
	if query.Path != nil {
		if err := validate(root, *query.Path); err != nil {
			return out, err
		}
	}
	operation, scan, close, err := s.liveFiles(ctx, id, root, false)
	if err != nil {
		return out, err
	}
	defer close()
	hints := []string{deref(query.Name)}
	if strings.TrimSpace(hints[0]) == "" {
		hints = strings.Split(knowledge.Query(query.Query), " OR ")
		for i := range hints {
			hints[i] = strings.Trim(hints[i], `"`)
		}
	}
	if err := scan.walk(deref(query.Path), expression(hints, false)); err != nil {
		return out, err
	}
	if operation.Err() != nil {
		return out, cancelled()
	}
	if len(scan.parsed) != 0 {
		i, err := scan.index(operation)
		if err != nil {
			return out, err
		}
		defer i.close()
		if out, err = i.search(operation, &query); err != nil {
			return out, err
		}
		out.IndexRevision = 0 // There is no reusable index snapshot.
	}
	status := scan.report.scoped(query.Path)
	out.Visited, out.Skipped, out.Issues = scan.report.visited, status.skipped, status.issues
	out.Partial, out.Truncated = out.Partial || status.partial, out.Truncated || status.truncated
	return out, nil
}

// graphHints expands from seeded declarations and their calls, so callers can
// cross files and callees can resolve even when a definition has another name.
func (scan *liveFiles) graphHints(query protocol.CodeIndexGraph) []string {
	hints := []string{}
	add := func(name string) {
		if name == "" || slices.Contains(hints, name) {
			return
		}
		if len(hints) >= queryHints {
			scan.cut()
			return
		}
		hints = append(hints, name)
	}
	add(deref(query.Name))
	for _, file := range scan.parsed {
		if len(query.Paths) > 0 && !slices.ContainsFunc(query.Paths, func(path string) bool { return within(file.Path, path) }) {
			continue
		}
		for _, symbol := range file.Symbols {
			at := symbol.Range
			if !definition(symbol.Kind) || query.Name != nil && symbol.Name != *query.Name || query.Line != nil && (*query.Line < at.StartLine || *query.Line > at.EndLine) {
				continue
			}
			add(symbol.Name)
			if query.View == protocol.CodeIndexGraphViewCallees || query.View == protocol.CodeIndexGraphViewContext {
				for _, ref := range file.References {
					if ref.Start >= at.StartByte && ref.Start < at.EndByte {
						add(ref.Name)
					}
				}
			}
		}
	}
	return hints
}

// GraphOnDemand builds a query-local graph from selected source and lexical
// matches. The same resolver supplies positions, ambiguity and confidence.
func (s *Service) GraphOnDemand(ctx context.Context, id protocol.WorktreeID, root string, query protocol.CodeIndexGraph) (protocol.CodeIndexGraphGraphPage, error) {
	out := protocol.DefaultCodeIndexGraphGraphPage()
	out.View = query.View
	if err := checkGraph(root, &query); err != nil {
		return out, err
	}
	operation, scan, close, err := s.liveFiles(ctx, id, root, true)
	if err != nil {
		return out, err
	}
	defer close()
	scopes := query.Paths
	if len(scopes) == 0 {
		scopes = []string{""}
	}
	var match *regexp.Regexp
	if query.View != protocol.CodeIndexGraphViewMap {
		match = expression([]string{deref(query.Name)}, true)
	}
	for _, path := range scopes {
		if err := scan.walk(path, match); err != nil {
			return out, err
		}
	}
	if query.View != protocol.CodeIndexGraphViewMap {
		hints := scan.graphHints(query)
		if len(hints) > 0 && (len(query.Paths) > 0 || len(hints) > 1) {
			if err := scan.walk("", expression(hints, true)); err != nil {
				return out, err
			}
		}
		// A name-focused scan cannot establish complete reverse paths beyond
		// its selected definitions without reading additional files.
		if query.Depth > 1 && (query.View == protocol.CodeIndexGraphViewContext || query.View == protocol.CodeIndexGraphViewImpact) {
			scan.report.partial = true
		}
	} else if len(query.Paths) > 0 {
		scan.report.partial = true
	}
	if operation.Err() != nil {
		return out, cancelled()
	}
	if len(scan.parsed) != 0 {
		i, err := scan.index(operation)
		if err != nil {
			return out, err
		}
		defer i.close()
		if out, err = i.graph(operation, &query); err != nil {
			return out, err
		}
		out.IndexRevision = 0
	}
	status := scan.report.scoped(nil)
	out.Visited, out.Skipped, out.Issues = scan.report.visited, status.skipped, status.issues
	out.Partial, out.Truncated = out.Partial || status.partial, out.Truncated || status.truncated
	return out, nil
}
