package codeindex

import (
	"cmp"
	"context"
	"io/fs"
	"maps"
	"os"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Reconciling scans the worktree, reads and parses bounded batches in
// parallel, then commits short writer transactions. The staged approach
// follows ast-index v3.56.0 (MIT).

// report is the scan-wide reconcile state plus the per-file problems that
// searches scope by path.
type report struct {
	visited uint64
	// skipped counts walk entries omitted without a path, such as links.
	skipped uint64
	// partial marks a reconcile that was cut short or could not prove freshness.
	partial   bool
	truncated bool
	problems  map[string]problem
}

type problem struct {
	status    Status
	partial   bool
	truncated bool
}

// scoped is search metadata limited to one requested path prefix.
type scoped struct {
	skipped   uint64
	partial   bool
	truncated bool
	issues    []protocol.CodeIndexIssue
}

// interrupted reports that another refresh or an exhausted walk cannot prove
// the index is current.
func interrupted() report { return report{partial: true, truncated: true} }

func (r *report) record(path string, state *state) {
	// Files without a supported grammar are expected and never degrade a result.
	if state.Status != StatusUnsupported && (state.Status != StatusParsed || state.Partial || state.Truncated) {
		r.problems[path] = problem{status: state.Status, partial: state.Partial, truncated: state.Truncated}
	} else {
		delete(r.problems, path)
	}
}

func (r *report) scoped(filter *string) scoped {
	found := scoped{skipped: r.skipped, partial: r.partial, truncated: r.truncated, issues: []protocol.CodeIndexIssue{}}
	for _, path := range slices.Sorted(maps.Keys(r.problems)) {
		if filter != nil && !within(path, *filter) {
			continue
		}
		problem := r.problems[path]
		found.partial = found.partial || problem.partial
		found.truncated = found.truncated || problem.truncated
		if problem.status != StatusParsed {
			found.skipped++
			if len(found.issues) < maxIssues {
				found.issues = append(found.issues, protocol.CodeIndexIssue{Path: path, Status: problem.status})
			}
		}
	}
	return found
}

type limits struct {
	files    int
	batches  int
	deadline time.Time
}

func defaultLimits() limits {
	return limits{files: maxFiles, batches: int(^uint(0) >> 1), deadline: time.Now().Add(15 * time.Second)}
}

// progress is the reconcile state a worktree keeps under its sync lock.
type progress struct {
	scan    *scan
	records records
}

// records are the file records kept between reconciles. Every content check
// still reads and hashes the file; this only avoids reloading the whole table
// per query.
type records struct {
	// revision is the index revision entries reflect; it is unknown once
	// another writer may have committed in between.
	revision uint64
	known    bool
	entries  map[string]cached
}

func (r *records) load(ctx context.Context, index *index) error {
	current, err := index.revision(ctx)
	if err != nil {
		return err
	}
	if !r.known || r.revision != current {
		revision, entries, err := index.snapshot(ctx)
		if err != nil {
			return err
		}
		r.revision, r.known, r.entries = revision, true, entries
	}
	return nil
}

// advance follows one transaction committed by the sync owner after its
// changes were applied to entries. A gap means another writer interleaved.
func (r *records) advance(revision uint64) {
	r.known = r.known && r.revision+1 == revision
	r.revision = revision
}

func (r *records) insert(file *Parsed, fingerprint string) state {
	entry := cached{fingerprint: fingerprint, state: stateOf(file)}
	if file.Hash != "" {
		entry.hash = &file.Hash
	}
	r.entries[file.Path] = entry
	return entry.state
}

type scan struct {
	generation uint64
	limit      int
	pending    []string
	versions   map[string]uint64
	seen       map[string]bool
	complete   bool
	report     report
}

// changed moves exactly invalidated files to the front of the cursor.
func (s *scan) changed(paths map[string]uint64) {
	for _, path := range slices.Backward(slices.Sorted(maps.Keys(paths))) {
		generation := paths[path]
		if known, ok := s.versions[path]; ok && known == generation || !s.seen[path] || !Candidate(path) {
			continue
		}
		// New paths must pass the next ignore-aware walk before entering this cursor.
		s.pending = slices.DeleteFunc(s.pending, func(pending string) bool { return pending == path })
		s.pending = slices.Insert(s.pending, 0, path)
		s.versions[path] = generation
	}
}

// inspected is one file's refresh: a new parse, nothing when the record is
// current, or the fault that prevented reading it.
type inspected struct {
	file *Parsed
	err  error
}

func prior(entries map[string]cached, path string) *cached {
	if entry, ok := entries[path]; ok {
		return &entry
	}
	return nil
}

// read reads, fully hashes and, when changed, parses files in order while
// holding one shared parsing permit.
func (s *Service) read(ctx context.Context, files *core.Files, paths []string, entries map[string]cached, deadline time.Time) ([]inspected, error) {
	timer := time.NewTimer(time.Until(deadline))
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return nil, cancelled()
	case <-timer.C:
		return nil, fault(protocol.ErrorCodeBusy, "code indexing budget exhausted")
	case s.parsing <- struct{}{}:
	}
	defer func() { <-s.parsing }()
	bounded, cancel := context.WithDeadline(ctx, deadline)
	defer cancel()
	results := make([]inspected, 0, len(paths))
	for _, path := range paths {
		if ctx.Err() != nil {
			return nil, cancelled()
		}
		file, err := s.inspect(ctx, bounded, files, path, prior(entries, path))
		results = append(results, inspected{file, err})
	}
	return results, nil
}

