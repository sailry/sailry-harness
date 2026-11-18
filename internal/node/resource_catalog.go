package node

import (
	"cmp"
	"context"
	"database/sql"
	"encoding/json"
	"os"
	"runtime"
	"slices"
	"sync"
	"time"

	"lukechampine.com/blake3"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) resourceClients(ctx context.Context) map[protocol.NodeID]*client.Client {
	user, present := store.UserFromContext(ctx)
	if !present || !user.Local {
		return map[protocol.NodeID]*client.Client{}
	}
	return node.controllerClients()
}

func (node *Node) resourceCatalog(ctx context.Context) (protocol.ResourceCatalog, error) {
	catalog := protocol.ResourceCatalog{Entry: node.ID(), Sources: []protocol.ResourceSource{}}
	local := protocol.ResourceSource{Node: node.ID(), Route: protocol.RouteKey{Entry: node.ID(), Target: node.ID()}, Platform: runtime.GOOS}
	local.Name, _ = os.Hostname()
	var user protocol.User
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		if user, err = store.CurrentUser(ctx, db); err != nil {
			return err
		}
		owner := store.WithUser(ctx, user)
		if local.Projects, err = store.ReadProjects(owner, db); err != nil {
			return err
		}
		if local.Worktrees, err = store.ReadWorktrees(owner, db); err != nil {
			return err
		}
		local.Revision, err = store.EventCursor(owner, db)
		return err
	}); err != nil {
		return catalog, err
	}
	ctx = store.WithUser(ctx, user)
	local.Worktrees = resourceDirectoryWorktrees(local.Projects, local.Worktrees)
	output, err := node.readKnowledge(ctx, protocol.NewRequest(node.ID(), protocol.Command{Kind: "list_knowledge_collections"}))
	if err != nil {
		local.Fault = resourceFault(err)
	} else if values, valid := dispatchPayload[[]protocol.KnowledgeCollection](output.Data); valid {
		local.Knowledge = values
	}
	catalog.Sources = append(catalog.Sources, local)
	if !user.Local {
		return catalog, nil
	}
	cached, err := node.cachedResourceSources(ctx)
	if err != nil {
		return catalog, err
	}
	peers := node.resourceClients(ctx)
	names := map[protocol.NodeID]string{}
	allowed := map[protocol.NodeID]bool{}
	checkRelationships := false
	if node.network != nil {
		if relationships, err := node.network.Relationships(ctx); err == nil {
			checkRelationships = true
			for _, relationship := range relationships {
				if relationship.Direct && relationship.DirectDirection.AllowsOutbound() && !relationship.Removed {
					allowed[relationship.ID], names[relationship.ID] = true, relationship.Name
				}
			}
		}
	}
	for id := range peers {
		if _, exists := cached[id]; !exists {
			cached[id] = protocol.ResourceSource{Node: id, Route: protocol.RouteKey{Entry: node.ID(), Target: id}}
		}
	}
	// A persisted source can precede its Client during startup. Relationship
	// removal is authoritative; transport startup or failure is not deletion.
	for id, source := range cached {
		if checkRelationships && !allowed[id] {
			continue
		}
		if remote := peers[id]; remote != nil {
			if !checkRelationships && !node.resourceSourceCurrent(ctx, id, remote) {
				continue
			}
			// The shared observer is the single source writer. Catalog reads
			// consume its projection without starting another remote snapshot
			// that could finish later and overwrite a newer version.
			confirmed := false
			if value, exists := resourceProjections.Load(resourceProjectionKey{node, id, user.ID}); exists {
				state := value.(*resourceProjection)
				state.mu.Lock()
				if state.known && !state.removed {
					source = state.source
					confirmed = true
				}
				state.mu.Unlock()
			}
			if !remote.Connected() {
				source.Fault = resourceDisconnected(remote)
			} else if !confirmed {
				// A connected Client confirms its project snapshot, not the full
				// knowledge definitions. Disk summaries remain read-only until
				// this process's observer has confirmed the source metadata.
				source.Fault = resourceUnavailable()
			}
			if !resourceCacheAllowed(source.Fault) {
				source.Projects, source.Worktrees, source.Knowledge = nil, nil, nil
				source.Revision, source.LastSyncedAtMs = 0, nil
			}
		} else {
			source.Fault = resourceUnavailable()
		}
		if name := names[id]; name != "" {
			source.Name = name
		}
		catalog.Sources = append(catalog.Sources, source)
	}
	slices.SortStableFunc(catalog.Sources, func(a, b protocol.ResourceSource) int {
		if a.Node == b.Node {
			return 0
		}
		if a.Node == node.ID() {
			return -1
		}
		if b.Node == node.ID() {
			return 1
		}
		return cmp.Compare(a.Node.String(), b.Node.String())
	})
	return catalog, nil
}

