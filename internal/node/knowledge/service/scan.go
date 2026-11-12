package service

import (
	"cmp"
	"context"
	"database/sql"
	"runtime"
	"slices"
	"sync"
	"sync/atomic"

	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	// vectorBudget bounds the vectors of all collections kept in memory. A
	// vector costs its dimension in bytes plus about 100 for its key and norm.
	vectorBudget = 256 << 20
	// keyBytes is the kept size of a chunk key: its text and string header.
	keyBytes = 96
	// streamBlock is the number of vectors a scan that reads the database holds
	// at a time.
	streamBlock = 16384
	// scanGrain is the run of vectors one worker takes at a time.
	scanGrain = 2048
	// scanWorkers bounds the goroutines of one scan.
	scanWorkers = 8
)

// block is a run of stored vectors in the layout the scan reads: the
// components of every vector in one slice, with the keys and norms beside it.
type block struct {
	dimension int
	keys      []string
	values    []int8
	norms     []float32
}

func (block *block) add(key string, bytes []byte) error {
	values, norm, err := knowledge.AppendVector(block.values, bytes, block.dimension)
	if err != nil {
		return err
	}
	block.values, block.keys, block.norms = values, append(block.keys, key), append(block.norms, norm)
	return nil
}

func (block *block) reset() {
	block.keys, block.values, block.norms = block.keys[:0], block.values[:0], block.norms[:0]
}

func (block *block) size() int64 {
	return int64(len(block.keys)) * int64(block.dimension+keyBytes+4)
}

// span is a run of vectors of one block.
type span struct {
	block    *block
	from, to int
}

// order ranks nearer first; equal scores order by key. Keys are unique, so
// it is total and every scan of the same vectors selects the same ones.
func order(left, right knowledge.Ranked) int {
	return cmp.Or(cmp.Compare(right.Score, left.Score), cmp.Compare(left.Key, right.Key))
}

// nearest keeps the best candidates of the vectors it is shown.
type nearest struct {
	kept  []knowledge.Ranked
	floor knowledge.Ranked
	full  bool
}

func (nearest *nearest) add(candidate knowledge.Ranked) {
	if nearest.full && order(candidate, nearest.floor) >= 0 {
		return
	}
	if nearest.kept = append(nearest.kept, candidate); len(nearest.kept) == 2*candidates {
		nearest.cut()
	}
}

func (nearest *nearest) cut() {
	slices.SortFunc(nearest.kept, order)
	if len(nearest.kept) >= candidates {
		nearest.kept = nearest.kept[:candidates]
		nearest.floor, nearest.full = nearest.kept[candidates-1], true
	}
}

func (nearest *nearest) scan(query knowledge.Direction, run span) {
	block := run.block
	for position := run.from; position < run.to; position++ {
		score := query.Cosine(block.values[position*block.dimension:(position+1)*block.dimension], block.norms[position])
		nearest.add(knowledge.Ranked{Key: block.keys[position], Score: score})
	}
}

// scan returns the nearest candidates of the spans, as scoring every vector
// and sorting all of them would. Workers take grains of the spans, each keeps
// its own nearest and the merge orders them again, so the result does not
// depend on the number of workers or on which worker scored a vector.
func scan(ctx context.Context, query knowledge.Direction, spans []span, workers int) ([]knowledge.Ranked, error) {
	var grains []span
	for _, run := range spans {
		for ; run.from < run.to; run.from += scanGrain {
			grains = append(grains, span{run.block, run.from, min(run.from+scanGrain, run.to)})
		}
	}
	workers = min(workers, len(grains))
	found := make([]nearest, max(workers, 1))
	var next atomic.Int64
	work := func(nearest *nearest) {
		for ctx.Err() == nil {
			position := int(next.Add(1)) - 1
			if position >= len(grains) {
				break
			}
			nearest.scan(query, grains[position])
		}
		nearest.cut()
	}
	if workers <= 1 {
		work(&found[0])
	} else {
		var group sync.WaitGroup
		for worker := range found {
			group.Go(func() { work(&found[worker]) })
		}
		group.Wait()
	}
	if err := searchCancelled(ctx); err != nil {
		return nil, err
	}
	var merged nearest
	for _, part := range found {
		merged.kept = append(merged.kept, part.kept...)
	}
	merged.cut()
	return merged.kept, nil
}

func scanParallelism() int { return min(runtime.GOMAXPROCS(0), scanWorkers) }

// stamp identifies the published state of one collection's chunks.
type stamp struct {
	version uint64
	writing int
}

