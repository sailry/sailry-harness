//go:build !sailry_no_code_index

package codeindex

import (
	containerlist "container/list"
	"context"
	"encoding/json"
	"slices"
	"strings"
	"sync"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	liveCacheFiles  = 4096
	liveCacheBytes  = 8 << 20
	liveFileBytes   = 512 << 10
	liveMatchers    = 4
	livePatternSize = 4096
	livePages       = 4
	livePageBytes   = 128 << 10
	livePageKeySize = 16 << 10
)

type liveScope struct {
	id   protocol.WorktreeID
	root string
}

type liveFileKey struct {
	liveScope
	path    string
	content parseContent
}

type liveMatch struct {
	pattern string
	matched bool
}

// A proof covers one file version and only the exact lexical tests already
// performed on it. File discovery and access checks never use this cache.
type liveProof struct {
	stamp core.FileStamp
	match [liveMatchers]liveMatch
	file  *Parsed
	bytes int
}

func (p *liveProof) selected(pattern string) (*Parsed, bool) {
	if pattern == "" || p.file != nil && p.file.Status == StatusTooLarge {
		return p.file, p.file != nil
	}
	for _, match := range p.match {
		if match.pattern == pattern {
			if !match.matched {
				return nil, true
			}
			return p.file, p.file != nil
		}
	}
	return nil, false
}

func (p *liveProof) remember(pattern string, matched bool) {
	if pattern == "" || len(pattern) > livePatternSize || p.stamp == (core.FileStamp{}) {
		return
	}
	if !matched || p.file != nil {
		copy(p.match[1:], p.match[:liveMatchers-1])
		p.match[0] = liveMatch{pattern, matched}
	}
}

type liveFileEntry struct {
	key   liveFileKey
	proof liveProof
	bytes int
}

type livePageKey struct {
	liveScope
	content parseContent
	query   string
	files   string
}

type livePageEntry struct {
	key  livePageKey
	body []byte
}

type liveCache struct {
	mu     sync.Mutex
	files  map[liveFileKey]*containerlist.Element
	recent containerlist.List
	bytes  int
	pages  [livePages]livePageEntry
}

func (c *liveCache) proof(key liveFileKey) liveProof {
	c.mu.Lock()
	defer c.mu.Unlock()
	if found := c.files[key]; found != nil {
		c.recent.MoveToFront(found)
		return found.Value.(liveFileEntry).proof
	}
	return liveProof{}
}