func (node *Node) cachedResourceSources(ctx context.Context) (map[protocol.NodeID]protocol.ResourceSource, error) {
	values := map[protocol.NodeID]protocol.ResourceSource{}
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		stored, err := store.ReadResourceCatalogSources(ctx, db, node.ID())
		for _, source := range stored {
			values[source.Node] = source
		}
		return err
	})
	return values, err
}

func (node *Node) captureResourceSource(ctx context.Context, source protocol.NodeID, remote *client.Client, previous protocol.ResourceSource) (protocol.ResourceSource, bool) {
	previous.Node, previous.Route = source, protocol.RouteKey{Entry: node.ID(), Target: source}
	previous.Fault = nil
	complete := false
	if !remote.Connected() {
		previous.Fault = resourceDisconnected(remote)
	} else {
		// Client publishes a replacement only after every snapshot page has
		// validated. Never persist an in-progress snapshot as an empty source.
		previous.Projects, previous.Worktrees, previous.Revision = remote.ResourceSnapshot()
		previous.Worktrees = resourceDirectoryWorktrees(previous.Projects, previous.Worktrees)
		now := uint64(time.Now().UnixMilli())
		previous.LastSyncedAtMs, complete = &now, true
		request := protocol.NewRequest(source, protocol.Command{Kind: "list_knowledge_collections"})
		output, err := resourceResult(ctx, remote.Resource, protocol.ResourceCall{Request: request, Local: true})
		if err != nil {
			previous.Fault = resourceFault(err)
		} else if collections, valid := dispatchPayload[[]protocol.KnowledgeCollection](output.Data); valid && output.Kind == "knowledge_collections" {
			previous.Knowledge = collections
		} else {
			previous.Fault = resourceUnavailable()
		}
	}
	if !resourceCacheAllowed(previous.Fault) {
		previous.Projects, previous.Worktrees, previous.Knowledge = nil, nil, nil
		previous.Revision, previous.LastSyncedAtMs, complete = 0, nil, false
	}
	return previous, complete
}

func resourceDirectoryWorktrees(projects []protocol.Project, worktrees []protocol.Worktree) []protocol.Worktree {
	// Canonical history can retain a removed project's worktrees for admitted
	// tasks. A directory must remove those descendants with the project.
	present := make(map[protocol.ProjectID]bool, len(projects))
	for _, project := range projects {
		present[project.ID] = true
	}
	return slices.DeleteFunc(worktrees, func(worktree protocol.Worktree) bool {
		return worktree.Project != nil && !present[*worktree.Project]
	})
}

func resourceResult(ctx context.Context, send func(context.Context, protocol.ResourceCall) (link.Admission, error), call protocol.ResourceCall) (protocol.Output, error) {
	accepted, err := send(ctx, call)
	if err != nil {
		return protocol.Output{}, err
	}
	select {
	case result, open := <-accepted.Completion:
		if !open {
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "resource completion is unavailable"}
		}
		if result.Fault != nil {
			return protocol.Output{}, result.Fault
		}
		return result.Output, nil
	case <-ctx.Done():
		return protocol.Output{}, ctx.Err()
	}
}