// cached is the vectors of one collection as they were at a stamp. Sources
// index the runs of each source's vectors. A collection over the budget keeps
// no block and is read from the database by every query.
type cached struct {
	stamp   stamp
	block   *block
	sources map[int64][]span
	size    int64
	used    uint64
}

// vectors keeps the vectors of collections between queries. Every writer of
// a collection's chunks brackets its transaction with change, so a reader
// that saw the same idle stamp before its read transaction began and after
// it loaded knows that no write was committed in between.
type vectors struct {
	mu      sync.Mutex
	stamps  map[string]stamp
	entries map[string]*cached
	size    int64
	budget  int64
	clock   uint64
}

func newVectors() *vectors {
	return &vectors{stamps: map[string]stamp{}, entries: map[string]*cached{}, budget: vectorBudget}
}

// change marks a write to the chunks of one collection, or of any collection
// for an empty id, and returns what marks its end.
func (vectors *vectors) change(id string) func() {
	move := func(by int) {
		vectors.mu.Lock()
		defer vectors.mu.Unlock()
		current := vectors.stamps[id]
		vectors.stamps[id] = stamp{current.version + 1, current.writing + by}
		for known, entry := range vectors.entries {
			if id == "" || known == id {
				vectors.size -= entry.size
				delete(vectors.entries, known)
			}
		}
	}
	move(1)
	return func() { move(-1) }
}

func (vectors *vectors) stampLocked(id string) stamp {
	own, all := vectors.stamps[id], vectors.stamps[""]
	return stamp{own.version + all.version, own.writing + all.writing}
}

// snapshot returns the stamps a reader compares after its transaction began.
func (vectors *vectors) snapshot(bindings []Binding) map[string]stamp {
	vectors.mu.Lock()
	defer vectors.mu.Unlock()
	stamps := make(map[string]stamp, len(bindings))
	for _, binding := range bindings {
		id := binding.ID.String()
		stamps[id] = vectors.stampLocked(id)
	}
	return stamps
}

// idle reports that the collection is at the stamp and has no write under way.
func (vectors *vectors) idle(id string, before stamp) bool {
	vectors.mu.Lock()
	defer vectors.mu.Unlock()
	return before.writing == 0 && vectors.stampLocked(id) == before
}

func (vectors *vectors) get(id string, at stamp) *cached {
	vectors.mu.Lock()
	defer vectors.mu.Unlock()
	entry := vectors.entries[id]
	if entry == nil || entry.stamp != at || vectors.stampLocked(id) != at {
		return nil
	}
	vectors.clock++
	entry.used = vectors.clock
	return entry
}

// put keeps an entry loaded at a stamp that still holds, dropping the entries
// used longest ago until the budget holds it.
func (vectors *vectors) put(id string, entry *cached) {
	vectors.mu.Lock()
	defer vectors.mu.Unlock()
	if vectors.stampLocked(id) != entry.stamp {
		return
	}
	if previous := vectors.entries[id]; previous != nil {
		vectors.size -= previous.size
		delete(vectors.entries, id)
	}
	for vectors.size+entry.size > vectors.budget && len(vectors.entries) > 0 {
		oldest := ""
		for known, other := range vectors.entries {
			if oldest == "" || other.used < vectors.entries[oldest].used {
				oldest = known
			}
		}
		vectors.size -= vectors.entries[oldest].size
		delete(vectors.entries, oldest)
	}
	vectors.clock++
	entry.used = vectors.clock
	vectors.entries[id] = entry
	vectors.size += entry.size
}

// load reads the vectors of one collection inside the caller's read
// transaction. A collection that could exceed the budget is not read.
func load(ctx context.Context, db *sql.Conn, id string, dimension int, at stamp, budget int64) (*cached, error) {
	var chunks int64
	if err := db.QueryRowContext(ctx, "SELECT count(*) FROM chunks c JOIN sources s ON s.id=c.source WHERE s.collection=?1", id).Scan(&chunks); err != nil {
		return nil, err
	}
	entry := &cached{stamp: at}
	if chunks*int64(dimension+keyBytes+4) > budget {
		return entry, nil
	}
	rows, err := db.QueryContext(ctx, "SELECT c.source,c.key,c.vector FROM chunks c JOIN sources s ON s.id=c.source WHERE s.collection=?1 AND c.vector IS NOT NULL", id)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	entry.block, entry.sources = &block{dimension: dimension}, map[int64][]span{}
	var bytes sql.RawBytes
	for rows.Next() {
		if err := searchCancelled(ctx); err != nil {
			return nil, err
		}
		var source int64
		var key string
		if err := rows.Scan(&source, &key, &bytes); err != nil {
			return nil, err
		}
		position := len(entry.block.keys)
		if err := entry.block.add(key, bytes); err != nil {
			return nil, err
		}
		runs := entry.sources[source]
		if last := len(runs) - 1; last >= 0 && runs[last].to == position {
			runs[last].to++
		} else {
			runs = append(runs, span{entry.block, position, position + 1})
		}
		entry.sources[source] = runs
	}
	if err := rows.Close(); err != nil {
		return nil, err
	}
	if entry.size = entry.block.size() + int64(len(entry.sources))*64; entry.size > budget {
		return &cached{stamp: at}, nil
	}
	return entry, nil
}

