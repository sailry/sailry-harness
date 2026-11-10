package codeindex

import (
	"cmp"
	"context"
	"database/sql"
	"encoding/json"
	"math"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Callers, callees, impact and a ranked map over name-resolved references.
// Ideas follow Aider's repository map and name-based tag graphs. Results are
// heuristic and carry the confidence of each match.

const (
	// chunk is the number of parameters in one IN list.
	chunk = 256
	// maxCandidates bounds the definitions kept per name.
	maxCandidates = 1024
	usageColumns  = "name,kind,start_line,start_column,start_byte,receiver,scope,scope_kind,scope_line,scope_end_line"
)

// candidate is an indexed declaration a reference may resolve to.
type candidate struct {
	path, name, kind   string
	startLine, endLine uint32
	start, end         uint32
	depth              uint16
	signature          *string
}

// key is the identity shared by indexed declarations and reference scopes.
type key struct {
	path, name string
	line       uint32
}

func (k key) compare(other key) int {
	return cmp.Or(cmp.Compare(k.path, other.path), cmp.Compare(k.name, other.name), cmp.Compare(k.line, other.line))
}

func (c *candidate) key() key { return key{c.path, c.name, c.startLine} }

func (c *candidate) definition() protocol.CodeIndexGraphDefinition {
	return protocol.CodeIndexGraphDefinition{Path: c.path, Name: c.name, Kind: c.kind, StartLine: c.startLine, EndLine: c.endLine}
}

// definition reports the kinds a reference can resolve to.
func definition(kind string) bool {
	switch kind {
	case "variable", "data", "impl", "field", "property", "parameter", "import", "label":
		return false
	}
	return true
}

// owner reports the kinds whose members share a lexical owner.
func owner(kind string) bool {
	switch kind {
	case "class", "struct", "impl", "trait", "interface", "enum", "module", "namespace", "object", "protocol", "extension", "mixin", "union", "record":
		return true
	}
	return false
}

// reader runs the bounded reads of one graph query in its transaction.
type reader struct {
	ctx context.Context
	db  *sql.Conn
	// limited records that a bounded read omitted rows.
	limited bool
}

func (r *reader) candidates(statement string, values ...any) ([]candidate, error) {
	rows, err := r.db.QueryContext(r.ctx, statement, values...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var found []candidate
	for rows.Next() {
		var body string
		if err := rows.Scan(&body); err != nil {
			return nil, err
		}
		var symbol Symbol
		if err := json.Unmarshal([]byte(body), &symbol); err != nil {
			return nil, err
		}
		found = append(found, candidate{
			path: symbol.Path, name: symbol.Name, kind: symbol.Kind,
			startLine: symbol.Range.StartLine, endLine: symbol.Range.EndLine,
			start: symbol.Range.StartByte, end: symbol.Range.EndByte,
			depth: symbol.Depth, signature: symbol.Signature,
		})
	}
	return found, rows.Err()
}

func placeholders(values []string) (string, []any) {
	arguments := make([]any, len(values))
	for index, value := range values {
		arguments[index] = value
	}
	return strings.TrimSuffix(strings.Repeat("?,", len(values)), ","), arguments
}

// definitions returns the definitions with exactly these names, in path order.
func (r *reader) definitions(names []string) (map[string][]candidate, error) {
	found := map[string][]candidate{}
	seen := map[key]bool{}
	for part := range slices.Chunk(names, chunk) {
		if err := r.ctx.Err(); err != nil {
			return nil, err
		}
		list, values := placeholders(part)
		candidates, err := r.candidates("SELECT body FROM symbols WHERE name IN ("+list+") ORDER BY path,id", values...)
		if err != nil {
			return nil, err
		}
		for _, candidate := range candidates {
			if !definition(candidate.kind) {
				continue
			}
			if len(found[candidate.name]) >= maxCandidates {
				r.limited = true
				continue
			}
			if !seen[candidate.key()] {
				seen[candidate.key()] = true
				found[candidate.name] = append(found[candidate.name], candidate)
			}
		}
	}
	return found, nil
}

// outline returns every indexed declaration in one file, in source order.
func (r *reader) outline(path string) ([]candidate, error) {
	if err := r.ctx.Err(); err != nil {
		return nil, err
	}
	return r.candidates("SELECT body FROM symbols WHERE path=?1 ORDER BY id LIMIT ?2", path, maxSymbols)
}

// within returns definitions in files equal to or below the given paths.
func (r *reader) within(paths []string, limit int) ([]candidate, error) {
	var found []candidate
	seen := map[key]bool{}
	for _, path := range paths {
		if err := r.ctx.Err(); err != nil {
			return nil, err
		}
		candidates, err := r.candidates("SELECT body FROM symbols WHERE path=?1 OR substr(path,1,length(?1)+1)=?1||'/' ORDER BY path,id", path)
		if err != nil {
			return nil, err
		}
		for _, candidate := range candidates {
			if !definition(candidate.kind) || seen[candidate.key()] {
				continue
			}
			seen[candidate.key()] = true
			if len(found) == limit {
				r.limited = true
				return found, nil
			}
			found = append(found, candidate)
		}
	}
	return found, nil
}

// located is a reference with the file it occurs in.
type located struct {
	path  string
	usage Usage
}

func (r *reader) usages(located_ bool, statement string, values ...any) ([]located, error) {
	rows, err := r.db.QueryContext(r.ctx, statement, values...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var found []located
	for rows.Next() {
		var item located
		var scope, kind *string
		var start, end *uint32
		targets := []any{&item.usage.Name, &item.usage.Kind, &item.usage.Line, &item.usage.Column, &item.usage.Start, &item.usage.Receiver, &scope, &kind, &start, &end}
		if located_ {
			targets = append([]any{&item.path}, targets...)
		}
		if err := rows.Scan(targets...); err != nil {
			return nil, err
		}
		if scope != nil {
			item.usage.Scope = &Scope{Name: *scope}
			if kind != nil {
				item.usage.Scope.Kind = *kind
			}
			if start != nil {
				item.usage.Scope.StartLine = *start
			}
			if end != nil {
				item.usage.Scope.EndLine = *end
			}
		}
		found = append(found, item)
	}
	return found, rows.Err()
}

// references returns references to any of the names, by file and position.
func (r *reader) references(names []string, limit int) ([]located, error) {
	var found []located
	for part := range slices.Chunk(names, chunk) {
		if err := r.ctx.Err(); err != nil {
			return nil, err
		}
		remaining := limit - len(found)
		if remaining <= 0 {
			r.limited = true
			break
		}
		list, values := placeholders(part)
		rows, err := r.usages(true, "SELECT path,"+usageColumns+" FROM refs WHERE name IN ("+list+") ORDER BY path,start_byte LIMIT ?", append(values, remaining+1)...)
		if err != nil {
			return nil, err
		}
		if len(rows) > remaining {
			r.limited = true
			rows = rows[:remaining]
		}
		found = append(found, rows...)
	}
	return found, nil
}

// inside returns references starting inside a byte range of one file.
func (r *reader) inside(path string, start, end uint32, limit int) ([]located, error) {
	if err := r.ctx.Err(); err != nil {
		return nil, err
	}
	rows, err := r.usages(false, "SELECT "+usageColumns+" FROM refs WHERE path=?1 AND start_byte>=?2 AND start_byte<?3 ORDER BY start_byte LIMIT ?4", path, start, end, limit+1)
	if err != nil {
		return nil, err
	}
	if len(rows) > limit {
		r.limited = true
		rows = rows[:limit]
	}
	for index := range rows {
		rows[index].path = path
	}
	return rows, nil
}

func (r *reader) modulesOf(limit int, statement string, values ...any) (map[string][]string, error) {
	rows, err := r.db.QueryContext(r.ctx, statement, values...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	found := map[string][]string{}
	for count := 0; rows.Next(); count++ {
		if count == limit {
			r.limited = true
			break
		}
		var path, module string
		if err := rows.Scan(&path, &module); err != nil {
			return nil, err
		}
		found[path] = append(found[path], module)
	}
	return found, rows.Err()
}

// imports returns the module keys imported by each of the files.
func (r *reader) imports(paths []string) (map[string][]string, error) {
	found := map[string][]string{}
	for part := range slices.Chunk(paths, chunk) {
		if err := r.ctx.Err(); err != nil {
			return nil, err
		}
		list, values := placeholders(part)
		modules, err := r.modulesOf(math.MaxInt, "SELECT path,module FROM imports WHERE path IN ("+list+")", values...)
		if err != nil {
			return nil, err
		}
		for path, keys := range modules {
			found[path] = append(found[path], keys...)
		}
	}
	return found, nil
}

// modules returns the imports of every file, bounded.
func (r *reader) modules(limit int) (map[string][]string, error) {
	if err := r.ctx.Err(); err != nil {
		return nil, err
	}
	return r.modulesOf(limit, "SELECT path,module FROM imports ORDER BY path LIMIT ?1", limit+1)
}

// total is the reference count of one name in one referencing file.
type total struct {
	path, name string
	count      uint32
}

func counted(value int64) uint32 { return uint32(min(value, math.MaxUint32)) }

// totals returns reference counts per referencing file and name, bounded.
func (r *reader) totals(limit int) ([]total, error) {
	if err := r.ctx.Err(); err != nil {
		return nil, err
	}
	rows, err := r.db.QueryContext(r.ctx, "SELECT path,name,count(*) FROM refs GROUP BY path,name ORDER BY path,name LIMIT ?1", limit+1)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var found []total
	for rows.Next() {
		if len(found) == limit {
			r.limited = true
			break
		}
		var item total
		var count int64
		if err := rows.Scan(&item.path, &item.name, &count); err != nil {
			return nil, err
		}
		item.count = counted(count)
		found = append(found, item)
	}
	return found, rows.Err()
}

// counts returns the indexed declarations per file.
func (r *reader) counts() (map[string]uint32, error) {
	if err := r.ctx.Err(); err != nil {
		return nil, err
	}
	rows, err := r.db.QueryContext(r.ctx, "SELECT path,count(*) FROM symbols GROUP BY path")
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	found := map[string]uint32{}
	for rows.Next() {
		var path string
		var count int64
		if err := rows.Scan(&path, &count); err != nil {
			return nil, err
		}
		found[path] = counted(count)
	}
	return found, rows.Err()
}

// Conservative name resolution. A reference prefers definitions in its own
// file, then in files it imports, then in its directory, then anywhere in the
// worktree. Several definitions in the chosen tier remain ambiguous.

type confidence = protocol.CodeIndexGraphConfidence

// tier orders confidences from the strongest resolution to the weakest.
func tier(value confidence) int {
	switch value {
	case protocol.CodeIndexGraphConfidenceLocal:
		return 0
	case protocol.CodeIndexGraphConfidenceImported:
		return 1
	case protocol.CodeIndexGraphConfidenceDirectory:
		return 2
	}
	return 3
}

type site struct {
	path     string
	start    uint32
	receiver bool
	// imports are the module keys imported by the referencing file.
	imports []string
}

func choose(candidates []candidate, keep func(*candidate) bool) []*candidate {
	var chosen []*candidate
	for index := range candidates {
		if keep(&candidates[index]) {
			chosen = append(chosen, &candidates[index])
		}
	}
	return chosen
}

// resolve returns the strongest tier with candidates, or nothing when no
// definition has the name.
func resolve(site *site, candidates []candidate) (confidence, []*candidate) {
	if local := choose(candidates, func(c *candidate) bool { return c.path == site.path }); len(local) != 0 {
		return protocol.CodeIndexGraphConfidenceLocal, local
	}
	if imported := choose(candidates, func(c *candidate) bool {
		return slices.ContainsFunc(site.imports, func(key string) bool { return importsFile(key, c.path) })
	}); len(imported) != 0 {
		return protocol.CodeIndexGraphConfidenceImported, imported
	}
	directory := parent(site.path)
	if near := choose(candidates, func(c *candidate) bool { return parent(c.path) == directory }); len(near) != 0 {
		return protocol.CodeIndexGraphConfidenceDirectory, near
	}
	return protocol.CodeIndexGraphConfidenceWorkspace, choose(candidates, func(*candidate) bool { return true })
}

// narrow keeps the same-file definitions sharing the reference's lexical
// owner, such as a class or impl block. A self or this reference requires
// that owner; other references also accept top-level definitions.
func narrow(site *site, targets []*candidate, outline []candidate) []*candidate {
	if len(targets) < 2 {
		return targets
	}
	owners := choose(outline, func(c *candidate) bool { return owner(c.kind) })
	home, housed := around(owners, site.start, site.start+1)
	var selected []*candidate
	for _, target := range targets {
		found, ok := around(owners, target.start, target.end)
		if ok == housed && found == home || !site.receiver && !ok {
			selected = append(selected, target)
		}
	}
	if len(selected) == 0 {
		return targets
	}
	return selected
}

// importsFile reports whether an import key names this file, its directory
// or a package path ending in its directory.
func importsFile(key, path string) bool {
	directory, file := "", path
	if index := strings.LastIndexByte(path, '/'); index >= 0 {
		directory, file = path[:index], path[index+1:]
	}
	stem := file
	if index := strings.LastIndexByte(file, '.'); index >= 0 {
		stem = file[:index]
	}
	named := key == stem
	if index := strings.LastIndexByte(key, '/'); index >= 0 {
		named = key[index+1:] == stem && ends(directory, key[:index])
	}
	return named || ends(directory, key) || directory != "" && ends(key, directory)
}

func parent(path string) string {
	if index := strings.LastIndexByte(path, '/'); index >= 0 {
		return path[:index]
	}
	return ""
}

func ends(path, suffix string) bool {
	rest, ok := strings.CutSuffix(path, suffix)
	return suffix != "" && ok && (rest == "" || strings.HasSuffix(rest, "/"))
}

// around names the innermost owner strictly containing a byte range.
func around(owners []*candidate, start, end uint32) (string, bool) {
	var best *candidate
	for _, item := range owners {
		if item.start > start || end > item.end || item.start == start && item.end == end {
			continue
		}
		if best == nil || item.start > best.start || item.start == best.start && item.end <= best.end {
			best = item
		}
	}
	if best == nil {
		return "", false
	}
	return best.name, true
}

// Personalized PageRank over weighted file references, after Aider's
// repository map. Fixed iteration order keeps equal inputs bit-identical.
const (
	damping   = 0.85
	rounds    = 100
	tolerance = 1e-10
)

type edge struct {
	from, to int
	weight   float64
}

// pagerank returns ranks for the nodes summing to one. Edges carry positive
// weights; focus restricts teleports, or all nodes when empty.
func pagerank(nodes int, edges []edge, focus []int) []float64 {
	if nodes == 0 {
		return nil
	}
	teleport := make([]float64, nodes)
	focus = slices.DeleteFunc(slices.Clone(focus), func(node int) bool { return node >= nodes })
	for index := range teleport {
		if len(focus) == 0 {
			teleport[index] = 1 / float64(nodes)
		}
	}
	for _, node := range focus {
		teleport[node] += 1 / float64(len(focus))
	}
	outgoing := make([]float64, nodes)
	for _, edge := range edges {
		outgoing[edge.from] += edge.weight
	}
	rank := slices.Clone(teleport)
	for range rounds {
		dangling := 0.0
		for index, value := range rank {
			if outgoing[index] <= 0 {
				dangling += value
			}
		}
		next := make([]float64, nodes)
		for index, share := range teleport {
			next[index] = (1 - damping + damping*dangling) * share
		}
		for _, edge := range edges {
			next[edge.to] += damping * rank[edge.from] * edge.weight / outgoing[edge.from]
		}
		delta := 0.0
		for index := range next {
			delta += math.Abs(next[index] - rank[index])
		}
		rank = next
		if delta < tolerance {
			break
		}
	}
	return rank
}