func (s *Service) inspect(ctx, bounded context.Context, files *core.Files, path string, prior *cached) (*Parsed, error) {
	source, err := files.BoundedText(bounded, path, MaxFileBytes)
	if err != nil {
		return nil, err
	}
	if source == nil {
		// A fresh size check proves the same omitted state. There are no
		// symbols or source revision to replace while the file is oversized.
		if prior != nil && prior.fingerprint == Fingerprint && prior.state.Status == StatusTooLarge {
			return nil, nil
		}
		file := Omitted(path, StatusTooLarge)
		return &file, nil
	}
	if prior != nil && prior.hash != nil && *prior.hash == Hash(*source) && prior.fingerprint == Fingerprint {
		return nil, nil
	}
	file, err := s.parser.Parse(ctx, path, *source)
	if err != nil {
		return nil, cancelled()
	}
	return &file, nil
}

// refreshed is the outcome of re-reading the files named by a result page.
type refreshed struct {
	changed bool
	checked map[string]bool
}

// refresh re-reads files under the sync lock and replaces changed ones, so
// returned revisions match disk even when a path-less watch event has not
// reached them in the scan cursor.
func (s *Service) refresh(ctx context.Context, worktree *worktree, index *index, progress *progress, paths []string, deadline time.Time) (refreshed, error) {
	result := refreshed{checked: map[string]bool{}}
	if len(paths) == 0 {
		return result, nil
	}
	if err := progress.records.load(ctx, index); err != nil {
		return result, err
	}
	files, err := core.OpenFiles(worktree.root, core.RestrictedFiles)
	if err != nil {
		return result, err
	}
	defer files.Close()
	results, err := s.read(ctx, files, paths, progress.records.entries, deadline)
	if err != nil {
		return result, err
	}
	var changes []Parsed
	for index, path := range paths {
		switch found := results[index]; {
		case found.err == nil:
			result.checked[path] = true
			if found.file != nil {
				changes = append(changes, *found.file)
			}
		case code(found.err) == protocol.ErrorCodeCancelled || code(found.err) == protocol.ErrorCodeNotConfigured:
			return result, found.err
		case code(found.err) == protocol.ErrorCodeBusy:
			// The scan resumes files left by the refresh budget.
		default:
			result.checked[path] = true
			changes = append(changes, Omitted(path, StatusInvalid))
		}
	}
	revision, committed, err := index.replace(ctx, changes, Fingerprint)
	if err != nil {
		return result, err
	}
	if committed {
		for index := range changes {
			progress.records.insert(&changes[index], Fingerprint)
		}
		progress.records.advance(revision)
	}
	result.changed = committed
	return result, nil
}

