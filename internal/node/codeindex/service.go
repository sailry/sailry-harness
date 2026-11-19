package codeindex

import (
	"context"
	"maps"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/odvcencio/gotreesitter"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	maxFiles   = 50_000
	batchFiles = 16
	// readers bounds the read and parse workers shared by all worktrees.
	readers   = 1
	maxIssues = 5
	// Overflow falls back to a full content check.
	maxChanged = 4096
	// idleRelease is how long an unused worktree keeps its open index and
	// its file watch.
	idleRelease = 2 * time.Minute
)

// Fingerprint identifies this engine's extraction rules in files.fingerprint.
// Records written under any other rules are parsed again.
var Fingerprint = Hash("code-index-v2:gotreesitter:v0.57.0:embedded-1:source-1:references-2:budget-64m")

// Watch subscribes to changes of one worktree root. Its owner delivers every
// batch to Observed (empty for unknown changes) before waking the subscription.
// Successful wakes may coalesce; subscription errors report loss of the watch.
type Watch func(ctx context.Context, root string, worktree protocol.WorktreeID) (link.Subscription, error)

// Service owns the code indexes of a profile, one per worktree.
type Service struct {
	profile     string
	watch       Watch
	parser      *Parser
	inspectText func(*core.Files, context.Context, string, int, core.FileStamp, func(*string) error) (core.FileStamp, error)
	parsing     chan struct{}
	closed      context.Context
	close       context.CancelFunc

	// maintenance serializes operations that close or remove an index.
	maintenance sync.Mutex
	mutex       sync.Mutex
	enabled     bool
	worktrees   map[protocol.WorktreeID]*worktree
}

type worktree struct {
	root     string
	database string
	// access is shared by queries and exclusive for closing the index.
	access sync.RWMutex
	// sync holds the reconcile state; its owner is the one refreshing reader.
	sync    chan *progress
	dirty   dirty
	stopped context.Context
	stop    context.CancelFunc
	observe sync.Once
	// used is the end of the latest persistent query, guarded by the service.
	used time.Time

	opening   sync.Mutex
	index     *index
	cacheUsed time.Time
}

// dirty counts invalidations and remembers the exactly changed paths.
type dirty struct {
	mutex       sync.Mutex
	revision    uint64
	paths       map[string]uint64
	full        bool
	watchFailed bool
}

func (d *dirty) change(path *string) {
	d.mutex.Lock()
	defer d.mutex.Unlock()
	if path == nil {
		d.revision++
		d.full = true
		return
	}
	d.markPathLocked(*path)
}

func (d *dirty) markPathLocked(path string) {
	d.revision++
	if _, known := d.paths[path]; known || len(d.paths) < maxChanged {
		d.paths[path] = d.revision
	} else {
		d.full = true
	}
}

func (d *dirty) fail() {
	d.mutex.Lock()
	defer d.mutex.Unlock()
	d.watchFailed = true
	d.revision++
}

func (d *dirty) generation() uint64 {
	d.mutex.Lock()
	defer d.mutex.Unlock()
	return d.revision
}

func (d *dirty) snapshot() (uint64, map[string]uint64) {
	d.mutex.Lock()
	defer d.mutex.Unlock()
	return d.revision, maps.Clone(d.paths)
}

// New returns the enabled index service of a profile directory.
func New(profile string, watch Watch) *Service {
	closed, cancel := context.WithCancel(context.Background())
	service := &Service{profile: profile, watch: watch, parser: &Parser{}, inspectText: (*core.Files).InspectText, parsing: make(chan struct{}, readers), closed: closed, close: cancel, enabled: true, worktrees: map[protocol.WorktreeID]*worktree{}}
	go func() {
		ticker := time.NewTicker(time.Minute)
		defer ticker.Stop()
		lastPruned := time.Now()
		service.prune(lastPruned, cacheMaxAge, cacheMaxBytes)
		for {
			select {
			case <-closed.Done():
				return
			case now := <-ticker.C:
				service.releaseIdle(now)
				if now.Sub(lastPruned) >= cachePruneInterval {
					service.prune(now, cacheMaxAge, cacheMaxBytes)
					lastPruned = now
				}
			}
		}
	}()
	return service
}

// releaseIdle closes the index and ends the file watch of every worktree no
// query used for idleRelease, so a watch lasts only while its worktree is
// queried. The next query opens both again and reconciles by revision.
func (s *Service) releaseIdle(now time.Time) {
	s.maintenance.Lock()
	defer s.maintenance.Unlock()
	s.mutex.Lock()
	defer s.mutex.Unlock()
	for id, worktree := range s.worktrees {
		if now.Sub(worktree.used) >= idleRelease && worktree.access.TryLock() {
			worktree.stop()
			_ = worktree.closeIndex()
			worktree.access.Unlock()
			delete(s.worktrees, id)
		}
	}
}

