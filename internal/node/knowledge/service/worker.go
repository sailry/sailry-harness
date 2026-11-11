package service

import (
	"context"
	"log/slog"
	"maps"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/node/knowledge/source"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func indexingCancelled(ctx context.Context) error {
	if ctx.Err() != nil {
		return newFault(protocol.ErrorCodeCancelled, "knowledge indexing was cancelled")
	}
	return nil
}

func message(err error) string {
	if fault, ok := err.(*protocol.Fault); ok {
		return fault.Message
	}
	return err.Error()
}

// attempts bounds how often a pass reads a source that keeps failing with
// unchanged metadata, and how often a failed collection is retried on the
// timer. A changed file, a mutation and Invalidate start the count again.
const attempts = 3

// reported bounds the failures named in a collection's progress.
const reported = 8

// failed is one source a pass could not index.
type failed struct {
	stat      source.Stat
	reason    string
	attempts  int
	directory bool
}

// failures is what the last pass of one collection could not index. It is
// kept in memory: a restart tries every source again.
type failures struct {
	generation, epoch uint64
	sources           map[string]failed
	// collection counts the consecutive passes that failed as a whole.
	collection int
}

// retryable reports failures a later pass may still resolve.
func (failures *failures) retryable() bool {
	if failures.collection > 0 {
		return failures.collection < attempts
	}
	for _, source := range failures.sources {
		if source.attempts < attempts {
			return true
		}
	}
	return false
}

// progress adds the failures to a collection's progress, the first by path.
func (failures *failures) progress(progress *protocol.KnowledgeProgress) {
	progress.Failed = uint64(len(failures.sources))
	progress.Failures = []protocol.KnowledgeFailure{}
	for _, path := range slices.Sorted(maps.Keys(failures.sources)) {
		if len(progress.Failures) == reported {
			break
		}
		progress.Failures = append(progress.Failures, protocol.KnowledgeFailure{Path: path, Reason: failures.sources[path].reason})
	}
}

func (service *Service) run() {
	defer close(service.done)
	watches := newWatches(service.Refresh)
	defer watches.close()
	for service.wait(watches.needsPoll() || service.retry) {
		service.mu.Lock()
		ctx := service.pass
		service.mu.Unlock()
		err := service.sweep(ctx, watches)
		if err != nil {
			watches.finish(false)
			if ctx.Err() == nil && code(err) != protocol.ErrorCodeCancelled {
				slog.Warn("knowledge catalog is unavailable", "error", message(err))
			}
		}
	}
}

// wait blocks until a pass is due. A burst of requests ends after a quiet
// interval, bounded for continuous edits. poll also runs a pass after five
// minutes without a request.
func (service *Service) wait(poll bool) bool {
	clock := service.options.Clock
	var timeout <-chan time.Time
	if poll {
		timeout = clock.After(5 * time.Minute)
	}
	select {
	case <-service.stop.Done():
		return false
	case <-timeout:
		return true
	case <-service.wake:
	}
	started := clock.Now()
	for {
		remaining := time.Second - clock.Since(started)
		if remaining <= 0 {
			return true
		}
		select {
		case <-service.stop.Done():
			return false
		case <-service.wake:
		case <-clock.After(min(remaining, 250*time.Millisecond)):
			return true
		}
	}
}