// list lists the worktree's files with the ignore rules of file discovery.
func list(ctx context.Context, files *core.Files, limit int) (core.FileListing, error) {
	listing := core.FileListing{}
	var err error
	listing.Report, err = files.VisitFiles(ctx, "", nil, func(_ *os.Root, _, relative string, _ fs.FileInfo) (bool, error) {
		if len(listing.Entries) >= limit {
			return false, nil
		}
		listing.Entries = append(listing.Entries, core.FoundFile{Path: relative})
		return true, nil
	})
	return listing, err
}

// synchronize advances the worktree's reconcile within its limits and
// reports how far the index can be trusted.
func (s *Service) synchronize(ctx context.Context, worktree *worktree, index *index, progress *progress, limits limits) (report, error) {
	if ctx.Err() != nil {
		return report{}, cancelled()
	}
	generation, dirty := worktree.dirty.snapshot()
	if progress.scan == nil || progress.scan.limit != limits.files {
		files, err := core.OpenFiles(worktree.root, core.RestrictedFiles)
		if err != nil {
			return report{}, err
		}
		bounded, cancel := context.WithDeadline(ctx, limits.deadline)
		listing, err := list(bounded, files, limits.files)
		cancel()
		files.Close()
		switch {
		case ctx.Err() != nil:
			return report{}, cancelled()
		case err != nil && code(err) != protocol.ErrorCodeBusy:
			return report{}, err
		case err != nil || !time.Now().Before(limits.deadline):
			return interrupted(), nil
		}
		seen := make(map[string]bool, len(listing.Entries))
		var pending []string
		for _, entry := range listing.Entries {
			seen[entry.Path] = true
			if Candidate(entry.Path) {
				pending = append(pending, entry.Path)
			}
		}
		slices.SortFunc(pending, func(left, right string) int {
			_, first := dirty[left]
			_, second := dirty[right]
			return cmp.Or(cmp.Compare(truth(!first), truth(!second)), strings.Compare(left, right))
		})
		progress.scan = &scan{
			generation: generation, limit: limits.files, pending: pending, versions: dirty, seen: seen,
			complete: !listing.Report.Partial && !listing.Report.Truncated,
			report:   report{visited: listing.Report.Visited, skipped: listing.Report.Skipped, partial: listing.Report.Partial, truncated: listing.Report.Truncated, problems: map[string]problem{}},
		}
	}
	scan, records := progress.scan, &progress.records
	if err := records.load(ctx, index); err != nil {
		return report{}, err
	}
	// One confined root handle serves this segment; the next opens it again.
	var files *core.Files
	if len(scan.pending) != 0 {
		var err error
		if files, err = core.OpenFiles(worktree.root, core.RestrictedFiles); err != nil {
			return report{}, err
		}
		defer files.Close()
	}
	for batches := 0; len(scan.pending) != 0 && batches < limits.batches && time.Now().Before(limits.deadline); batches++ {
		if ctx.Err() != nil {
			return report{}, cancelled()
		}
		// Root invalidations can come from ignored build output. Finish this
		// cursor before reconciling them; exact writes still move to its front.
		_, changed := worktree.dirty.snapshot()
		scan.changed(changed)
		batch := slices.Clone(scan.pending[:min(len(scan.pending), batchFiles)])
		// Shared permits bound all worktrees; no writer is held while parsing.
		// Each permit covers a short ordered run, not one task per file.
		results := make([]inspected, len(batch))
		var group sync.WaitGroup
		for start := 0; start < len(batch); start += batchFiles / readers {
			run := batch[start:min(start+batchFiles/readers, len(batch))]
			group.Go(func() {
				found, err := s.read(ctx, files, run, records.entries, limits.deadline)
				for index := range run {
					if err != nil {
						results[start+index] = inspected{err: err}
					} else {
						results[start+index] = found[index]
					}
				}
			})
		}
		group.Wait()
		var changes []Parsed
		var states []string
		exhausted := false
		for index, path := range batch {
			switch found := results[index]; {
			case found.err == nil && found.file != nil:
				changes = append(changes, *found.file)
			case found.err == nil:
				if _, ok := records.entries[path]; ok {
					states = append(states, path)
				}
			case code(found.err) == protocol.ErrorCodeCancelled || code(found.err) == protocol.ErrorCodeNotConfigured:
				return report{}, found.err
			case code(found.err) == protocol.ErrorCodeBusy:
				exhausted = true
			default:
				changes = append(changes, Omitted(path, StatusInvalid))
			}
		}
		if exhausted {
			break
		}
		committed, revisions, err := commit(ctx, index, changes, limits.deadline)
		if err != nil {
			return report{}, err
		}
		completed := map[string]bool{}
		for index := range changes[:committed] {
			state := records.insert(&changes[index], Fingerprint)
			scan.report.record(changes[index].Path, &state)
			completed[changes[index].Path] = true
		}
		for _, revision := range revisions {
			records.advance(revision)
		}
		for _, path := range states {
			state := records.entries[path].state
			scan.report.record(path, &state)
			completed[path] = true
		}
		// Keep every committed file and cached result when a later transaction
		// reaches the work budget; the remaining atomic files stay queued.
		// The batch is still the cursor front, so only that front is filtered.
		kept := slices.DeleteFunc(batch, func(path string) bool { return completed[path] })
		scan.pending = append(kept, scan.pending[len(results):]...)
		if committed < len(changes) {
			break
		}
	}
	if ctx.Err() != nil {
		return report{}, cancelled()
	}
	current := worktree.dirty.generation() == scan.generation
	finished := len(scan.pending) == 0
	if finished && scan.complete && current {
		// Absence is proven only by a complete walk. Committed pruning batches
		// also survive a budget boundary; unfinished deletions remain pending.
		var err error
		if finished, err = prune(ctx, index, records, scan.seen, limits.deadline); err != nil {
			return report{}, err
		}
	}
	result := scan.report
	result.problems = maps.Clone(scan.report.problems)
	worktree.dirty.mutex.Lock()
	defer worktree.dirty.mutex.Unlock()
	result.partial = result.partial || worktree.dirty.watchFailed || !current || !finished
	result.truncated = result.truncated || !finished || !current
	if finished && scan.complete {
		result.partial = result.partial || !worktree.dirty.finish(scan.generation)
	}
	if finished {
		progress.scan = nil
	}
	return result, nil
}