func (s *Service) acquireReader(ctx context.Context) (func(), error) {
	select {
	case <-ctx.Done():
		return nil, failure(ctx.Err())
	case s.parsing <- struct{}{}:
		if err := ctx.Err(); err != nil {
			<-s.parsing
			return nil, failure(err)
		}
		return func() { <-s.parsing }, nil
	}
}

func (s *Service) directory(id protocol.WorktreeID) string {
	return filepath.Join(s.profile, "cache", "code-index", id.String())
}

func (s *Service) worktree(id protocol.WorktreeID, root string) (*worktree, error) {
	s.mutex.Lock()
	defer s.mutex.Unlock()
	return s.worktreeLocked(id, root)
}

func (s *Service) worktreeLocked(id protocol.WorktreeID, root string) (*worktree, error) {
	if s.closed.Err() != nil {
		return nil, cancelled()
	}
	if !s.enabled {
		return nil, fault(protocol.ErrorCodeNotConfigured, "code index is disabled")
	}
	if known := s.worktrees[id]; known != nil {
		if known.root != root {
			return nil, fault(protocol.ErrorCodeRevisionConflict, "worktree root changed")
		}
		if known.stopped.Err() != nil {
			return nil, cancelled()
		}
		return known, nil
	}
	created := &worktree{root: root, database: filepath.Join(s.directory(id), "index.turso"), sync: make(chan *progress, 1), dirty: dirty{paths: map[string]uint64{}}, used: time.Now()}
	created.sync <- &progress{}
	created.stopped, created.stop = context.WithCancel(s.closed)
	s.worktrees[id] = created
	return created, nil
}

// release waits for stopped queries to return before closing the index.
func (w *worktree) release() error {
	w.access.Lock()
	defer w.access.Unlock()
	return w.closeIndex()
}

func (w *worktree) closeIndex() error {
	if w.index == nil {
		return nil
	}
	err := w.index.close()
	w.index = nil
	touchCache(filepath.Dir(w.database))
	return err
}

// Configure enables or disables indexing. Disabling stops every query and
// closes the indexes; their files stay as a cache.
func (s *Service) Configure(enabled bool) {
	s.maintenance.Lock()
	defer s.maintenance.Unlock()
	s.mutex.Lock()
	s.enabled = enabled && s.closed.Err() == nil
	var released []*worktree
	if !s.enabled {
		released = slices.Collect(maps.Values(s.worktrees))
	}
	s.mutex.Unlock()
	for _, worktree := range released {
		worktree.stop()
	}
	for _, worktree := range released {
		_ = worktree.release()
	}
	if len(released) != 0 {
		s.mutex.Lock()
		clear(s.worktrees)
		s.mutex.Unlock()
	}
}

// Close stops the service for good.
func (s *Service) Close() {
	s.close()
	s.Configure(false)
}

// Changed invalidates a worktree, moving an exactly known path to the front
// of its next reconcile.
func (s *Service) Changed(id protocol.WorktreeID, path *string) {
	s.mutex.Lock()
	defer s.mutex.Unlock()
	if worktree := s.worktrees[id]; worktree != nil {
		worktree.dirty.change(path)
	}
}

// Observed invalidates the walk for one batch and marks exactly named source
// files for content checks. Empty batches and root/unknown paths require a full
// check. A directory or ignore-rule change still invalidates the walk even if
// none of its paths are index candidates.
func (s *Service) Observed(id protocol.WorktreeID, paths []string) {
	s.mutex.Lock()
	worktree := s.worktrees[id]
	s.mutex.Unlock()
	if worktree == nil {
		return
	}
	d := &worktree.dirty
	d.mutex.Lock()
	defer d.mutex.Unlock()
	before := d.revision
	if len(paths) == 0 {
		d.full = true
	}
	for _, path := range paths {
		relative, err := filepath.Rel(worktree.root, path)
		if err != nil || relative == "." || !filepath.IsLocal(relative) {
			d.full = true
			continue
		}
		relative = filepath.ToSlash(relative)
		// Git's own files are never indexed and change in bursts.
		if relative == ".git" || strings.HasPrefix(relative, ".git/") || strings.Contains(relative, "/.git/") || !Candidate(relative) {
			continue
		}
		d.markPathLocked(relative)
	}
	if d.revision == before {
		d.revision++
	}
}