func (node *Node) aggregateKnowledge(ctx context.Context, caller protocol.NodeID, call protocol.ResourceCall) (protocol.Output, error) {
	request := call.Request
	query, searching := request.Command.Data.(*protocol.KnowledgeQuery)
	if searching {
		if err := knowledge.ValidateQuery(*query); err != nil {
			return protocol.Output{}, err
		}
	}
	type result struct {
		node   protocol.NodeID
		output protocol.Output
		err    error
	}
	peers := node.resourceClients(ctx)
	known := map[protocol.NodeID]bool{node.ID(): true}
	for id := range peers {
		known[id] = true
	}
	missing := []protocol.NodeID{}
	if user, present := store.UserFromContext(ctx); present && user.Local {
		cached, err := node.cachedResourceSources(ctx)
		if err != nil {
			return protocol.Output{}, err
		}
		for id := range cached {
			if !known[id] {
				known[id] = true
				missing = append(missing, id)
			}
		}
	}
	results := make(chan result, len(peers)+len(missing)+1)
	var workers sync.WaitGroup
	searchNode := func(id protocol.NodeID) bool {
		return !searching || len(query.Refs) == 0 || slices.ContainsFunc(query.Refs, func(reference protocol.KnowledgeRef) bool { return reference.Node == id })
	}
	for _, id := range missing {
		if searchNode(id) {
			results <- result{node: id, err: resourceUnavailable()}
		}
	}
	leaf := func(id protocol.NodeID) protocol.ResourceCall {
		command := request.Command
		if searching {
			selected := *query
			selected.Offset, selected.Limit = 0, knowledge.PageLimit
			command.Data = &selected
		}
		selected := protocol.NewRequest(id, command)
		selected.Plugin = request.Plugin
		return protocol.ResourceCall{Local: true, Request: selected, Scope: call.Scope}
	}
	if searchNode(node.ID()) {
		workers.Go(func() {
			output, err := resourceKnowledgePrefix(ctx, leaf(node.ID()), query, func(call protocol.ResourceCall) (protocol.Output, error) {
				return node.readKnowledge(ctx, call.Request)
			})
			results <- result{node.ID(), output, err}
		})
	}
	for id, peer := range peers {
		if !searchNode(id) {
			continue
		}
		workers.Go(func() {
			if !peer.Connected() {
				results <- result{node: id, err: resourceUnavailable()}
				return
			}
			output, err := resourceKnowledgePrefix(ctx, leaf(id), query, func(call protocol.ResourceCall) (protocol.Output, error) {
				return resourceResult(ctx, peer.Resource, call)
			})
			results <- result{id, output, err}
		})
	}
	workers.Wait()
	close(results)
	collections := []protocol.KnowledgeCollection{}
	page := protocol.DefaultKnowledgePage()
	ranks := map[string]float64{}
	page.Untrusted = true
	if searching {
		page.Mode = query.Mode
	}
	if searching {
		for _, ref := range query.Refs {
			if !known[ref.Node] && !slices.ContainsFunc(page.Sources, func(source protocol.ResourceSourceStatus) bool { return source.Node == ref.Node }) {
				page.Sources = append(page.Sources, protocol.ResourceSourceStatus{Node: ref.Node, Fault: resourceUnavailable()})
			}
		}
	}
	modelSeen := false
	for result := range results {
		page.Sources = append(page.Sources, protocol.ResourceSourceStatus{Node: result.node, Fault: resourceFault(result.err)})
		if result.err != nil {
			continue
		}
		if searching {
			part, valid := dispatchPayload[protocol.KnowledgePage](result.output.Data)
			if !valid || result.output.Kind != "knowledge_search" {
				page.Sources[len(page.Sources)-1].Fault = resourceUnavailable()
				continue
			}
			page.Sources[len(page.Sources)-1].NextOffset = part.NextOffset
			for position, hit := range part.Hits {
				key := hit.Citation.Node.String() + "/" + hit.Citation.Collection.String() + "/" + hit.Citation.Chunk
				ranks[key] = 1 / float64(60+position+1)
				page.Hits = append(page.Hits, hit)
			}
			page.Indexing = page.Indexing || part.Indexing
			if !modelSeen {
				page.Model, page.Mode, modelSeen = part.Model, part.Mode, true
			} else {
				if page.Model == nil || part.Model == nil || *page.Model != *part.Model {
					page.Model = nil
				}
				if page.Mode != part.Mode {
					page.Mode = protocol.KnowledgeModeHybrid
				}
			}
		} else if part, valid := dispatchPayload[[]protocol.KnowledgeCollection](result.output.Data); valid && result.output.Kind == "knowledge_collections" {
			collections = append(collections, part...)
		} else {
			page.Sources[len(page.Sources)-1].Fault = resourceUnavailable()
		}
	}
	slices.SortFunc(page.Sources, func(a, b protocol.ResourceSourceStatus) int { return cmp.Compare(a.Node.String(), b.Node.String()) })
	if !searching {
		// This legacy list has no source-status field. A partial directory must
		// fail explicitly; ResourceCatalog is the projection that can represent
		// cached entries alongside an unavailable source.
		for _, source := range page.Sources {
			if source.Fault != nil {
				fault := *source.Fault
				fault.Message = "knowledge directory source " + source.Node.String() + ": " + fault.Message
				return protocol.Output{}, &fault
			}
		}
		slices.SortFunc(collections, func(a, b protocol.KnowledgeCollection) int {
			return cmp.Or(cmp.Compare(a.Definition.Name, b.Definition.Name), cmp.Compare(a.Node.String(), b.Node.String()), cmp.Compare(a.ID.String(), b.ID.String()))
		})
		return protocol.Output{Kind: "knowledge_collections", Data: collections}, nil
	}
	for index := range page.Hits {
		hit := &page.Hits[index]
		key := hit.Citation.Node.String() + "/" + hit.Citation.Collection.String() + "/" + hit.Citation.Chunk
		hit.Score = uint32(ranks[key] * 1_000_000)
	}
	slices.SortFunc(page.Hits, func(a, b protocol.KnowledgeHit) int {
		left := a.Citation.Node.String() + "/" + a.Citation.Collection.String() + "/" + a.Citation.Chunk
		right := b.Citation.Node.String() + "/" + b.Citation.Collection.String() + "/" + b.Citation.Chunk
		return cmp.Or(cmp.Compare(ranks[right], ranks[left]), cmp.Compare(a.Citation.Node.String(), b.Citation.Node.String()), cmp.Compare(a.Citation.Collection.String(), b.Citation.Collection.String()), cmp.Compare(a.Citation.Chunk, b.Citation.Chunk))
	})
	start := min(uint64(query.Offset), uint64(len(page.Hits)))
	end := min(start+uint64(min(query.Limit, knowledge.PageLimit)), uint64(len(page.Hits)))
	if end < uint64(len(page.Hits)) || slices.ContainsFunc(page.Sources, func(source protocol.ResourceSourceStatus) bool { return source.NextOffset != nil }) {
		next := uint32(end)
		page.NextOffset = &next
	}
	page.Hits = page.Hits[start:end]
	return protocol.Output{Kind: "knowledge_search", Data: page}, nil
}

