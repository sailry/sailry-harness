package codeindex

import (
	"cmp"
	"context"
	"io/fs"
	"maps"
	"os"
	"runtime"
	"runtime/metrics"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/odvcencio/gotreesitter"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Reconciling scans the worktree, reads and parses bounded batches, then
// commits short writer transactions. The staged approach
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
	// recheck marks a finished pass invalidated by a newer watch generation.
	recheck  bool
	problems map[string]problem
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
	scan     *scan
	records  records
	verified map[string]verified
}

// A stamp is only a shortcut for comparing this specific content hash.
type verified struct {
	stamp core.FileStamp
	hash  string
}

// records are the committed file records kept between reconciles.
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
		entry.hash = new(file.Hash)
	}
	r.entries[file.Path] = entry
	return entry.state
}

type scan struct {
	generation uint64
	limit      int
	pending    []string
	versions   map[string]uint64
	verified   map[string]verified
	// ready holds at most one batch parsed before the writer budget ran out.
	// The next segment checks the current bytes before using those parses.
	ready    map[string]*Parsed
	seen     map[string]bool
	complete bool
	report   report
}

// Stage only changed proofs. An unchanged hot scan must not allocate a second
// full table just to publish the same stamps at the end of the walk.
func (s *scan) remember(path string, hash *string, stamp core.FileStamp, previous map[string]verified) {
	var value verified
	if hash != nil && stamp != (core.FileStamp{}) {
		value = verified{stamp: stamp, hash: *hash}
	}
	if value == previous[path] {
		delete(s.verified, path)
		return
	}
	if s.verified == nil {
		s.verified = make(map[string]verified)
	}
	s.verified[path] = value
}

func (s *scan) publish(previous map[string]verified) map[string]verified {
	if previous == nil {
		return s.verified
	}
	for path := range previous {
		if !s.seen[path] {
			delete(previous, path)
		}
	}
	for path, value := range s.verified {
		if value.hash == "" {
			delete(previous, path)
		} else {
			previous[path] = value
		}
	}
	return previous
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
	file  *Parsed
	stamp core.FileStamp
	err   error
}

type inputs struct {
	entries  map[string]cached
	verified map[string]verified
	dirty    map[string]uint64
	ready    map[string]*Parsed
}

// read checks files in order under the original batch deadline.
func (s *Service) read(ctx context.Context, files *core.Files, paths []string, in inputs, deadline time.Time) ([]inspected, error) {
	bounded, cancel := context.WithDeadline(ctx, deadline)
	defer cancel()
	results := make([]inspected, 0, len(paths))
	for _, path := range paths {
		if ctx.Err() != nil {
			return nil, cancelled()
		}
		prior := in.entries[path]
		var stamp core.FileStamp
		if known, ok := in.verified[path]; ok && in.dirty[path] == 0 && prior.fingerprint == Fingerprint && prior.hash != nil && known.hash == *prior.hash {
			stamp = known.stamp
		}
		result := s.inspect(bounded, files, path, prior, stamp, in.ready[path])
		if ctx.Err() != nil {
			return nil, cancelled()
		}
		results = append(results, result)
	}
	return results, nil
}

func (s *Service) inspect(bounded context.Context, files *core.Files, path string, prior cached, previous core.FileStamp, ready *Parsed) inspected {
	release, err := s.acquireReader(bounded)
	if err != nil {
		return inspected{err: err}
	}
	defer release()
	var result inspected
	if !IndexedSource(path) {
		file, err := files.Open(path)
		if err != nil {
			return inspected{err: failure(err)}
		}
		_ = file.Close()
		if prior.fingerprint != Fingerprint || prior.state.Status != StatusUnsupported {
			file := Omitted(path, StatusUnsupported)
			result.file = &file
		}
		return result
	}
	result.stamp, result.err = s.inspectText(files, bounded, path, MaxFileBytes, previous, func(source *string) error {
		if source == nil {
			if prior.fingerprint != Fingerprint || prior.state.Status != StatusTooLarge {
				file := Omitted(path, StatusTooLarge)
				result.file = &file
			}
			return nil
		}
		input := parseInput{text: *source}
		if prior.hash != nil && prior.fingerprint == Fingerprint || ready != nil {
			hash := input.hash()
			switch {
			case prior.hash != nil && prior.fingerprint == Fingerprint && *prior.hash == hash:
				return nil
			case ready != nil && ready.Hash == hash:
				result.file = ready
				return nil
			}
		}
		file, err := s.parser.parseSource(bounded, path, input, sourceContent)
		if err == nil {
			result.file = &file
		}
		return err
	})
	result.err = failure(result.err)
	return result
}

// refreshed is the outcome of re-reading the files named by a result page.
type refreshed struct {
	changed bool
	checked map[string]bool
}