// Remove deletes the derived cache of a removed worktree.
func (s *Service) Remove(id protocol.WorktreeID) error {
	s.maintenance.Lock()
	defer s.maintenance.Unlock()
	s.mutex.Lock()
	worktree := s.worktrees[id]
	if worktree == nil {
		defer s.mutex.Unlock()
		return failure(s.removeCache(id))
	}
	worktree.stop()
	s.mutex.Unlock()
	err := worktree.release()
	s.mutex.Lock()
	defer s.mutex.Unlock()
	delete(s.worktrees, id)
	if err != nil {
		return err
	}
	return failure(s.removeCache(id))
}

// begin scopes a query to its caller and its worktree and takes shared
// access to the index.
func (s *Service) begin(ctx context.Context, id protocol.WorktreeID, root string, persistent bool) (context.Context, *worktree, func(), error) {
	if ctx.Err() != nil {
		return nil, nil, nil, cancelled()
	}
	s.mutex.Lock()
	w, err := s.worktreeLocked(id, root)
	if err != nil {
		s.mutex.Unlock()
		return nil, nil, nil, err
	}
	w.access.RLock()
	s.mutex.Unlock()
	ctx, cancel := context.WithCancel(ctx)
	detach := context.AfterFunc(w.stopped, cancel)
	end := func() {
		if persistent {
			s.mutex.Lock()
			w.used = time.Now()
			s.mutex.Unlock()
		}
		w.access.RUnlock()
		detach()
		cancel()
	}
	if ctx.Err() != nil || w.stopped.Err() != nil {
		end()
		return nil, nil, nil, cancelled()
	}
	return ctx, w, func() {
		// Keep arenas reusable across batches in a query. The parser
		// pool keeps strong references until drained, even while idle.
		gotreesitter.DrainArenaPools()
		end()
	}, nil
}

// observed owns the subscription lifetime. Observed has already recorded
// every change represented by a successful (possibly coalesced) update.
func (w *worktree) observed(s *Service, id protocol.WorktreeID) {
	w.observe.Do(func() {
		events, err := s.watch(w.stopped, w.root, id)
		if err != nil {
			w.dirty.fail()
			return
		}
		go func() {
			defer events.Close()
			for {
				_, err := events.Next(w.stopped)
				if w.stopped.Err() != nil {
					return
				}
				if err != nil {
					w.dirty.fail()
					return
				}
			}
		}()
	})
}

func (w *worktree) opened(ctx context.Context) (*index, error) {
	w.opening.Lock()
	defer w.opening.Unlock()
	directory := filepath.Dir(w.database)
	if w.index != nil {
		if time.Since(w.cacheUsed) >= cachePruneInterval {
			w.cacheUsed = touchCache(directory)
		}
		return w.index, nil
	}
	cache := filepath.Dir(directory)
	for _, private := range []string{filepath.Dir(cache), cache, directory} {
		if err := core.PrivateDirectory(private); err != nil {
			return nil, failure(err)
		}
	}
	opened, err := openIndex(ctx, w.database)
	if err != nil {
		return nil, err
	}
	w.index = opened
	w.cacheUsed = touchCache(directory)
	return opened, nil
}

func validate(root string, paths ...string) error {
	if len(paths) == 0 {
		return nil
	}
	files, err := core.OpenFiles(root, core.RestrictedFiles)
	if err != nil {
		return err
	}
	defer files.Close()
	for _, path := range paths {
		if err := files.ValidatePath(path, false); err != nil {
			return err
		}
	}
	return nil
}

// reconcile refreshes the index when no other reader is doing so. It returns
// the held sync state for the caller to release, or nil.
func (s *Service) reconcile(ctx context.Context, worktree *worktree, index *index) (report, *progress, error) {
	select {
	case progress := <-worktree.sync:
		budget := defaultLimits()
		found, err := s.synchronize(ctx, worktree, index, progress, budget)
		if err == nil && found.recheck && time.Now().Before(budget.deadline) {
			// A watch event can arrive during a completed pass. Follow
			// it once under the original deadline; continued writes
			// leave the result partial for the next query.
			found, err = s.synchronize(ctx, worktree, index, progress, budget)
		}
		if err != nil {
			worktree.sync <- progress
			return report{}, nil, err
		}
		return found, progress, nil
	default:
		// Another reader is already refreshing this worktree. Its committed
		// transactions remain readable without queuing a second full scan.
		return interrupted(), nil, nil
	}
}