// sweep runs one pass over every collection. A source that cannot be read
// is recorded on its collection and the pass continues; a collection that
// fails as a whole is reported paused with its reason and the others
// continue. Both are tried again on the next pass.
func (service *Service) sweep(ctx context.Context, watches *watches) error {
	watches.begin()
	catalog, err := service.options.Catalog(ctx)
	if err != nil {
		return err
	}
	choice := service.options.Choice()
	installed, err := service.options.Installed()
	if err != nil {
		return err
	}
	ids := make([]protocol.KnowledgeID, len(catalog))
	for position, stored := range catalog {
		ids[position] = stored.Collection.ID
	}
	if err := service.index.Retain(ctx, ids); err != nil {
		return err
	}
	maps.DeleteFunc(service.failures, func(id protocol.KnowledgeID, _ *failures) bool { return !slices.Contains(ids, id) })
	epoch := service.epoch.Load()
	complete, retry := true, false
	for _, stored := range catalog {
		if err := indexingCancelled(ctx); err != nil {
			return err
		}
		generation := stored.Collection.Progress.Generation
		previous := service.failures[stored.Collection.ID]
		if previous == nil || previous.generation != generation || previous.epoch != epoch {
			previous = &failures{}
		}
		found := &failures{generation: generation, epoch: epoch, sources: map[string]failed{}}
		service.failures[stored.Collection.ID] = found
		err := service.collection(ctx, stored, choice, installed, watches, previous, found)
		if err == nil {
			retry = retry || found.retryable()
			continue
		}
		if err := indexingCancelled(ctx); err != nil {
			return err
		}
		complete = false
		found.collection = previous.collection + 1
		found.sources = map[string]failed{}
		retry = retry || found.retryable()
		slog.Warn("knowledge indexing paused", "collection", stored.Collection.ID.String(), "error", message(err))
		sources, chunks, _ := service.index.Counts(ctx, service.binding(stored, choice))
		_ = service.options.Progress(ctx, stored.Collection.ID, generation, protocol.KnowledgeProgress{
			Status: protocol.KnowledgeStatusPaused, Sources: sources, Completed: sources, Chunks: chunks,
			Skipped: stored.Collection.Progress.Skipped, Generation: generation,
			Failures: []protocol.KnowledgeFailure{{Reason: message(err)}},
		})
	}
	if err := indexingCancelled(ctx); err != nil {
		return err
	}
	service.retry = retry
	watches.finish(complete)
	return nil
}

