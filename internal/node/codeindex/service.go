package codeindex

import (
	"context"
	"maps"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	maxFiles   = 50_000
	batchFiles = 16
	// readers bounds the read and parse workers shared by all worktrees.
	readers   = 4
	maxIssues = 5
)

// Fingerprint identifies this engine's extraction rules in files.fingerprint.
// Records written under any other rules are parsed again.
var Fingerprint = Hash("code-index-v1:gotreesitter:v0.57.0:embedded-1:data-1:references-1")

// Watch subscribes to the changes of one worktree root.
type Watch func(ctx context.Context, root string, worktree protocol.WorktreeID) (link.Subscription, error)

// Service owns the code indexes of a profile, one per worktree.
type Service struct {
	profile string
	watch   Watch
	parser  *Parser
	parsing chan struct{}
	closed  context.Context
	close   context.CancelFunc

	mutex     sync.Mutex
	enabled   bool
	worktrees map[protocol.WorktreeID]*worktree
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

	opening sync.Mutex
	index   *index
}

// dirty counts invalidations and remembers the exactly changed paths.
type dirty struct {
	mutex       sync.Mutex
	revision    uint64
	paths       map[string]uint64
	watchFailed bool
}

func (d *dirty) change(path *string) {
	d.mutex.Lock()
	defer d.mutex.Unlock()
	d.revision++
	if path != nil {
		d.paths[*path] = d.revision
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

// finish forgets the changed paths a complete reconcile covered. The caller
// holds the mutex.
func (d *dirty) finish(generation uint64) bool {
	if d.revision != generation {
		return false
	}
	clear(d.paths)
	return true
}

// New returns the enabled index service of a profile directory.
func New(profile string, watch Watch) *Service {
	closed, cancel := context.WithCancel(context.Background())
	return &Service{profile: profile, watch: watch, parser: &Parser{}, parsing: make(chan struct{}, readers), closed: closed, close: cancel, enabled: true, worktrees: map[protocol.WorktreeID]*worktree{}}
}

func (s *Service) directory(id protocol.WorktreeID) string {
	return filepath.Join(s.profile, "cache", "code-index", id.String())
}

func (s *Service) worktree(id protocol.WorktreeID, root string) (*worktree, error) {
	s.mutex.Lock()
	defer s.mutex.Unlock()
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
		return known, nil
	}
	created := &worktree{root: root, database: filepath.Join(s.directory(id), "index.sqlite"), sync: make(chan *progress, 1), dirty: dirty{paths: map[string]uint64{}}}
	created.sync <- &progress{}
	created.stopped, created.stop = context.WithCancel(s.closed)
	s.worktrees[id] = created
	return created, nil
}

// release stops a worktree's queries and closes its index.
func (w *worktree) release() error {
	w.stop()
	// Readers release the index before it closes.
	w.access.Lock()
	defer w.access.Unlock()
	if w.index == nil {
		return nil
	}
	err := w.index.close()
	w.index = nil
	return err
}

// Configure enables or disables indexing. Disabling stops every query and
// closes the indexes; their files stay as a cache.
func (s *Service) Configure(enabled bool) {
	s.mutex.Lock()
	s.enabled = enabled
	var released []*worktree
	if !enabled {
		released = slices.Collect(maps.Values(s.worktrees))
		clear(s.worktrees)
	}
	s.mutex.Unlock()
	for _, worktree := range released {
		_ = worktree.release()
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

// Remove deletes the derived cache of a removed worktree.
func (s *Service) Remove(id protocol.WorktreeID) error {
	s.mutex.Lock()
	worktree := s.worktrees[id]
	s.mutex.Unlock()
	if worktree != nil {
		if err := worktree.release(); err != nil {
			return err
		}
	}
	if err := os.RemoveAll(s.directory(id)); err != nil {
		return failure(err)
	}
	s.mutex.Lock()
	defer s.mutex.Unlock()
	if s.worktrees[id] == worktree {
		delete(s.worktrees, id)
	}
	return nil
}

// begin scopes a query to its caller and its worktree and takes shared
// access to the index.
func (w *worktree) begin(ctx context.Context) (context.Context, func(), error) {
	ctx, cancel := context.WithCancel(ctx)
	detach := context.AfterFunc(w.stopped, cancel)
	w.access.RLock()
	end := func() {
		w.access.RUnlock()
		detach()
		cancel()
	}
	if ctx.Err() != nil || w.stopped.Err() != nil {
		end()
		return nil, nil, cancelled()
	}
	return ctx, end, nil
}

// observed subscribes once to the worktree's file changes. Watch events carry
// no paths, so each one invalidates the whole worktree.
func (w *worktree) observed(s *Service, id protocol.WorktreeID) {
	w.observe.Do(func() {
		events, err := s.watch(w.stopped, w.root, id)
		if err != nil {
			w.dirty.mutex.Lock()
			w.dirty.watchFailed = true
			w.dirty.mutex.Unlock()
			return
		}
		go func() {
			defer events.Close()
			for initial := true; ; initial = false {
				_, err := events.Next(w.stopped)
				if w.stopped.Err() != nil {
					return
				}
				if err != nil {
					w.dirty.fail()
					return
				}
				if !initial {
					w.dirty.change(nil)
				}
			}
		}()
	})
}

func (w *worktree) opened(ctx context.Context) (*index, error) {
	w.opening.Lock()
	defer w.opening.Unlock()
	if w.index != nil {
		return w.index, nil
	}
	directory := filepath.Dir(w.database)
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

// Outline refreshes one file and returns its declarations.
func (s *Service) Outline(ctx context.Context, id protocol.WorktreeID, root, path string) (protocol.CodeIndexOutline, error) {
	var outline protocol.CodeIndexOutline
	if err := validate(root, path); err != nil {
		return outline, err
	}
	worktree, err := s.worktree(id, root)
	if err != nil {
		return outline, err
	}
	ctx, end, err := worktree.begin(ctx)
	if err != nil {
		return outline, err
	}
	defer end()
	var progress *progress
	select {
	case <-ctx.Done():
		return outline, cancelled()
	case progress = <-worktree.sync:
	}
	defer func() { worktree.sync <- progress }()
	worktree.observed(s, id)
	if ctx.Err() != nil {
		return outline, cancelled()
	}
	index, err := worktree.opened(ctx)
	if err != nil {
		return outline, err
	}
	files, err := core.OpenFiles(root, core.RestrictedFiles)
	if err != nil {
		return outline, err
	}
	defer files.Close()
	if err := progress.records.load(ctx, index); err != nil {
		return outline, err
	}
	results, err := s.read(ctx, files, []string{path}, progress.records.entries, time.Now().Add(10*time.Second))
	if err != nil {
		return outline, err
	}
	if results[0].err != nil {
		return outline, failure(results[0].err)
	}
	if file := results[0].file; file != nil {
		revision, committed, err := index.replace(ctx, []Parsed{*file}, Fingerprint)
		if err != nil {
			return outline, err
		}
		if committed {
			progress.records.insert(file, Fingerprint)
			progress.records.advance(revision)
		}
	}
	if outline, err = index.outline(ctx, path); err != nil {
		return outline, err
	}
	worktree.dirty.mutex.Lock()
	outline.Partial = outline.Partial || worktree.dirty.watchFailed
	worktree.dirty.mutex.Unlock()
	return outline, nil
}

// reconcile refreshes the index when no other reader is doing so. It returns
// the held sync state for the caller to release, or nil.
func (s *Service) reconcile(ctx context.Context, worktree *worktree, index *index) (report, *progress, error) {
	select {
	case progress := <-worktree.sync:
		found, err := s.synchronize(ctx, worktree, index, progress, defaultLimits())
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
	worktree, err := s.worktree(id, root)
	if err != nil {
		return page, err
	}
	ctx, end, err := worktree.begin(ctx)
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
	// Watch events carry no paths, so a bounded scan can still return files
	// changed outside the agent. Matched files are re-read before their
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
	worktree, err := s.worktree(id, root)
	if err != nil {
		return page, err
	}
	ctx, end, err := worktree.begin(ctx)
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
	}
	return nil
}