// resourceKnowledgePrefix reads enough of each source's ranking to preserve
// global offset paging. A source's own cursor must not be mistaken for EOF.
func resourceKnowledgePrefix(ctx context.Context, call protocol.ResourceCall, query *protocol.KnowledgeQuery, read func(protocol.ResourceCall) (protocol.Output, error)) (protocol.Output, error) {
	if query == nil {
		return read(call)
	}
	wanted := uint64(query.Offset) + uint64(min(query.Limit, knowledge.PageLimit)) + 1
	page := protocol.DefaultKnowledgePage()
	for {
		output, err := read(call)
		if err != nil {
			return protocol.Output{}, err
		}
		part, valid := dispatchPayload[protocol.KnowledgePage](output.Data)
		if !valid || output.Kind != "knowledge_search" {
			return protocol.Output{}, resourceUnavailable()
		}
		input := *call.Request.Command.Data.(*protocol.KnowledgeQuery)
		if part.NextOffset != nil && (*part.NextOffset <= input.Offset || len(part.Hits) == 0) {
			return protocol.Output{}, resourceUnavailable()
		}
		page.Hits = append(page.Hits, part.Hits...)
		page.Mode, page.Model = part.Mode, part.Model
		page.Indexing, page.Untrusted = page.Indexing || part.Indexing, true
		page.NextOffset = part.NextOffset
		if part.NextOffset == nil || uint64(len(page.Hits)) >= wanted {
			return protocol.Output{Kind: "knowledge_search", Data: page}, nil
		}
		input.Offset = *part.NextOffset
		plugin := call.Request.Plugin
		call.Request = protocol.NewRequest(call.Request.Target, protocol.Command{Kind: "query_knowledge", Data: &input})
		call.Request.Plugin = plugin
	}
}

