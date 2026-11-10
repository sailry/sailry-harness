package codeindex

import (
	"cmp"
	"maps"
	"math"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Graph views over resolved references. Every read is bounded; reaching a
// bound marks the page partial and truncated instead of failing it.
const (
	// maxTargets bounds the definitions selected for callers, callees and context.
	maxTargets = 20
	// maxSeeds bounds the definitions seeding impact.
	maxSeeds = 512
	// maxScan bounds the reference rows read per lookup or hop.
	maxScan = 20_000
	// maxFrontier bounds the definitions carried into the next impact hop.
	maxFrontier = 512
	maxSamples  = 3
	maxAffected = 10
	maxRanked   = 8
	// maxMapRows bounds the grouped file and name reference counts read for the map.
	maxMapRows    = 200_000
	maxMapImports = 200_000
)

// strength is how a definition was reached: the weakest confidence on the
// path and whether any step was ambiguous.
type strength struct {
	confidence confidence
	ambiguous  bool
}

func truth(value bool) int {
	if value {
		return 1
	}
	return 0
}

func (s strength) compare(other strength) int {
	return cmp.Or(cmp.Compare(tier(s.confidence), tier(other.confidence)), cmp.Compare(truth(s.ambiguous), truth(other.ambiguous)))
}

func run(reader *reader, query *protocol.CodeIndexGraph) (protocol.CodeIndexGraphGraphPage, error) {
	page := protocol.DefaultCodeIndexGraphGraphPage()
	page.View = query.View
	limit := int(query.Limit)
	if query.View == protocol.CodeIndexGraphViewMap {
		return page, ranking(reader, query, &page)
	}
	bound := maxTargets
	if query.View == protocol.CodeIndexGraphViewImpact {
		bound = maxSeeds
	}
	targets, err := selected(reader, query, bound)
	if err != nil {
		return page, err
	}
	for index := range targets {
		page.Targets = append(page.Targets, targets[index].definition())
	}
	outlines := outlines{}
	switch query.View {
	case protocol.CodeIndexGraphViewCallers:
		found, err := callers(reader, targets, outlines)
		if err != nil {
			return page, err
		}
		page.Callers = paged(found, query, &page)
	case protocol.CodeIndexGraphViewCallees:
		found, err := callees(reader, targets, outlines)
		if err != nil {
			return page, err
		}
		page.Callees = paged(found, query, &page)
	case protocol.CodeIndexGraphViewImpact:
		found, err := impact(reader, targets, query.Depth, outlines)
		if err != nil {
			return page, err
		}
		page.Impact = paged(found, query, &page)
	default:
		incoming, err := callers(reader, targets, outlines)
		if err != nil {
			return page, err
		}
		outgoing, err := callees(reader, targets, outlines)
		if err != nil {
			return page, err
		}
		affected, err := impact(reader, targets, query.Depth, outlines)
		if err != nil {
			return page, err
		}
		page.Total = uint64(len(affected))
		page.Truncated = page.Truncated || len(incoming) > limit || len(outgoing) > limit || len(affected) > limit
		page.Callers = incoming[:min(len(incoming), limit)]
		page.Callees = outgoing[:min(len(outgoing), limit)]
		page.Impact = affected[:min(len(affected), limit)]
	}
	return page, nil
}

func paged[T any](items []T, query *protocol.CodeIndexGraph, page *protocol.CodeIndexGraphGraphPage) []T {
	page.Total = uint64(len(items))
	end := query.Offset + query.Limit
	if end < uint64(len(items)) {
		page.NextOffset = &end
		page.Truncated = true
	}
	start := min(query.Offset, uint64(len(items)))
	return slices.Clone(items[start:min(end, uint64(len(items)))])
}

func within(path, prefix string) bool {
	rest, ok := strings.CutPrefix(path, prefix)
	return ok && (rest == "" || strings.HasPrefix(rest, "/"))
}

// selected returns the definitions chosen by name, path and line.
func selected(reader *reader, query *protocol.CodeIndexGraph, limit int) ([]candidate, error) {
	var found []candidate
	switch {
	case query.Name != nil:
		named, err := reader.definitions([]string{*query.Name})
		if err != nil {
			return nil, err
		}
		found = named[*query.Name]
		if len(query.Paths) != 0 {
			found = slices.DeleteFunc(found, func(c candidate) bool {
				return !slices.ContainsFunc(query.Paths, func(path string) bool { return within(c.path, path) })
			})
		}
	case query.Line != nil:
		outline, err := reader.outline(query.Paths[0])
		if err != nil {
			return nil, err
		}
		found = slices.DeleteFunc(outline, func(c candidate) bool { return !definition(c.kind) })
	default:
		var err error
		if found, err = reader.within(query.Paths, limit+1); err != nil {
			return nil, err
		}
	}
	if query.Line != nil {
		line := *query.Line
		var innermost *candidate
		for index := range found {
			item := &found[index]
			if item.startLine > line || line > item.endLine {
				continue
			}
			if innermost == nil || item.startLine > innermost.startLine || item.startLine == innermost.startLine && item.endLine <= innermost.endLine {
				innermost = item
			}
		}
		if innermost == nil {
			found = nil
		} else {
			found = []candidate{*innermost}
		}
	}
	if len(found) > limit {
		found = found[:limit]
		reader.limited = true
	}
	return found, nil
}

func names[T any](items []T, name func(*T) string) []string {
	unique := map[string]struct{}{}
	for index := range items {
		unique[name(&items[index])] = struct{}{}
	}
	return slices.Sorted(maps.Keys(unique))
}

// outlines keeps the declarations of files already read by this query.
type outlines map[string][]candidate

// resolved resolves one reference, narrowing same-file ambiguity lexically.
// It returns no candidates when no definition has the name.
func resolved(reader *reader, item *located, candidates map[string][]candidate, imports map[string][]string, outlines outlines) (confidence, []*candidate, error) {
	all := candidates[item.usage.Name]
	if len(all) == 0 {
		return "", nil, nil
	}
	site := site{path: item.path, start: item.usage.Start, receiver: item.usage.Receiver, imports: imports[item.path]}
	confidence, chosen := resolve(&site, all)
	if confidence != protocol.CodeIndexGraphConfidenceLocal || len(chosen) < 2 {
		return confidence, chosen, nil
	}
	outline, ok := outlines[item.path]
	if !ok {
		var err error
		if outline, err = reader.outline(item.path); err != nil {
			return "", nil, err
		}
		outlines[item.path] = outline
	}
	return confidence, narrow(&site, chosen, outline), nil
}

func reference(item *located, confidence *confidence, chosen, shown []*candidate) protocol.CodeIndexGraphReference {
	found := protocol.CodeIndexGraphReference{
		Path: item.path, Name: item.usage.Name, Kind: item.usage.Kind, Line: item.usage.Line, Column: item.usage.Column, Count: 1,
		Targets: []protocol.CodeIndexGraphDefinition{}, Candidates: uint64(len(chosen)), Confidence: confidence, Ambiguous: len(chosen) > 1,
	}
	if scope := item.usage.Scope; scope != nil {
		found.Scope = &protocol.CodeIndexGraphDefinition{Path: item.path, Name: scope.Name, Kind: scope.Kind, StartLine: scope.StartLine, EndLine: scope.EndLine}
	}
	for _, candidate := range shown[:min(len(shown), maxSamples)] {
		found.Targets = append(found.Targets, candidate.definition())
	}
	return found
}

// place groups the references made from one scope to one name.
type place struct {
	path   string
	scoped bool
	scope  string
	line   uint32
	name   string
	kind   string
}

func (p place) compare(other place) int {
	return cmp.Or(cmp.Compare(p.path, other.path), cmp.Compare(truth(p.scoped), truth(other.scoped)), cmp.Compare(p.scope, other.scope),
		cmp.Compare(p.line, other.line), cmp.Compare(p.name, other.name), cmp.Compare(p.kind, other.kind))
}

// grouped counts repeated references per place and returns them in place order.
type grouped struct {
	order []place
	found map[place]*protocol.CodeIndexGraphReference
}

func (g *grouped) add(at place, build func() protocol.CodeIndexGraphReference) {
	if known := g.found[at]; known != nil {
		known.Count = uint32(min(uint64(known.Count)+1, math.MaxUint32))
		return
	}
	value := build()
	g.found[at] = &value
	g.order = append(g.order, at)
}

func (g *grouped) values() []protocol.CodeIndexGraphReference {
	slices.SortFunc(g.order, place.compare)
	found := make([]protocol.CodeIndexGraphReference, len(g.order))
	for index, at := range g.order {
		found[index] = *g.found[at]
	}
	return found
}

// certainty orders unresolved references before resolved ones, then by tier.
func certainty(value *confidence) int {
	if value == nil {
		return -1
	}
	return tier(*value)
}

// priority puts calls first, then other reference kinds.
func priority(kind string) int {
	switch kind {
	case "call", "send":
		return 0
	case "class":
		return 1
	case "implementation", "interface":
		return 2
	case "type":
		return 3
	case "import":
		return 5
	}
	return 4
}

// callers returns the references resolving to any target, grouped by place.
func callers(reader *reader, targets []candidate, outlines outlines) ([]protocol.CodeIndexGraphReference, error) {
	wanted := names(targets, func(c *candidate) string { return c.name })
	if len(wanted) == 0 {
		return []protocol.CodeIndexGraphReference{}, nil
	}
	usages, err := reader.references(wanted, maxScan)
	if err != nil {
		return nil, err
	}
	candidates, err := reader.definitions(wanted)
	if err != nil {
		return nil, err
	}
	imports, err := reader.imports(names(usages, func(item *located) string { return item.path }))
	if err != nil {
		return nil, err
	}
	keys := map[key]bool{}
	for index := range targets {
		keys[targets[index].key()] = true
	}
	groups := grouped{found: map[place]*protocol.CodeIndexGraphReference{}}
	for index := range usages {
		item := &usages[index]
		if err := reader.ctx.Err(); err != nil {
			return nil, err
		}
		confidence, chosen, err := resolved(reader, item, candidates, imports, outlines)
		if err != nil {
			return nil, err
		}
		var hits []*candidate
		for _, candidate := range chosen {
			if keys[candidate.key()] {
				hits = append(hits, candidate)
			}
		}
		if len(hits) == 0 {
			continue
		}
		at := place{path: item.path, name: item.usage.Name, kind: item.usage.Kind}
		if scope := item.usage.Scope; scope != nil {
			at.scoped, at.scope, at.line = true, scope.Name, scope.StartLine
		}
		groups.add(at, func() protocol.CodeIndexGraphReference { return reference(item, &confidence, chosen, hits) })
	}
	found := groups.values()
	// Calls first, then stronger and unambiguous resolutions, then position.
	slices.SortStableFunc(found, func(left, right protocol.CodeIndexGraphReference) int {
		return cmp.Or(cmp.Compare(priority(left.Kind), priority(right.Kind)), cmp.Compare(certainty(left.Confidence), certainty(right.Confidence)),
			cmp.Compare(truth(left.Ambiguous), truth(right.Ambiguous)), cmp.Compare(left.Path, right.Path), cmp.Compare(left.Line, right.Line))
	})
	return found, nil
}

// callees returns the references inside the targets with their resolved definitions.
func callees(reader *reader, targets []candidate, outlines outlines) ([]protocol.CodeIndexGraphReference, error) {
	var usages []located
	for index := range targets {
		inside, err := reader.inside(targets[index].path, targets[index].start, targets[index].end, maxScan)
		if err != nil {
			return nil, err
		}
		usages = append(usages, inside...)
	}
	candidates, err := reader.definitions(names(usages, func(item *located) string { return item.usage.Name }))
	if err != nil {
		return nil, err
	}
	imports, err := reader.imports(names(targets, func(c *candidate) string { return c.path }))
	if err != nil {
		return nil, err
	}
	groups := grouped{found: map[place]*protocol.CodeIndexGraphReference{}}
	for index := range usages {
		item := &usages[index]
		if err := reader.ctx.Err(); err != nil {
			return nil, err
		}
		found, chosen, err := resolved(reader, item, candidates, imports, outlines)
		if err != nil {
			return nil, err
		}
		var confidence *confidence
		if len(chosen) != 0 {
			confidence = &found
		}
		groups.add(place{path: item.path, name: item.usage.Name, kind: item.usage.Kind}, func() protocol.CodeIndexGraphReference {
			return reference(item, confidence, chosen, chosen)
		})
	}
	found := groups.values()
	slices.SortStableFunc(found, func(left, right protocol.CodeIndexGraphReference) int {
		return cmp.Or(cmp.Compare(truth(left.Confidence == nil), truth(right.Confidence == nil)), cmp.Compare(certainty(left.Confidence), certainty(right.Confidence)),
			cmp.Compare(truth(left.Ambiguous), truth(right.Ambiguous)), cmp.Compare(left.Line, right.Line))
	})
	return found, nil
}

// reach is the shortest hop and strongest resolution path to a file.
type reach struct {
	hop      uint8
	strength strength
}

func (r reach) compare(other reach) int {
	return cmp.Or(cmp.Compare(r.hop, other.hop), r.strength.compare(other.strength))
}

// impact returns the definitions reached by reverse references within depth
// hops. Each keeps its shortest hop count and, at that hop, the strongest
// resolution path.
func impact(reader *reader, seeds []candidate, depth uint8, outlines outlines) ([]protocol.CodeIndexGraphImpacted, error) {
	seeded := map[key]bool{}
	frontier := map[key]strength{}
	for index := range seeds {
		seeded[seeds[index].key()] = true
		frontier[seeds[index].key()] = strength{confidence: protocol.CodeIndexGraphConfidenceLocal}
	}
	reached := map[key]*protocol.CodeIndexGraphAffected{}
	files := map[string]reach{}
	for hop := uint8(1); hop <= depth && len(frontier) != 0; hop++ {
		wanted := names(slices.Collect(maps.Keys(frontier)), func(k *key) string { return k.name })
		usages, err := reader.references(wanted, maxScan)
		if err != nil {
			return nil, err
		}
		candidates, err := reader.definitions(wanted)
		if err != nil {
			return nil, err
		}
		imports, err := reader.imports(names(usages, func(item *located) string { return item.path }))
		if err != nil {
			return nil, err
		}
		next := map[key]strength{}
		for index := range usages {
			item := &usages[index]
			if err := reader.ctx.Err(); err != nil {
				return nil, err
			}
			confidence, chosen, err := resolved(reader, item, candidates, imports, outlines)
			if err != nil {
				return nil, err
			}
			var via *strength
			for _, candidate := range chosen {
				if found, ok := frontier[candidate.key()]; ok && (via == nil || found.compare(*via) < 0) {
					via = &found
				}
			}
			if via == nil {
				continue
			}
			reachedBy := strength{confidence: via.confidence, ambiguous: via.ambiguous || len(chosen) > 1}
			if tier(confidence) > tier(reachedBy.confidence) {
				reachedBy.confidence = confidence
			}
			scope := item.usage.Scope
			if scope == nil {
				current := reach{hop, reachedBy}
				if known, ok := files[item.path]; !ok || current.compare(known) < 0 {
					files[item.path] = current
				}
				continue
			}
			at := key{item.path, scope.Name, scope.StartLine}
			if seeded[at] {
				continue
			}
			known := reached[at]
			if known == nil {
				known = &protocol.CodeIndexGraphAffected{Name: scope.Name, Kind: scope.Kind, StartLine: scope.StartLine, EndLine: scope.EndLine, Depth: hop, Confidence: reachedBy.confidence, Ambiguous: reachedBy.ambiguous}
				reached[at] = known
			}
			if known.Depth != hop {
				continue
			}
			if reachedBy.compare(strength{known.Confidence, known.Ambiguous}) < 0 {
				known.Confidence, known.Ambiguous = reachedBy.confidence, reachedBy.ambiguous
			}
			if current, ok := next[at]; ok {
				if reachedBy.compare(current) < 0 {
					next[at] = reachedBy
				}
			} else if len(next) < maxFrontier {
				next[at] = reachedBy
			} else {
				reader.limited = true
			}
		}
		frontier = next
	}
	groups := map[string]*protocol.CodeIndexGraphImpacted{}
	merge := func(path string, hop uint8, reachedBy strength) *protocol.CodeIndexGraphImpacted {
		item := groups[path]
		if item == nil {
			item = &protocol.CodeIndexGraphImpacted{Path: path, Depth: hop, Confidence: reachedBy.confidence, Ambiguous: reachedBy.ambiguous, Symbols: []protocol.CodeIndexGraphAffected{}}
			groups[path] = item
		}
		item.Depth = min(item.Depth, hop)
		if reachedBy.compare(strength{item.Confidence, item.Ambiguous}) < 0 {
			item.Confidence, item.Ambiguous = reachedBy.confidence, reachedBy.ambiguous
		}
		return item
	}
	for _, at := range slices.SortedFunc(maps.Keys(reached), key.compare) {
		affected := reached[at]
		item := merge(at.path, affected.Depth, strength{affected.Confidence, affected.Ambiguous})
		item.Symbols = append(item.Symbols, *affected)
	}
	for path, found := range files {
		merge(path, found.hop, found.strength)
	}
	found := make([]protocol.CodeIndexGraphImpacted, 0, len(groups))
	for _, path := range slices.Sorted(maps.Keys(groups)) {
		item := groups[path]
		slices.SortStableFunc(item.Symbols, func(left, right protocol.CodeIndexGraphAffected) int {
			return cmp.Or(cmp.Compare(left.Depth, right.Depth), strength{left.Confidence, left.Ambiguous}.compare(strength{right.Confidence, right.Ambiguous}), cmp.Compare(left.StartLine, right.StartLine))
		})
		item.Truncated = len(item.Symbols) > maxAffected
		item.Symbols = item.Symbols[:min(len(item.Symbols), maxAffected)]
		found = append(found, *item)
	}
	slices.SortStableFunc(found, func(left, right protocol.CodeIndexGraphImpacted) int {
		return cmp.Or(cmp.Compare(left.Depth, right.Depth), strength{left.Confidence, left.Ambiguous}.compare(strength{right.Confidence, right.Ambiguous}), cmp.Compare(left.Path, right.Path))
	})
	return found, nil
}

// ranking fills the map view: files ranked by personalized PageRank over
// weighted references.
func ranking(reader *reader, query *protocol.CodeIndexGraph, page *protocol.CodeIndexGraphGraphPage) error {
	totals, err := reader.totals(maxMapRows)
	if err != nil {
		return err
	}
	counts, err := reader.counts()
	if err != nil {
		return err
	}
	candidates, err := reader.definitions(names(totals, func(item *total) string { return item.name }))
	if err != nil {
		return err
	}
	imports, err := reader.modules(maxMapImports)
	if err != nil {
		return err
	}
	nodes := map[string]int{}
	for path := range counts {
		nodes[path] = 0
	}
	for _, item := range totals {
		nodes[item.path] = 0
	}
	paths := slices.Sorted(maps.Keys(nodes))
	for index, path := range paths {
		nodes[path] = index
	}
	type link struct{ from, to int }
	type arrival struct {
		to   int
		name string
	}
	weights := map[link]float64{}
	inbound := map[arrival]float64{}
	for _, item := range totals {
		if err := reader.ctx.Err(); err != nil {
			return err
		}
		all := candidates[item.name]
		if len(all) == 0 {
			continue
		}
		_, chosen := resolve(&site{path: item.path, imports: imports[item.path]}, all)
		from := nodes[item.path]
		// Dampened like Aider: many uses of one name should not dominate.
		weight := math.Sqrt(float64(item.count)) / float64(len(chosen))
		for _, candidate := range chosen {
			to, ok := nodes[candidate.path]
			if candidate.path == item.path || !ok {
				continue
			}
			weights[link{from, to}] += weight
			inbound[arrival{to, candidate.name}] += weight
		}
	}
	edges := make([]edge, 0, len(weights))
	for link, weight := range weights {
		edges = append(edges, edge{link.from, link.to, weight})
	}
	slices.SortFunc(edges, func(left, right edge) int {
		return cmp.Or(cmp.Compare(left.from, right.from), cmp.Compare(left.to, right.to))
	})
	var focus, order []int
	for index, path := range paths {
		if slices.ContainsFunc(query.Paths, func(prefix string) bool { return within(path, prefix) }) {
			focus = append(focus, index)
		}
		if counts[path] > 0 {
			order = append(order, index)
		}
	}
	ranks := pagerank(len(paths), edges, focus)
	slices.SortStableFunc(order, func(left, right int) int {
		return cmp.Or(cmp.Compare(ranks[right], ranks[left]), cmp.Compare(paths[left], paths[right]))
	})
	top := 0.0
	if len(order) != 0 {
		top = ranks[order[0]]
	}
	type weighted struct {
		name   string
		weight float64
	}
	arrivals := map[int][]weighted{}
	for arrival, weight := range inbound {
		arrivals[arrival.to] = append(arrivals[arrival.to], weighted{arrival.name, weight})
	}
	for _, index := range paged(order, query, page) {
		path := paths[index]
		named := arrivals[index]
		slices.SortFunc(named, func(left, right weighted) int {
			return cmp.Or(cmp.Compare(right.weight, left.weight), cmp.Compare(left.name, right.name))
		})
		outline, err := reader.outline(path)
		if err != nil {
			return err
		}
		symbols := []protocol.CodeIndexGraphRanked{}
		for _, item := range named {
			if len(symbols) == maxRanked {
				break
			}
			if at := slices.IndexFunc(outline, func(c candidate) bool { return c.name == item.name && definition(c.kind) }); at >= 0 {
				symbols = append(symbols, ranked(&outline[at], uint32(math.Round(item.weight))))
			}
		}
		if len(symbols) == 0 {
			for at := range outline {
				if len(symbols) == maxRanked {
					break
				}
				if outline[at].depth == 0 && definition(outline[at].kind) {
					symbols = append(symbols, ranked(&outline[at], 0))
				}
			}
		}
		score := uint32(0)
		if top > 0 {
			score = uint32(math.Round(ranks[index] / top * 1000))
		}
		page.Map = append(page.Map, protocol.CodeIndexGraphEntry{Path: path, Score: score, Symbols: symbols})
	}
	return nil
}

func ranked(candidate *candidate, references uint32) protocol.CodeIndexGraphRanked {
	return protocol.CodeIndexGraphRanked{Name: candidate.name, Kind: candidate.kind, StartLine: candidate.startLine, EndLine: candidate.endLine, Signature: candidate.signature, References: references}
}