// refresh re-reads files under the sync lock and replaces changed ones, so
// returned revisions match disk even when a watch event has not
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
	results, err := s.read(ctx, files, paths, inputs{entries: progress.records.entries}, deadline)
	if err != nil {
		if code(err) == protocol.ErrorCodeBusy {
			return result, nil
		}
		return result, err
	}
	var changes []Parsed
	for index, path := range paths {
		switch found := results[index]; {
		case found.err == nil && found.file == nil:
			result.checked[path] = true
		case found.err == nil:
			changes = append(changes, *found.file)
		case code(found.err) == protocol.ErrorCodeCancelled || code(found.err) == protocol.ErrorCodeNotConfigured:
			return result, found.err
		case code(found.err) == protocol.ErrorCodeBusy || code(found.err) == protocol.ErrorCodeRevisionConflict:
			// The scan resumes files left by the refresh budget.
		default:
			changes = append(changes, Omitted(path, StatusInvalid))
		}
	}
	committed, revisions, err := commit(ctx, index, changes, deadline)
	for index := range changes[:committed] {
		file := &changes[index]
		progress.records.insert(file, Fingerprint)
		result.checked[file.Path] = true
	}
	for _, revision := range revisions {
		progress.records.advance(revision)
	}
	result.changed = committed > 0
	return result, err
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
		in := inputs{entries: records.entries, ready: scan.ready, dirty: scan.versions}
		worktree.dirty.mutex.Lock()
		var changed map[string]uint64
		if worktree.dirty.revision != scan.generation && len(scan.ready) == 0 {
			changed = maps.Clone(worktree.dirty.paths)
		}
		if !worktree.dirty.full && !worktree.dirty.watchFailed && len(scan.ready) == 0 {
			in.verified = progress.verified
		}
		worktree.dirty.mutex.Unlock()
		scan.changed(changed)
		length := min(len(scan.pending), batchFiles)
		if len(scan.ready) != 0 {
			// Commit revalidated parses before admitting the slow file that
			// exhausted the previous segment. Otherwise it can starve this prefix.
			length = len(scan.ready)
		}
		batch := slices.Clone(scan.pending[:length])
		// Workers check ordered runs without holding a writer. Each file
		// competes for the same reader as live queries and other worktrees.
		results := make([]inspected, len(batch))
		var group sync.WaitGroup
		for start := 0; start < len(batch); start += batchFiles / readers {
			run := batch[start:min(start+batchFiles/readers, len(batch))]
			group.Go(func() {
				found, err := s.read(ctx, files, run, in, limits.deadline)
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
		completed := map[string]bool{}
		exhausted := false
		for index, path := range batch {
			switch found := results[index]; {
			case found.err == nil && found.file != nil:
				changes = append(changes, *found.file)
			case found.err == nil:
				completed[path] = true
			case code(found.err) == protocol.ErrorCodeCancelled || code(found.err) == protocol.ErrorCodeNotConfigured:
				return report{}, found.err
			case code(found.err) == protocol.ErrorCodeBusy || code(found.err) == protocol.ErrorCodeRevisionConflict:
				exhausted = true
			default:
				changes = append(changes, Omitted(path, StatusInvalid))
			}
		}
		if len(changes) != 0 {
			releaseBatchScratch()
		}
		committed, revisions, err := commit(ctx, index, changes, limits.deadline)
		for index := range changes[:committed] {
			records.insert(&changes[index], Fingerprint)
			completed[changes[index].Path] = true
		}
		for _, revision := range revisions {
			records.advance(revision)
		}
		for i, path := range batch {
			if !completed[path] {
				continue
			}
			delete(scan.ready, path)
			entry := records.entries[path]
			scan.report.record(path, &entry.state)
			scan.remember(path, entry.hash, results[i].stamp, progress.verified)
		}
		for _, file := range changes[committed:] {
			if file.Hash != "" {
				if scan.ready == nil {
					scan.ready = make(map[string]*Parsed)
				}
				scan.ready[file.Path] = &file
			}
		}
		// Keep unfinished files queued at the front without copying the
		// unprocessed tail on every batch.
		kept := slices.DeleteFunc(batch, func(path string) bool { return completed[path] })
		if len(scan.ready) != 0 {
			slices.SortStableFunc(kept, func(left, right string) int {
				return cmp.Compare(truth(scan.ready[left] == nil), truth(scan.ready[right] == nil))
			})
		}
		advance := len(batch) - len(kept)
		copy(scan.pending[advance:len(batch)], kept)
		clear(scan.pending[:advance])
		scan.pending = scan.pending[advance:]
		if err != nil {
			return report{}, err
		}
		if exhausted || committed < len(changes) {
			break
		}
	}
	if ctx.Err() != nil {
		return report{}, cancelled()
	}
	finished := len(scan.pending) == 0
	if finished && scan.complete && worktree.dirty.generation() == scan.generation {
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
	current := worktree.dirty.revision == scan.generation
	result.partial = result.partial || worktree.dirty.watchFailed || !current || !finished
	result.truncated = result.truncated || !finished || !current
	result.recheck = finished && scan.complete && !current
	clean := finished && scan.complete && current
	if clean {
		clear(worktree.dirty.paths)
		worktree.dirty.full = false
	}
	publish := clean && !worktree.dirty.watchFailed
	worktree.dirty.mutex.Unlock()
	if publish {
		progress.verified = scan.publish(progress.verified)
	}
	if finished {
		progress.scan = nil
	}
	return result, nil
}

var scratchCollection sync.Mutex

// A cold scan can allocate several large parse trees before the next ordinary
// collection. At a batch boundary the workers have released their trees, and
// only the declarations, references and imports are needed for the write.
func releaseBatchScratch() {
	// The metrics and collection are process-wide. One worktree handles
	// pressure while any other worktrees continue their scans.
	if !scratchCollection.TryLock() {
		return
	}
	defer scratchCollection.Unlock()
	samples := [...]metrics.Sample{
		{Name: "/memory/classes/heap/objects:bytes"},
		{Name: "/gc/heap/live:bytes"},
	}
	metrics.Read(samples[:])
	heap, live := samples[0].Value.Uint64(), samples[1].Value.Uint64()
	const highHeap, heapGrowth = 384 << 20, 96 << 20
	// Require fresh growth as well as pressure, so a large live heap doesn't
	// cause repeated collections on batches that allocate little.
	if heap > highHeap && heap > live+heapGrowth {
		gotreesitter.DrainArenaPools()
		runtime.GC()
	}
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