// prune removes records absent from a complete walk in committed batches of
// 64. It reports false when the deadline left deletions pending.
func prune(ctx context.Context, index *index, records *records, seen map[string]bool, deadline time.Time) (bool, error) {
	// Revalidate so records committed by any other writer are also pruned.
	if err := records.load(ctx, index); err != nil {
		return false, err
	}
	var absent []string
	for _, path := range slices.Sorted(maps.Keys(records.entries)) {
		if !seen[path] {
			absent = append(absent, path)
		}
	}
	for paths := range slices.Chunk(absent, 64) {
		if !time.Now().Before(deadline) {
			return false, nil
		}
		revision, err := index.remove(ctx, paths)
		if err != nil {
			return false, err
		}
		for _, path := range paths {
			delete(records.entries, path)
		}
		records.advance(revision)
	}
	return true, nil
}

// commit writes files in transactions of at most batchSymbols symbols and
// returns how many files were committed with their revisions.
func commit(ctx context.Context, index *index, files []Parsed, deadline time.Time) (int, []uint64, error) {
	start := 0
	var revisions []uint64
	for start < len(files) {
		if ctx.Err() != nil {
			return start, revisions, cancelled()
		}
		end, symbols := start+1, len(files[start].Symbols)
		for end < len(files) && symbols+len(files[end].Symbols) <= batchSymbols {
			symbols += len(files[end].Symbols)
			end++
		}
		if !time.Now().Before(deadline) {
			break
		}
		// The work budget stops new transactions, not an in-flight atomic file.
		// Caller cancellation and the admitted query deadline still roll it back.
		revision, committed, err := index.replace(ctx, files[start:end], Fingerprint)
		if err != nil {
			return start, revisions, err
		}
		if committed {
			revisions = append(revisions, revision)
		}
		start = end
	}
	return start, revisions, nil
}
