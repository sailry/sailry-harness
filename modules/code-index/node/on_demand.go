//go:build !sailry_no_code_index

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
	"github.com/sailry/sailry-harness/internal/node/codeindexapi"
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

var errScanBudget = errors.New("live code scan budget exhausted")

// liveFiles owns a bounded, query-local source selection. It does not open a
// worktree database or start a watch.
type liveFiles struct {
	ctx      context.Context
	service  *Service
	files    *core.Files
	scope    liveScope
	content  parseContent
	parsed   []Parsed
	seen     map[string]bool
	report   report
	bytes    int64
	rows     int
	attempts int
	started  bool
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
	bounded, cancel := context.WithTimeoutCause(ctx, queryTime, errScanBudget)
	close := func() { cancel(); _ = files.Close(); end() }
	content := outlineContent
	if references {
		content = sourceContent
	}
	return ctx, &liveFiles{ctx: bounded, service: s, files: files, scope: liveScope{id, root}, content: content, seen: map[string]bool{}, report: report{problems: map[string]problem{}}}, close, nil
}

func (scan *liveFiles) read(path string, match *regexp.Regexp) (*Parsed, error) {
	if err := scan.ctx.Err(); err != nil {
		return nil, err
	}
	// Bound the source buffer as well as the parser, but yield between files
	// and before the transient index is built or queried.
	release, err := scan.service.acquireReader(scan.ctx)
	if err != nil {
		if context.Cause(scan.ctx) == errScanBudget {
			return nil, fault(protocol.ErrorCodeBusy, codeindexapi.ReaderWaitMessage)
		}
		return nil, err
	}
	defer release()
	scan.started = true
	key := liveFileKey{scan.scope, path, scan.content}
	prior := scan.service.live.proof(key)
	pattern := ""
	if match != nil {
		pattern = match.String()
	}
	var previous core.FileStamp
	cached, known := prior.selected(pattern)
	if prior.stamp != (core.FileStamp{}) {
		if !known && prior.file != nil && match != nil && matchesSource(match, path) {
			cached, known = prior.file, true
		}
		if known {
			previous = prior.stamp
		}
	}

	var parsed *Parsed
	inspected := false
	stamp, err := scan.service.inspectText(scan.files, scan.ctx, path, MaxFileBytes, previous, func(source *string) error {
		inspected = true
		if source == nil {
			file := Omitted(path, StatusTooLarge)
			parsed = &file
			return nil
		}
		if match != nil && !matchesSource(match, path) && !matchesSource(match, *source) {
			return nil
		}
		scan.attempts++
		input := parseInput{text: *source}
		if prior.file != nil && prior.file.Hash != "" && input.hash() == prior.file.Hash {
			parsed = prior.file
			return nil
		}
		file, err := scan.service.parser.parseSource(scan.ctx, path, input, scan.content)
		parsed = &file
		return err
	})
	if err != nil {
		return nil, err
	}
	if !inspected {
		if previous == (core.FileStamp{}) || stamp != previous {
			return nil, fault(protocol.ErrorCodeInternal, "code index: file inspection did not validate source")
		}
		if cached != nil && cached.Status != StatusTooLarge {
			scan.attempts++ // Hits consume the same selection budget as cold parses.
		}
		return cached, nil
	}

	proof := liveProof{stamp: stamp}
	if stamp != (core.FileStamp{}) && stamp == prior.stamp {
		proof = prior
	}
	if parsed != nil && (stamp != (core.FileStamp{}) || parsed.Hash != "") {
		if parsed == prior.file {
			proof.file, proof.bytes = prior.file, prior.bytes
		} else {
			proof.file, proof.bytes = ownLiveParse(parsed)
		}
	}
	proof.remember(pattern, parsed != nil)
	if proof != prior {
		scan.service.live.saveProof(key, proof)
	}
	if parsed != nil && proof.file != nil {
		return proof.file, nil
	}
	return parsed, nil
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
	var refusal *protocol.Fault
	walked, err := scan.files.VisitFiles(scan.ctx, scope, nil, func(_ *os.Root, _, path string, metadata fs.FileInfo) (bool, error) {
		if scan.seen[path] || !indexedCandidate(path) {
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
			if f, ok := err.(*protocol.Fault); !scan.started && ok && ReaderWaitExpired(f) {
				// No source was inspected. The caller received no coverage,
				// unlike a scan that exhausted its budget after making progress.
				refusal = f
				return false, nil
			}
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
	if refusal != nil {
		return refusal
	}
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
	mode := appendSearchFiles
	if scan.content == sourceContent {
		mode = appendGraphFiles
	}
	if _, _, err := i.writeFiles(ctx, scan.parsed, Fingerprint, mode); err != nil {
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
		out.FileRevision = new(file.Hash)
	}
	out.Partial, out.Truncated = value.Partial, value.Truncated || len(file.Symbols) > maxOutlineSymbols
	out.Symbols = slices.Clone(file.Symbols[:min(len(file.Symbols), maxOutlineSymbols)])
	for i := range out.Symbols {
		if signature := out.Symbols[i].Signature; signature != nil {
			out.Symbols[i].Signature = new(*signature)
		}
	}
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
		if out, err = indexedLivePage(operation, scan, &query, (*index).search); err != nil {
			return out, err
		}
		out.IndexRevision = 0 // On-demand results have no persistent revision.
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
			if match == nil || len(hints) > 1 {
				match = expression(hints, true)
			}
			if err := scan.walk("", match); err != nil {
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
		if out, err = indexedLivePage(operation, scan, &query, (*index).graph); err != nil {
			return out, err
		}
		out.IndexRevision = 0
	}
	status := scan.report.scoped(nil)
	out.Visited, out.Skipped, out.Issues = scan.report.visited, status.skipped, status.issues
	out.Partial, out.Truncated = out.Partial || status.partial, out.Truncated || status.truncated
	return out, nil
}