// Search finds indexed declarations by name and full-text terms.
func (s *Service) Search(ctx context.Context, id protocol.WorktreeID, root string, query protocol.CodeIndexSearch) (protocol.CodeIndexSearchPage, error) {
	var page protocol.CodeIndexSearchPage
	if query.Limit == 0 || len(query.Query) > 4096 || len(deref(query.Name)) > 512 || len(deref(query.Language)) > 128 {
		return page, fault(protocol.ErrorCodeInvalidRequest, "invalid code search limits")
	}
	if query.Path != nil {
		if err := validate(root, *query.Path); err != nil {
			return page, err
		}
	}
	ctx, worktree, end, err := s.begin(ctx, id, root, true)
	if err != nil {
		return page, err
	}
	defer end()
	worktree.observed(s, id)
	index, err := worktree.opened(ctx)
	if err != nil {
		return page, err
	}
	report, progress, err := s.reconcile(ctx, worktree, index)
	if err != nil {
		return page, err
	}
	// A bounded scan can still return files whose change has not reached
	// the front of its cursor. Matched files are re-read before their
	// revisions reach revision-checked line reads.
	if progress != nil {
		page, err = s.validated(ctx, worktree, index, progress, &query)
		worktree.sync <- progress
	} else {
		page, err = index.search(ctx, &query)
	}
	if err != nil {
		return page, err
	}
	scoped := report.scoped(query.Path)
	page.Visited, page.Skipped, page.Issues = report.visited, scoped.skipped, scoped.issues
	page.Partial = page.Partial || scoped.partial
	page.Truncated = page.Truncated || scoped.truncated
	return page, nil
}

func (s *Service) validated(ctx context.Context, worktree *worktree, index *index, progress *progress, query *protocol.CodeIndexSearch) (protocol.CodeIndexSearchPage, error) {
	page, err := index.search(ctx, query)
	if err != nil {
		return page, err
	}
	checked := map[string]bool{}
	deadline := time.Now().Add(10 * time.Second)
	for {
		paths := names(page.Symbols, func(symbol *Symbol) string { return symbol.Path })
		paths = slices.DeleteFunc(paths, func(path string) bool { return checked[path] })
		if len(paths) == 0 {
			return page, nil
		}
		refreshed, err := s.refresh(ctx, worktree, index, progress, paths, deadline)
		if err != nil {
			return page, err
		}
		maps.Copy(checked, refreshed.checked)
		if refreshed.changed {
			if page, err = index.search(ctx, query); err != nil {
				return page, err
			}
		}
		if len(refreshed.checked) == 0 || !time.Now().Before(deadline) {
			page.Symbols = slices.DeleteFunc(page.Symbols, func(symbol Symbol) bool { return !checked[symbol.Path] })
			page.Partial, page.Truncated = true, true
			return page, nil
		}
	}
}

// Graph traces name-resolved references between indexed definitions.
func (s *Service) Graph(ctx context.Context, id protocol.WorktreeID, root string, query protocol.CodeIndexGraph) (protocol.CodeIndexGraphGraphPage, error) {
	var page protocol.CodeIndexGraphGraphPage
	if err := checkGraph(root, &query); err != nil {
		return page, err
	}
	ctx, worktree, end, err := s.begin(ctx, id, root, true)
	if err != nil {
		return page, err
	}
	defer end()
	worktree.observed(s, id)
	index, err := worktree.opened(ctx)
	if err != nil {
		return page, err
	}
	report, progress, err := s.reconcile(ctx, worktree, index)
	if err != nil {
		return page, err
	}
	if progress != nil {
		worktree.sync <- progress
	}
	if page, err = index.graph(ctx, &query); err != nil {
		return page, err
	}
	scoped := report.scoped(nil)
	page.Visited, page.Skipped, page.Issues = report.visited, scoped.skipped, scoped.issues
	page.Partial = page.Partial || scoped.partial
	page.Truncated = page.Truncated || scoped.truncated
	return page, nil
}

func checkGraph(root string, query *protocol.CodeIndexGraph) error {
	invalid := fault(protocol.ErrorCodeInvalidRequest, "invalid code graph query")
	if query.Limit == 0 || query.Limit > maxGraphItems || query.Offset > maxGraphOffset || query.Depth == 0 || query.Depth > maxGraphDepth ||
		len(query.Paths) > maxGraphPaths || query.Name != nil && (strings.TrimSpace(*query.Name) == "" || len(*query.Name) > maxNameBytes) ||
		query.Line != nil && len(query.Paths) != 1 {
		return invalid
	}
	if err := validate(root, query.Paths...); err != nil {
		return err
	}
	selected := query.Name != nil || query.Line != nil
	switch query.View {
	case protocol.CodeIndexGraphViewCallers, protocol.CodeIndexGraphViewCallees, protocol.CodeIndexGraphViewContext:
		if !selected {
			return invalid
		}
	case protocol.CodeIndexGraphViewImpact:
		if !selected && len(query.Paths) == 0 {
			return invalid
		}
	case protocol.CodeIndexGraphViewMap:
		return nil
	default:
		return invalid
	}
	return nil
}