func (service *Service) collection(ctx context.Context, stored store.KnowledgeStored, choice protocol.ModelsLocalChoice, installed map[string]bool, watches *watches, previous, found *failures) error {
	index := service.index
	binding := service.binding(stored, choice)
	if err := index.Bind(ctx, binding); err != nil {
		return err
	}
	report := func(progress protocol.KnowledgeProgress) error {
		return service.options.Progress(ctx, binding.ID, binding.Generation, progress)
	}
	if stored.Collection.Paused {
		sources, chunks, err := index.Counts(ctx, binding)
		if err != nil {
			return err
		}
		return report(protocol.KnowledgeProgress{
			Status: protocol.KnowledgeStatusPaused, Sources: sources, Completed: sources, Chunks: chunks,
			Skipped: stored.Collection.Progress.Skipped, Generation: binding.Generation,
		})
	}
	model := ""
	if installed[binding.Model] {
		model = binding.Model
	}
	progress := protocol.KnowledgeProgress{Status: protocol.KnowledgeStatusIndexing, Generation: binding.Generation}
	if model != "" {
		progress.Model = &model
	}
	definition := stored.Collection.Definition
	filters := make([]*regexp.Regexp, len(definition.Globs))
	for position, pattern := range definition.Globs {
		glob, err := knowledge.Glob(pattern)
		if err != nil {
			return invalid("invalid knowledge glob")
		}
		filters[position] = glob
	}
	selected := store.ReadKnowledgeSource(definition.Source)
	if selected.Kind == "storage" {
		return service.storage(ctx, stored, binding, model, choice, progress, report)
	}
	var root, watched string
	var err error
	switch selected.Kind {
	case "worktree":
		if root, err = service.options.Root(ctx, selected.Worktree); err != nil {
			return err
		}
		watched = root
	case "directory":
		if watched, err = knowledge.Native(selected.Path); err != nil {
			return err
		}
	case "file":
		if watched, err = knowledge.Native(selected.Path); err != nil {
			return err
		}
		watched = filepath.Dir(watched)
	}
	scan, err := service.scan(ctx, selected, root)
	if err != nil {
		return err
	}
	defer scan.Close()
	watches.add(watched, scan.TakeWatches())
	matches := func(path string) bool {
		if len(filters) == 0 {
			return true
		}
		relative := path
		switch selected.Kind {
		case "directory":
			if below, found := within(selected.Path, path); found {
				relative = below
			}
		case "file":
			relative = filepath.Base(path)
		}
		for _, filter := range filters {
			if filter.MatchString(relative) {
				return true
			}
		}
		return false
	}
	active, working := false, func() {}
	defer func() { working() }()
	starting := func() error {
		if active {
			return nil
		}
		active = true
		if service.options.Working != nil {
			working = service.options.Working()
		}
		return report(progress)
	}
	clock := service.options.Clock
	last := clock.Now()
	// locators are the sources to keep; kept are the directories whose
	// published sources stay because the scan could not enter them.
	var locators, kept []string
	separator := string(filepath.Separator)
	if selected.Kind == "worktree" {
		separator = "/"
	}
	// reading is the file whose stamp the scan last asked about, so a
	// failure can be tied to the metadata it failed with.
	var reading struct {
		path string
		stat source.Stat
	}
	// fail records a source this pass could not index. Its published text
	// stays until the source is read again.
	fail := func(path, reason string, stat source.Stat, directory, tried bool) {
		entry := failed{stat: stat, reason: reason, attempts: 1, directory: directory}
		if before, known := previous.sources[path]; known && before.stat == stat && before.directory == directory {
			entry.attempts = before.attempts
			if tried {
				entry.attempts++
			}
		}
		if tried {
			slog.Info("knowledge source failed", "collection", binding.ID.String(), "path", path, "reason", reason, "attempt", entry.attempts)
		}
		found.sources[path] = entry
		locators = append(locators, path)
		if directory {
			kept = append(kept, path+separator)
		}
	}
	for {
		var fatal error
		exhausted := false
		entry, err := scan.Next(func(path string, stat source.Stat) (bool, error) {
			if !matches(path) {
				return true, nil
			}
			reading.path, reading.stat = path, stat
			cached, err := index.Cached(ctx, binding, path, stat, definition.Tags, model != "")
			if err == nil && !cached {
				if before, known := previous.sources[path]; known && before.stat == stat && before.attempts >= attempts {
					// It failed this often with the same metadata; do
					// not read it again until it changes.
					exhausted = true
					return false, newFault(protocol.ErrorCodeUnavailable, before.reason)
				}
				err = starting()
			}
			fatal = err
			return cached, err
		}, func(found []source.Watch) { watches.add(watched, found) })
		if fatal != nil {
			return fatal
		}
		if err != nil {
			return err
		}
		if entry == nil {
			break
		}
		if err := indexingCancelled(ctx); err != nil {
			return err
		}
		switch {
		case entry.Cached:
			if matches(entry.Path) {
				progress.Sources++
				progress.Completed++
				locators = append(locators, entry.Path)
			}
		case entry.Failed:
			if entry.Directory || matches(entry.Path) {
				var stat source.Stat
				if reading.path == entry.Path {
					stat = reading.stat
				}
				fail(entry.Path, entry.Reason, stat, entry.Directory, !exhausted)
			}
		case entry.Document == nil:
			if !entry.Excluded {
				progress.Skipped++
				slog.Info("knowledge source skipped", "collection", binding.ID.String(), "path", entry.Path, "reason", entry.Reason)
			}
		case matches(entry.Document.Path):
			document := entry.Document
			reason, err := service.publish(ctx, stored, selected, root, binding, model, choice, document, starting)
			if err != nil {
				return err
			}
			if reason != "" {
				fail(document.Path, reason, document.Stat, false, true)
				break
			}
			progress.Sources++
			locators = append(locators, document.Path)
			progress.Completed++
		}
		if active && clock.Since(last) >= 500*time.Millisecond {
			if _, progress.Chunks, err = index.Counts(ctx, binding); err != nil {
				return err
			}
			found.progress(&progress)
			if err := report(progress); err != nil {
				return err
			}
			last = clock.Now()
		}
	}
	if err := indexingCancelled(ctx); err != nil {
		return err
	}
	if err := index.Prune(ctx, binding, locators, kept); err != nil {
		return err
	}
	if progress.Sources, progress.Chunks, err = index.Counts(ctx, binding); err != nil {
		return err
	}
	progress.Completed = progress.Sources
	progress.Status = protocol.KnowledgeStatusReady
	found.progress(&progress)
	return report(progress)
}

// publish chunks, embeds and replaces one extracted document. It returns
// the reason when this document could not be indexed and an error when the
// collection cannot continue.
func (service *Service) publish(ctx context.Context, stored store.KnowledgeStored, selected store.KnowledgeSource, root string, binding Binding, model string, choice protocol.ModelsLocalChoice, document *source.Document, starting func() error) (string, error) {
	index := service.index
	indexed := Document{Locator: document.Path, Revision: document.Revision, ModifiedMs: document.ModifiedMs, Embedded: model != "", Tags: stored.Collection.Definition.Tags}
	unchanged, err := index.Unchanged(ctx, binding, indexed)
	if err != nil {
		return "", err
	}
	if !unchanged {
		if err := starting(); err != nil {
			return "", err
		}
		var parts []Part
		for _, part := range document.Parts {
			encoded, err := service.parts(ctx, model, choice, part)
			if err != nil {
				if cancelled := indexingCancelled(ctx); cancelled != nil {
					return "", cancelled
				}
				return message(err), nil
			}
			parts = append(parts, encoded...)
		}
		if err := indexingCancelled(ctx); err != nil {
			return "", err
		}
		same, err := service.unchanged(ctx, selected, root, document)
		if err != nil {
			if cancelled := indexingCancelled(ctx); cancelled != nil {
				return "", cancelled
			}
			return message(err), nil
		}
		if !same {
			return "knowledge file changed before publication", nil
		}
		if selected.Kind == "worktree" {
			moved, err := service.options.Root(ctx, selected.Worktree)
			if err != nil {
				return "", err
			}
			if moved != root {
				return "", newFault(protocol.ErrorCodeRevisionConflict, "knowledge worktree moved")
			}
		}
		if err := index.Replace(ctx, binding, indexed, parts); err != nil {
			return "", err
		}
	}
	return "", index.Remember(ctx, binding, document.Path, document.Stat)
}