func (c *liveCache) saveProof(key liveFileKey, proof liveProof) {
	// Charge the entry, map/list overhead, strings and detached rows. Shared
	// paths and patterns are charged per entry, rather than discounting them.
	size := 512 + len(key.root) + len(key.path) + proof.bytes
	for _, match := range proof.match {
		size += len(match.pattern)
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if found := c.files[key]; found != nil {
		c.evict(found)
	}
	if size > liveCacheBytes || proof.file == nil && proof.match[0].pattern == "" {
		return
	}
	for c.recent.Len() >= liveCacheFiles || c.bytes+size > liveCacheBytes {
		c.evict(c.recent.Back())
	}
	if c.files == nil {
		c.files = make(map[liveFileKey]*containerlist.Element)
	}
	key.path = strings.Clone(key.path)
	c.files[key] = c.recent.PushFront(liveFileEntry{key: key, proof: proof, bytes: size})
	c.bytes += size
}

func (c *liveCache) evict(element *containerlist.Element) {
	entry := c.recent.Remove(element).(liveFileEntry)
	delete(c.files, entry.key)
	c.bytes -= entry.bytes
}

func (c *liveCache) forget(id protocol.WorktreeID) {
	c.mu.Lock()
	defer c.mu.Unlock()
	for key, element := range c.files {
		if key.id == id {
			c.evict(element)
		}
	}
	kept := 0
	for _, entry := range c.pages {
		if entry.body != nil && entry.key.id != id {
			c.pages[kept] = entry
			kept++
		}
	}
	clear(c.pages[kept:])
}

func (c *liveCache) clear() {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.files = nil
	c.recent.Init()
	c.bytes = 0
	clear(c.pages[:])
}

func (c *liveCache) page(key livePageKey) []byte {
	c.mu.Lock()
	defer c.mu.Unlock()
	for i, entry := range c.pages {
		if entry.body != nil && entry.key == key {
			copy(c.pages[1:i+1], c.pages[:i])
			c.pages[0] = entry
			return entry.body
		}
	}
	return nil
}

func (c *liveCache) savePage(key livePageKey, body []byte) {
	if cap(body) > livePageBytes {
		return
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	last := len(c.pages) - 1
	for i, entry := range c.pages {
		if entry.body == nil || entry.key == key {
			last = i
			break
		}
	}
	copy(c.pages[1:last+1], c.pages[:last])
	key.files = strings.Clone(key.files)
	c.pages[0] = livePageEntry{key, body}
}

// Own just the index rows: names, signatures and import keys may otherwise be
// substrings keeping the whole source buffer alive. Incomplete parses can
// vary with the parser budget and are not reused.
func ownLiveParse(file *Parsed) (*Parsed, int) {
	if file.Status != StatusTooLarge && (file.Partial || file.Truncated || file.Status != StatusParsed && file.Status != StatusUnsupported) {
		return nil, 0
	}
	size := 192 + len(file.Path) + len(file.Hash)
	for _, symbol := range file.Symbols {
		size += 144 + len(symbol.Name) + len(symbol.Kind)
		if symbol.Signature != nil {
			size += 16 + len(*symbol.Signature)
		}
	}
	for _, ref := range file.References {
		size += 80 + len(ref.Name) + len(ref.Kind)
		if ref.Scope != nil {
			size += 64 + len(ref.Scope.Name) + len(ref.Scope.Kind)
		}
	}
	for _, module := range file.Imports {
		size += 32 + len(module)
	}
	if size > liveFileBytes {
		return nil, 0
	}
	owned := *file
	owned.Path, owned.Hash = strings.Clone(file.Path), strings.Clone(file.Hash)
	owned.Symbols = slices.Clone(file.Symbols)
	for i := range owned.Symbols {
		symbol := &owned.Symbols[i]
		symbol.Path, symbol.FileRevision = owned.Path, owned.Hash
		symbol.Name, symbol.Kind = strings.Clone(symbol.Name), strings.Clone(symbol.Kind)
		if symbol.Signature != nil {
			symbol.Signature = new(strings.Clone(*symbol.Signature))
		}
	}
	owned.References = slices.Clone(file.References)
	scopes := map[*Scope]*Scope{}
	for i := range owned.References {
		ref := &owned.References[i]
		ref.Name, ref.Kind = strings.Clone(ref.Name), strings.Clone(ref.Kind)
		if ref.Scope != nil {
			detached := scopes[ref.Scope]
			if detached == nil {
				scope := *ref.Scope
				scope.Name, scope.Kind = strings.Clone(scope.Name), strings.Clone(scope.Kind)
				detached = &scope
				scopes[ref.Scope] = detached
			}
			ref.Scope = detached
		}
	}
	owned.Imports = slices.Clone(file.Imports)
	for i, module := range owned.Imports {
		owned.Imports[i] = strings.Clone(module)
	}
	return &owned, size
}

func (scan *liveFiles) pageKey(query any) (livePageKey, bool) {
	if scan.report.truncated {
		return livePageKey{}, false
	}
	// JSON preserves optional fields, but would merge different invalid UTF-8
	// input strings. Paths have already been validated by the caller.
	switch q := query.(type) {
	case *protocol.CodeIndexSearch:
		if !utf8.ValidString(q.Query) || !utf8.ValidString(deref(q.Name)) || !utf8.ValidString(deref(q.Language)) {
			return livePageKey{}, false
		}
	case *protocol.CodeIndexGraph:
		if !utf8.ValidString(deref(q.Name)) {
			return livePageKey{}, false
		}
	}
	request, err := json.Marshal(query)
	keyBytes := len(scan.scope.root) + len(request)
	if err != nil || keyBytes > livePageKeySize {
		return livePageKey{}, false
	}
	var selection strings.Builder
	for i := range scan.parsed {
		file := &scan.parsed[i]
		if file.Partial || file.Truncated || keyBytes+selection.Len()+len(file.Path)+len(file.Hash)+2 > livePageKeySize {
			return livePageKey{}, false
		}
		selection.WriteString(file.Path)
		selection.WriteByte(0)
		selection.WriteString(file.Hash)
		selection.WriteByte(0)
	}
	return livePageKey{scan.scope, scan.content, string(request), selection.String()}, true
}

// A cached page contains only the SQL result, not the walk report. Its inputs
// are the ordered, freshly selected paths and their content revisions.
func indexedLivePage[Q, P any](ctx context.Context, scan *liveFiles, query *Q, run func(*index, context.Context, *Q) (P, error)) (P, error) {
	var page P
	key, cacheable := scan.pageKey(query)
	if cacheable {
		if body := scan.service.live.page(key); body != nil && json.Unmarshal(body, &page) == nil {
			return page, failure(ctx.Err())
		}
	}
	index, err := scan.index(ctx)
	if err != nil {
		return page, err
	}
	defer index.close()
	page, err = run(index, ctx, query)
	if cacheable && err == nil && ctx.Err() == nil {
		if body, err := json.Marshal(page); err == nil {
			scan.service.live.savePage(key, body)
		}
	}
	return page, err
}