// An unavailable transport can retain a projection. An authorization failure
// invalidates its contents immediately, including when Client closed its stream.
func resourceCacheAllowed(fault *protocol.Fault) bool {
	return fault == nil || fault.Code != protocol.ErrorCodePermissionDenied && fault.Code != protocol.ErrorCodeNotFound && fault.Code != protocol.ErrorCodeExpired
}

func resourceDisconnected(remote *client.Client) *protocol.Fault {
	if err := remote.Err(); err != nil {
		return resourceFault(err)
	}
	return resourceUnavailable()
}

// resourceProjections coalesces the existing direct-host observer's changes.
// Directory contents survive here only as a view of the per-user Store cache.
var resourceProjections sync.Map

type resourceProjectionKey struct {
	owner  *Node
	source protocol.NodeID
	user   protocol.UserID
}

type resourceProjection struct {
	mu         sync.Mutex
	running    bool
	dirty      bool
	removed    bool
	known      bool
	generation uint64
	digest     [32]byte
	source     protocol.ResourceSource
}

func (node *Node) saveResourceProjection(ctx context.Context, state *resourceProjection, source protocol.ResourceSource, complete bool, generation uint64) error {
	// Invalidation and commit use the same lifecycle lock. A metadata answer
	// captured before access was revoked cannot recreate the cleared cache.
	state.mu.Lock()
	if state.removed || state.generation != generation {
		state.mu.Unlock()
		return nil
	}
	changed, err := node.commitResourceProjection(ctx, state, source, complete)
	state.mu.Unlock()
	if err == nil && changed {
		node.publish()
	}
	return err
}

func (node *Node) revokeResourceCatalog(ctx context.Context, user protocol.UserID, source protocol.NodeID, fault *protocol.Fault) error {
	key := resourceProjectionKey{node, source, user}
	value, _ := resourceProjections.LoadOrStore(key, &resourceProjection{})
	state := value.(*resourceProjection)
	state.mu.Lock()
	if state.removed {
		state.mu.Unlock()
		return nil
	}
	state.generation++
	cleared := protocol.ResourceSource{Node: source, Route: protocol.RouteKey{Entry: node.ID(), Target: source}, Fault: fault}
	// Even a failed disk write cannot leave private details in the live view.
	// Keep the old digest until commit so a later successful clear still emits
	// its invalidation event.
	state.source = cleared
	changed, err := node.commitResourceProjection(ctx, state, cleared, false)
	state.mu.Unlock()
	if err == nil && changed {
		node.publish()
	}
	return err
}

// commitResourceProjection holds state.mu across the durable write and memory
// publication. The caller establishes the capture's current access generation.
func (node *Node) commitResourceProjection(ctx context.Context, state *resourceProjection, source protocol.ResourceSource, complete bool) (bool, error) {
	content := source
	// Source events can advance its cursor without changing any directory
	// content. Publishing those metadata changes would circulate invalidations
	// between directly connected Nodes indefinitely.
	content.Revision, content.LastSyncedAtMs = 0, nil
	body, err := json.Marshal(content)
	if err != nil {
		return false, err
	}
	digest := blake3.Sum256(body)
	changed := !state.known || state.digest != digest
	if !changed {
		state.source = source
		return false, nil
	}
	err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		if complete {
			// Snapshot availability and knowledge metadata are independent. A
			// failed knowledge read retains its previous directory, while this
			// complete project snapshot can still confirm creation or deletion.
			cached := source
			cached.Fault = nil
			if err := store.SaveResourceCatalogSource(ctx, db, node.ID(), cached); err != nil {
				return err
			}
		} else if !resourceCacheAllowed(source.Fault) {
			if err := store.RemoveResourceCatalogSource(ctx, db, source.Node); err != nil {
				return err
			}
		}
		_, err := store.RecordEvents(ctx, db, node.ID(), []protocol.Event{{Kind: "resource_catalog_changed"}})
		return err
	})
	if err == nil {
		state.known, state.digest, state.source = true, digest, source
	}
	return changed, err
}

func (node *Node) forgetResourceCatalog(ctx context.Context, source protocol.NodeID) error {
	node.invalidateResourceProjection(source)
	return node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		return store.RemoveResourceCatalogSource(ctx, db, source)
	})
}