func (service *Service) scan(ctx context.Context, selected store.KnowledgeSource, root string) (*source.Scan, error) {
	var scan *source.Scan
	var err error
	if selected.Kind == "worktree" {
		scan, err = source.Worktree(ctx, root, selected.Path)
	} else {
		scan, err = source.Controller(ctx, selected.Kind, selected.Path)
	}
	if err != nil {
		return nil, err
	}
	scan.SetOCR(service.options.OCR)
	return scan, nil
}

// unchanged rechecks the metadata witness of a document after encoding.
// Citations always reread the bytes; a writer preserving size and time
// evades this witness.
func (service *Service) unchanged(ctx context.Context, selected store.KnowledgeSource, root string, document *source.Document) (bool, error) {
	if selected.Kind != "worktree" {
		selected.Kind = "file"
	}
	selected.Path = document.Path
	scan, err := service.scan(ctx, selected, root)
	if err != nil {
		return false, err
	}
	defer scan.Close()
	entry, err := scan.Next(func(path string, stat source.Stat) (bool, error) {
		return path == document.Path && stat == document.Stat, nil
	}, func([]source.Watch) {})
	if err != nil || entry == nil {
		return false, err
	}
	return entry.Cached && entry.Path == document.Path, nil
}

// parts chunks one positioned text and embeds its chunks when a model is
// installed. Without a model every character is one token.
func (service *Service) parts(ctx context.Context, model string, choice protocol.ModelsLocalChoice, part source.Part) ([]Part, error) {
	var offsets [][2]int
	var err error
	if model != "" {
		if offsets, err = service.options.Offsets(ctx, model, choice, part.Text); err != nil {
			return nil, err
		}
	}
	if len(offsets) == 0 {
		for start := 0; start < len(part.Text); {
			_, size := utf8.DecodeRuneInString(part.Text[start:])
			offsets = append(offsets, [2]int{start, start + size})
			start += size
		}
	}
	chunks, err := knowledge.Split(part.Text, offsets)
	if err != nil {
		return nil, err
	}
	first, _, ranged := lines(part.Position)
	parts := make([]Part, len(chunks))
	for position, chunk := range chunks {
		parts[position] = Part{Title: part.Title, Text: strings.Clone(part.Text[chunk.Start:chunk.End]), Position: part.Position}
		if chunk.Heading != nil {
			parts[position].Title = *chunk.Heading
		}
		if ranged {
			parts[position].Position = protocol.KnowledgePosition{Kind: "lines", Data: protocol.KnowledgePositionLinesPayload{
				Start: saturating(first, chunk.FirstLine-1), End: saturating(first, chunk.LastLine-1),
			}}
		}
	}
	if model == "" {
		return parts, nil
	}
	for start := 0; start < len(parts); start += batch {
		if err := indexingCancelled(ctx); err != nil {
			return nil, err
		}
		group := parts[start:min(start+batch, len(parts))]
		texts := make([]string, len(group))
		for position, part := range group {
			texts[position] = part.Text
		}
		vectors, err := service.options.Encode(ctx, model, choice, texts, false)
		if err != nil {
			return nil, err
		}
		if len(vectors) != len(group) {
			return nil, newFault(protocol.ErrorCodeInternal, "embedding batch count differs")
		}
		for position, vector := range vectors {
			// An input of only unknown tokens has no semantic direction;
			// its keyword text is kept.
			if nonzero(vector) {
				quantized, err := knowledge.Quantize(vector)
				if err != nil {
					return nil, err
				}
				group[position].Vector = &quantized
			}
		}
	}
	return parts, nil
}

func saturating(left, right uint32) uint32 {
	if left > ^uint32(0)-right {
		return ^uint32(0)
	}
	return left + right
}