// nearest returns the candidates of one query embedding among the vectors
// the grants and source filters admit. A collection's vectors are scanned in
// memory, loaded once per published state; a collection that is being written
// or exceeds the budget is read from the database in blocks.
func (index Index) nearest(ctx context.Context, db *sql.Conn, before map[string]stamp, embedding Embedding, filtered bool, grants []map[string]any, arguments []any) ([]knowledge.Ranked, error) {
	arguments = append(append([]any{encoded(grants)}, arguments...), vectorKey(embedding.Model, embedding.Identity), len(embedding.Values))
	const semantic = " AND json_extract(admitted.value,'$.semantic')=1 AND k.model=?7 AND k.dimension=?8"
	// One row per admitted source, or per admitted collection when no source
	// filter is set and all of its sources are admitted.
	statement := "SELECT k.id,NULL FROM collections k " + granted + " WHERE 1" + semantic
	if filtered {
		statement = "SELECT k.id,s.id FROM sources s JOIN collections k ON k.id=s.collection " + granted + " WHERE " + sourced + semantic
	}
	rows, err := db.QueryContext(ctx, statement, arguments...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var ids []string
	admitted := map[string][]sql.NullInt64{}
	for rows.Next() {
		if err := searchCancelled(ctx); err != nil {
			return nil, err
		}
		var id string
		var source sql.NullInt64
		if err := rows.Scan(&id, &source); err != nil {
			return nil, err
		}
		if _, seen := admitted[id]; !seen {
			ids = append(ids, id)
		}
		admitted[id] = append(admitted[id], source)
	}
	if err := rows.Close(); err != nil {
		return nil, err
	}
	var query knowledge.Direction
	prepared := false
	var found []knowledge.Ranked
	search := func(spans []span) error {
		if len(spans) == 0 {
			return nil
		}
		if !prepared {
			if query, err = knowledge.NewDirection(embedding.Values); err != nil {
				return err
			}
			prepared = true
		}
		ranked, err := scan(ctx, query, spans, scanParallelism())
		found = append(found, ranked...)
		return err
	}
	// The collections without kept vectors are read together.
	var streamed []map[string]any
	for _, id := range ids {
		at, known := before[id]
		var entry *cached
		if known && index.vectors.idle(id, at) {
			if entry = index.vectors.get(id, at); entry == nil {
				if entry, err = load(ctx, db, id, len(embedding.Values), at, index.vectors.budget); err != nil {
					return nil, err
				}
				index.vectors.put(id, entry)
			}
		}
		if entry == nil || entry.block == nil {
			for _, grant := range grants {
				if grant["id"].(protocol.KnowledgeID).String() == id {
					streamed = append(streamed, grant)
				}
			}
			continue
		}
		var spans []span
		for _, source := range admitted[id] {
			if !source.Valid {
				spans = append(spans, span{entry.block, 0, len(entry.block.keys)})
			} else {
				spans = append(spans, entry.sources[source.Int64]...)
			}
		}
		if err := search(spans); err != nil {
			return nil, err
		}
	}
	if len(streamed) != 0 {
		arguments[0] = encoded(streamed)
		rows, err := db.QueryContext(ctx, "SELECT c.key,c.vector FROM chunks c "+filter+semantic+" AND c.vector IS NOT NULL", arguments...)
		if err != nil {
			return nil, err
		}
		defer rows.Close()
		held := &block{dimension: len(embedding.Values)}
		var bytes sql.RawBytes
		for rows.Next() {
			if err := searchCancelled(ctx); err != nil {
				return nil, err
			}
			var key string
			if err := rows.Scan(&key, &bytes); err != nil {
				return nil, err
			}
			if err := held.add(key, bytes); err != nil {
				return nil, err
			}
			if len(held.keys) == streamBlock {
				if err := search([]span{{held, 0, streamBlock}}); err != nil {
					return nil, err
				}
				held.reset()
			}
		}
		if err := rows.Close(); err != nil {
			return nil, err
		}
		if err := search([]span{{held, 0, len(held.keys)}}); err != nil {
			return nil, err
		}
	}
	return found, nil
}