func (node *Node) invalidateResourceProjection(source protocol.NodeID) {
	resourceProjections.Range(func(key, value any) bool {
		identity := key.(resourceProjectionKey)
		if identity.owner != node || identity.source != source {
			return true
		}
		state := value.(*resourceProjection)
		state.mu.Lock()
		state.removed = true
		state.source = protocol.ResourceSource{}
		state.mu.Unlock()
		resourceProjections.Delete(key)
		return true
	})
}

func (node *Node) resourceSourceCurrent(ctx context.Context, source protocol.NodeID, remote *client.Client) bool {
	if node.controllerClients()[source] != remote {
		return false
	}
	if node.network == nil {
		return true
	}
	relationships, err := node.network.Relationships(ctx)
	if err != nil {
		// A failed local relationship read is not an explicit removal. The
		// cache write rechecks its direct outbound relationship transactionally.
		return true
	}
	return slices.ContainsFunc(relationships, func(peer link.PeerRelationship) bool {
		return peer.ID == source && peer.Direct && peer.DirectDirection.AllowsOutbound() && !peer.Removed
	})
}

func (node *Node) notifyResourceCatalog(source protocol.NodeID, remote *client.Client) {
	var user protocol.User
	if err := node.profile.Store.Node.Read(node.ctx, func(db *sql.Conn) error {
		var err error
		user, err = store.LocalUser(node.ctx, db)
		return err
	}); err != nil {
		return
	}
	owner := store.WithUser(node.ctx, user)
	if !remote.Connected() {
		if fault := resourceDisconnected(remote); !resourceCacheAllowed(fault) {
			if err := node.revokeResourceCatalog(owner, user.ID, source, fault); err != nil {
				return
			}
		}
	}
	key := resourceProjectionKey{node, source, user.ID}
	value, _ := resourceProjections.LoadOrStore(key, &resourceProjection{})
	state := value.(*resourceProjection)
	state.mu.Lock()
	state.dirty = true
	if state.running || state.removed {
		state.mu.Unlock()
		return
	}
	state.running = true
	state.mu.Unlock()
	err := node.tasks.Submit(func(ctx context.Context) {
		ctx, cancel := context.WithCancel(store.WithUser(ctx, user))
		defer cancel()
		defer context.AfterFunc(node.ctx, cancel)()
		retry := time.Second
		for {
			state.mu.Lock()
			if ctx.Err() != nil || state.removed || !state.dirty {
				state.running = false
				state.mu.Unlock()
				return
			}
			state.dirty = false
			projected, known, generation := state.source, state.known, state.generation
			state.mu.Unlock()
			if !known {
				stored, err := node.cachedResourceSources(ctx)
				if err != nil {
					state.mu.Lock()
					state.running = false
					state.mu.Unlock()
					return
				}
				projected = stored[source]
			}
			if !node.resourceSourceCurrent(ctx, source, remote) {
				state.mu.Lock()
				state.running = false
				state.mu.Unlock()
				return
			}
			projected, complete := node.captureResourceSource(ctx, source, remote, projected)
			if !node.resourceSourceCurrent(ctx, source, remote) {
				state.mu.Lock()
				state.running = false
				state.mu.Unlock()
				return
			}
			if err := node.saveResourceProjection(ctx, state, projected, complete, generation); err != nil {
				state.mu.Lock()
				state.running = false
				state.mu.Unlock()
				return
			}
			if complete && projected.Fault != nil && resourceObservationTransient(projected.Fault) {
				// The Node snapshot may remain healthy while a knowledge
				// read fails. Recover that projection without waiting for an
				// unrelated event or another controller to read the catalog.
				timer := time.NewTimer(retry)
				select {
				case <-ctx.Done():
					timer.Stop()
				case <-timer.C:
					state.mu.Lock()
					if !state.removed && state.generation == generation {
						state.dirty = true
					}
					state.mu.Unlock()
				}
				retry = min(retry*2, 30*time.Second)
			} else {
				retry = time.Second
			}
		}
	})
	if err != nil {
		state.mu.Lock()
		state.running = false
		state.mu.Unlock()
	}
}
