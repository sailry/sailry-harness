package mcp

import (
	"context"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"slices"
	"sync"
	"sync/atomic"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/modelcontextprotocol/go-sdk/jsonrpc"
	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/sync/semaphore"
	"lukechampine.com/blake3"
)

const (
	maxConnections = 32
	idleTTL        = 30 * time.Minute
	// Package MCP tools are deferred: the model sees their names until
	// tool_search loads one, so a server may list a service's full toolset.
	maxTools       = 256
	maxListedTools = 4096
	maxToolBytes   = 1024 * 1024
	maxMessage     = 8 * 1024 * 1024
	expiryInterval = time.Minute
	maxCalls       = 8
)

type Configuration struct {
	User          string
	Package       protocol.PluginReference
	Server        string
	Launch        Launch
	Authorization any
	Options       protocol.PluginMcpOptions
	Tools         *[]string
	Validate      func(context.Context) error
	Release       func() error
}

type Options struct {
	Clock clockwork.Clock
	// Warm asks the pool's owner to connect the reusable servers of the
	// current packages ahead of their first use.
	Warm bool
	Open func(context.Context, Launch, func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error)
}

type connectionKey struct {
	User, Package, Digest, Server string
	Settings                      uint64
	Authorization, Launch         [32]byte
}

type Pool struct {
	ctx      context.Context
	cancel   context.CancelFunc
	clock    clockwork.Clock
	open     func(context.Context, Launch, func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error)
	mu       sync.Mutex
	entries  map[connectionKey]*connection
	owners   map[*connection]struct{}
	current  map[packageKey]protocol.PluginSummary
	services map[string]*serviceLock
	status   map[statusKey]*statusRecord
	pending  map[Change]struct{}
	changed  chan struct{}
	stopped  bool
	known    map[string]bool
	work     sync.WaitGroup
}

type packageKey struct{ User, Package string }

type statusKey struct{ User, Package, Server string }

// Change identifies one user installation whose runtime changed.
type Change struct{ User, Package string }

// statusRecord is what a server of the current package version last reported.
// It outlives the connection, so a server closed for being idle is still the
// one that connected. An interrupted attempt returns to the settled runtime.
type statusRecord struct {
	reference protocol.PluginReference
	runtime   *protocol.PluginMcpRuntime
	settled   *protocol.PluginMcpRuntime
}

type serviceLock struct {
	lock *semaphore.Weighted
	refs int
}

type connection struct {
	pool              *Pool
	key               connectionKey
	reference         protocol.PluginReference
	ready             chan struct{}
	session           *sdk.ClientSession
	fault             error
	cancel            context.CancelFunc
	leases            int
	retired           bool
	used              time.Time
	reuse             bool
	exclusive         *serviceLock
	calls             *semaphore.Weighted
	mu                sync.Mutex
	poisoned          bool
	closing           bool
	closeOnce         sync.Once
	closeDone         chan struct{}
	closeErr          error
	release           func() error
	descriptors       []*sdk.Tool
	descriptorsFilter []string
	descriptorsReady  bool
	descriptorsGen    uint64
}

type Lease struct {
	pool       *Pool
	connection *connection
	config     Configuration
	closeOnce  sync.Once
	closeErr   error
}

// AuthorityKey identifies the destination and authorization without retaining
// the lease or exposing its credentials.
type AuthorityKey struct {
	key connectionKey
}

func (lease *Lease) AuthorityKey() AuthorityKey {
	return AuthorityKey{key: lease.connection.key}
}

func New(ctx context.Context, options Options) *Pool {
	if options.Clock == nil {
		options.Clock = clockwork.NewRealClock()
	}
	if options.Open == nil {
		options.Open = Open
	}
	lifetime, cancel := context.WithCancel(ctx)
	pool := &Pool{
		ctx: lifetime, cancel: cancel, clock: options.Clock, open: options.Open,
		entries: map[connectionKey]*connection{}, current: map[packageKey]protocol.PluginSummary{},
		services: map[string]*serviceLock{}, owners: map[*connection]struct{}{},
		status: map[statusKey]*statusRecord{}, pending: map[Change]struct{}{}, known: map[string]bool{}, changed: make(chan struct{}, 1),
	}
	// Idle servers leave on the pool's own timer, not only when a later
	// acquisition happens to scan them.
	ticker := pool.clock.NewTicker(expiryInterval)
	pool.work.Add(1)
	go func() {
		defer pool.work.Done()
		defer ticker.Stop()
		for {
			select {
			case <-lifetime.Done():
				return
			case <-ticker.Chan():
				if err := pool.Expire(); err != nil {
					slog.Warn("MCP connection cleanup failed", "code", protocolFault(err).Code)
				}
			}
		}
	}()
	return pool
}

func keyFor(config Configuration) (connectionKey, error) {
	authorization, err := json.Marshal(config.Authorization)
	if err != nil {
		return connectionKey{}, invalid("invalid MCP authorization")
	}
	launch, err := json.Marshal(config.Launch.identity())
	if err != nil {
		return connectionKey{}, invalid("invalid MCP launch")
	}
	return connectionKey{
		User: config.User, Package: config.Package.Name, Digest: config.Package.Digest, Settings: config.Package.SettingsRevision,
		Server: config.Server, Authorization: blake3.Sum256(authorization), Launch: blake3.Sum256(launch),
	}, nil
}

func (pool *Pool) Observe(summary protocol.PluginSummary, user string) error {
	identity := packageKey{user, summary.Name}
	pool.mu.Lock()
	previous, exists := pool.current[identity]
	if exists && (previous.Revision > summary.Revision || previous.Revision == summary.Revision && previous.SettingsRevision > summary.SettingsRevision) {
		pool.mu.Unlock()
		return nil
	}
	pool.current[identity] = summary
	if !exists || previous.Digest != summary.Digest || previous.SettingsRevision != summary.SettingsRevision ||
		(previous.Enabled && previous.Compatibility.Kind == "compatible") != (summary.Enabled && summary.Compatibility.Kind == "compatible") {
		pool.forget(summary.Name, user)
	}
	var retired []*connection
	for key, current := range pool.entries {
		if key.User == user && key.Package == summary.Name && (!summary.Enabled || summary.Compatibility.Kind != "compatible" || key.Digest != summary.Digest || key.Settings != summary.SettingsRevision) {
			delete(pool.entries, key)
			current.retired = true
			if current.leases == 0 {
				retired = append(retired, current)
			}
		}
	}
	pool.mu.Unlock()
	return closeConnections(retired)
}

func (pool *Pool) FinishInventory(user string) {
	pool.mu.Lock()
	pool.known[user] = true
	pool.mu.Unlock()
}

func (pool *Pool) reusable(reference protocol.PluginReference, user string) bool {
	summary, exists := pool.current[packageKey{user, reference.Name}]
	if !exists {
		return !pool.known[user]
	}
	return summary.Enabled && summary.Compatibility.Kind == "compatible" &&
		summary.Digest == reference.Digest && summary.SettingsRevision == reference.SettingsRevision
}

func (pool *Pool) Acquire(ctx context.Context, config Configuration) (result *Lease, resultErr error) {
	adopted := false
	defer func() {
		if !adopted && config.Release != nil {
			if err := config.Release(); err != nil {
				if result != nil {
					resultErr = errors.Join(resultErr, result.Close())
					result = nil
				}
				resultErr = errors.Join(resultErr, err)
			}
		}
	}()
	key, err := keyFor(config)
	if err != nil {
		return nil, err
	}
	pool.mu.Lock()
	if pool.stopped || ctx.Err() != nil {
		pool.mu.Unlock()
		return nil, cancelled()
	}
	reuse := config.Options.Reuse && pool.reusable(config.Package, config.User)
	var retired []*connection
	for candidate, current := range pool.entries {
		expired := current.leases == 0 && pool.clock.Since(current.used) >= idleTTL
		changed := candidate.User == key.User && candidate.Package == key.Package && candidate.Server == key.Server &&
			(candidate.Authorization != key.Authorization || (candidate.Digest != key.Digest || candidate.Settings != key.Settings) && current.leases == 0)
		if expired || changed {
			delete(pool.entries, candidate)
			current.retired = true
			if current.leases == 0 {
				retired = append(retired, current)
			}
		}
	}
	current := pool.entries[key]
	if current != nil {
		current.mu.Lock()
		broken := current.poisoned || current.fault != nil
		current.mu.Unlock()
		if broken {
			delete(pool.entries, key)
			current.retired = true
			if current.leases == 0 {
				retired = append(retired, current)
			}
			current = nil
		}
	}
	if !reuse {
		current = nil
	}
	if current == nil {
		if reuse && len(pool.entries) >= maxConnections {
			var oldest *connection
			for _, candidate := range pool.entries {
				if candidate.leases == 0 && (oldest == nil || candidate.used.Before(oldest.used)) {
					oldest = candidate
				}
			}
			if oldest != nil {
				delete(pool.entries, oldest.key)
				oldest.retired = true
				retired = append(retired, oldest)
			}
		}
		if reuse && len(pool.entries) >= maxConnections {
			pool.mu.Unlock()
			if err := closeConnections(retired); err != nil {
				return nil, err
			}
			return nil, &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "MCP connection pool is busy"}
		}
		serviceID := config.User + "\x00" + config.Package.Name + "\x00" + config.Server
		service := pool.services[serviceID]
		if service == nil {
			service = &serviceLock{lock: semaphore.NewWeighted(1)}
			pool.services[serviceID] = service
		}
		service.refs++
		opening, cancel := context.WithCancel(pool.ctx)
		current = &connection{
			pool: pool, key: key, reference: config.Package, ready: make(chan struct{}), closeDone: make(chan struct{}), cancel: cancel,
			used: pool.clock.Now(), reuse: reuse, exclusive: service, calls: semaphore.NewWeighted(maxCalls),
			release: config.Release,
		}
		pool.report(current, protocol.PluginMcpRuntimeStateConnecting, 0, nil)
		adopted = true
		if reuse {
			pool.entries[key] = current
		}
		pool.owners[current] = struct{}{}
		pool.work.Add(1)
		go current.open(opening, config.Launch)
	}
	current.leases++
	current.used = pool.clock.Now()
	pool.mu.Unlock()
	if err := closeConnections(retired); err != nil {
		return nil, errors.Join(err, (&Lease{pool: pool, connection: current}).Close())
	}
	lease := &Lease{pool: pool, connection: current, config: config}
	select {
	case <-ctx.Done():
		return nil, errors.Join(cancelled(), lease.Close())
	case <-pool.ctx.Done():
		return nil, errors.Join(cancelled(), lease.Close())
	case <-current.ready:
		current.mu.Lock()
		err := current.fault
		current.mu.Unlock()
		if err != nil {
			return nil, errors.Join(err, lease.Close())
		}
		return lease, nil
	}
}

func (connection *connection) open(ctx context.Context, launch Launch) {
	defer connection.pool.work.Done()
	finished := make(chan struct{})
	timer := connection.pool.clock.NewTimer(20 * time.Second)
	var expired atomic.Bool
	go func() {
		select {
		case <-timer.Chan():
			expired.Store(true)
			connection.cancel()
		case <-finished:
		case <-ctx.Done():
		}
	}()
	launch.OnToolsChanged = connection.invalidateDescriptors
	session, err := connection.pool.open(ctx, launch, connection.elicit)
	close(finished)
	timer.Stop()
	// A connection closed while it opened did not fail; one that ran out of
	// time did.
	interrupted := ctx.Err() != nil && !expired.Load()
	if err == nil && ctx.Err() != nil {
		err = unavailable()
		if session != nil {
			err = errors.Join(err, cleanup(session))
			session = nil
		}
	}
	if session != nil {
		// A server that closed its side is replaced at the next acquisition.
		connection.pool.work.Add(1)
		go func() {
			defer connection.pool.work.Done()
			_ = session.Wait()
			connection.mu.Lock()
			ended := !connection.closing
			connection.poisoned = true
			connection.mu.Unlock()
			if ended {
				connection.pool.mu.Lock()
				connection.pool.report(connection, protocol.PluginMcpRuntimeStateFailed, 0, unavailable())
				connection.pool.mu.Unlock()
			}
		}()
	}
	connection.mu.Lock()
	connection.session = session
	connection.fault = err
	connection.mu.Unlock()
	// A connected server reports once its tools are listed.
	if err != nil {
		failure := err
		if interrupted {
			failure = cancelled()
		}
		connection.pool.mu.Lock()
		connection.pool.report(connection, protocol.PluginMcpRuntimeStateFailed, 0, failure)
		connection.pool.mu.Unlock()
	}
	close(connection.ready)
}

// report records what a server of the current package version did and marks
// the package changed when its state, tool count or fault differs. A busy or
// cancelled attempt returns the server to the runtime it last settled on. The
// caller holds the pool lock.
func (pool *Pool) report(current *connection, state protocol.PluginMcpRuntimeState, tools int, failure error) {
	if !pool.reusable(current.reference, current.key.User) {
		return
	}
	key := statusKey{current.key.User, current.reference.Name, current.key.Server}
	record := pool.status[key]
	if record == nil || record.reference != current.reference {
		record = &statusRecord{reference: current.reference}
		pool.status[key] = record
	}
	next := &protocol.PluginMcpRuntime{State: state, Tools: uint32(tools), UpdatedAtMs: uint64(pool.clock.Now().UnixMilli())}
	interrupted := false
	if failure != nil {
		fault := protocolFault(failure)
		interrupted = fault.Code == protocol.ErrorCodeBusy || fault.Code == protocol.ErrorCodeCancelled
		next.State, next.Tools, next.Fault = protocol.PluginMcpRuntimeStateFailed, 0, fault
	}
	if interrupted {
		next = record.settled
	}
	same := record.runtime == nil && next == nil || record.runtime != nil && next != nil &&
		record.runtime.State == next.State && record.runtime.Tools == next.Tools &&
		(record.runtime.Fault == nil) == (next.Fault == nil) &&
		(next.Fault == nil || record.runtime.Fault.Code == next.Fault.Code && record.runtime.Fault.Message == next.Fault.Message)
	if !same {
		record.runtime = next
	}
	if state != protocol.PluginMcpRuntimeStateConnecting && !interrupted {
		record.settled = record.runtime
	}
	if !same {
		pool.pending[Change{current.key.User, current.reference.Name}] = struct{}{}
		select {
		case pool.changed <- struct{}{}:
		default:
		}
	}
}

// forget drops what the servers of a package reported. The caller holds the
// pool lock.
func (pool *Pool) forget(packageName, user string) {
	for key := range pool.status {
		if key.User == user && (packageName == "" || key.Package == packageName) {
			delete(pool.status, key)
		}
	}
}

// Changed wakes when a server's runtime differs from the one last reported.
func (pool *Pool) Changed() <-chan struct{} { return pool.changed }

// ScopedChanges drains runtime changes with their owning user.
func (pool *Pool) ScopedChanges() []Change {
	pool.mu.Lock()
	defer pool.mu.Unlock()
	changes := make([]Change, 0, len(pool.pending))
	for change := range pool.pending {
		changes = append(changes, change)
	}
	clear(pool.pending)
	slices.SortFunc(changes, func(a, b Change) int {
		if a.User < b.User {
			return -1
		}
		if a.User > b.User {
			return 1
		}
		if a.Package < b.Package {
			return -1
		}
		if a.Package > b.Package {
			return 1
		}
		return 0
	})
	return changes
}

// Changes takes package names for owners that do not use user namespaces.
func (pool *Pool) Changes() []string {
	var names []string
	for _, change := range pool.ScopedChanges() {
		names = append(names, change.Package)
	}
	slices.Sort(names)
	return slices.Compact(names)
}

// List reads the server's tools. Its outcome is what the server reports as
// its runtime, except for a caller that gave up.
func (lease *Lease) List(ctx context.Context) ([]*sdk.Tool, error) {
	tools, err := lease.list(ctx, true)
	failure := err
	if err != nil && ctx.Err() != nil {
		failure = cancelled()
	}
	lease.pool.mu.Lock()
	lease.pool.report(lease.connection, protocol.PluginMcpRuntimeStateConnected, len(tools), failure)
	lease.pool.mu.Unlock()
	return tools, err
}

// ListCached returns cached tool descriptors on an already-inspected healthy
// connection before falling back to a wire ListTools call.
func (lease *Lease) ListCached(ctx context.Context) ([]*sdk.Tool, error) {
	tools, err := lease.list(ctx, false)
	failure := err
	if err != nil && ctx.Err() != nil {
		failure = cancelled()
	}
	lease.pool.mu.Lock()
	lease.pool.report(lease.connection, protocol.PluginMcpRuntimeStateConnected, len(tools), failure)
	lease.pool.mu.Unlock()
	return tools, err
}

func (lease *Lease) filterDescriptors(all []*sdk.Tool) []*sdk.Tool {
	selected := lease.config.Tools
	if selected == nil {
		return slices.Clone(all)
	}
	filtered := make([]*sdk.Tool, 0, len(all))
	for _, tool := range all {
		if slices.Contains(*selected, tool.Name) {
			filtered = append(filtered, tool)
		}
	}
	return filtered
}

func (connection *connection) invalidateDescriptors() {
	connection.mu.Lock()
	connection.descriptorsReady = false
	connection.descriptors = nil
	connection.descriptorsFilter = nil
	connection.descriptorsGen++
	connection.mu.Unlock()
}

func normalizeToolSelection(selected []string) []string {
	normalized := slices.Clone(selected)
	slices.Sort(normalized)
	return slices.Compact(normalized)
}

func (lease *Lease) cachedDescriptors() ([]*sdk.Tool, bool) {
	connection := lease.connection
	connection.mu.Lock()
	ready := connection.descriptorsReady && !connection.poisoned && connection.fault == nil
	cached := connection.descriptors
	filter := connection.descriptorsFilter
	connection.mu.Unlock()
	if !ready {
		return nil, false
	}
	if filter == nil {
		return lease.filterDescriptors(cached), true
	}
	if lease.config.Tools != nil && slices.Equal(filter, normalizeToolSelection(*lease.config.Tools)) {
		return slices.Clone(cached), true
	}
	return nil, false
}

func (lease *Lease) list(ctx context.Context, refresh bool) ([]*sdk.Tool, error) {
	connection := lease.connection
	if !refresh {
		if tools, ok := lease.cachedDescriptors(); ok {
			return tools, nil
		}
	}
	if err := connection.lock(ctx, connection.calls, maxCalls); err != nil {
		return nil, err
	}
	defer connection.calls.Release(maxCalls)
	if !refresh {
		if tools, ok := lease.cachedDescriptors(); ok {
			return tools, nil
		}
	}
	connection.mu.Lock()
	gen := connection.descriptorsGen
	connection.mu.Unlock()
	listing, cancel := context.WithCancel(ctx)
	defer cancel()
	timer := lease.pool.clock.NewTimer(10 * time.Second)
	defer timer.Stop()
	finished := make(chan struct{})
	defer close(finished)
	go func() {
		select {
		case <-timer.Chan():
			cancel()
		case <-finished:
		case <-listing.Done():
		}
	}()
	selected := lease.config.Tools
	var selectedSet map[string]bool
	if selected != nil {
		selectedSet = make(map[string]bool, len(*selected))
		for _, name := range *selected {
			selectedSet[name] = true
		}
	}
	var all, filtered []*sdk.Tool
	names, cursors := map[string]bool{}, map[string]bool{}
	cursor := ""
	seen, allBytes, filteredBytes := 0, 0, 0
	allFits := true
	for {
		page, err := connection.session.ListTools(listing, &sdk.ListToolsParams{Cursor: cursor})
		if err != nil {
			// A transport failure, unlike a rejection or a cancelled listing,
			// ends the connection.
			var remote *jsonrpc.Error
			var responseFailure *responseReadError
			if listing.Err() == nil && (!errors.As(err, &remote) || errors.As(err, &responseFailure)) {
				connection.mu.Lock()
				connection.poisoned = true
				connection.descriptorsReady = false
				connection.descriptors = nil
				connection.descriptorsFilter = nil
				connection.descriptorsGen++
				connection.mu.Unlock()
			}
			return nil, unavailable()
		}
		for _, tool := range page.Tools {
			if tool.Name == "" || names[tool.Name] {
				return nil, unavailable()
			}
			names[tool.Name] = true
			seen++
			if seen > maxListedTools {
				return nil, unavailable()
			}
			wanted := selected == nil || selectedSet[tool.Name]
			if !wanted && !allFits {
				continue
			}
			body, err := json.Marshal(tool)
			if err != nil {
				return nil, unavailable()
			}
			if selected == nil {
				allBytes += len(body)
				if len(all) >= maxTools || allBytes > maxToolBytes {
					return nil, unavailable()
				}
				all = append(all, tool)
				continue
			}
			if allFits {
				allBytes += len(body)
				if len(all) >= maxTools || allBytes > maxToolBytes {
					allFits = false
					all = nil
				} else {
					all = append(all, tool)
				}
			}
			if wanted {
				filteredBytes += len(body)
				if len(filtered) >= maxTools || filteredBytes > maxToolBytes {
					return nil, unavailable()
				}
				filtered = append(filtered, tool)
			}
		}
		cursor = page.NextCursor
		if cursor == "" {
			break
		}
		if len(cursors) >= maxTools || cursors[cursor] {
			return nil, unavailable()
		}
		cursors[cursor] = true
	}
	connection.mu.Lock()
	if connection.descriptorsGen == gen {
		if selected == nil || allFits {
			connection.descriptors = all
			connection.descriptorsFilter = nil
		} else {
			connection.descriptors = filtered
			connection.descriptorsFilter = normalizeToolSelection(*selected)
		}
		connection.descriptorsReady = true
	}
	connection.mu.Unlock()
	if selected == nil {
		return slices.Clone(all), nil
	}
	return slices.Clone(filtered), nil
}

func (lease *Lease) Close() error {
	lease.closeOnce.Do(func() {
		pool, current := lease.pool, lease.connection
		pool.mu.Lock()
		current.leases--
		current.used = pool.clock.Now()
		current.mu.Lock()
		broken := current.poisoned || current.fault != nil
		current.mu.Unlock()
		closeNow := current.leases == 0 && (!current.reuse || current.retired || broken || pool.stopped)
		if closeNow && pool.entries[current.key] == current {
			delete(pool.entries, current.key)
		}
		pool.mu.Unlock()
		if closeNow {
			lease.closeErr = current.close()
		}
	})
	return lease.closeErr
}

func (pool *Pool) Invalidate(packageName, user string) error {
	identity := packageKey{user, packageName}
	pool.mu.Lock()
	var retired []*connection
	for key, current := range pool.entries {
		if key.User == user && (packageName == "" || key.Package == packageName) {
			delete(pool.entries, key)
			current.retired = true
			if current.leases == 0 {
				retired = append(retired, current)
			}
		}
	}
	if summary, ok := pool.current[identity]; ok {
		summary.Enabled = false
		pool.current[identity] = summary
	}
	pool.forget(packageName, user)
	pool.mu.Unlock()
	return closeConnections(retired)
}

func (pool *Pool) Expire() error {
	pool.mu.Lock()
	var retired []*connection
	for key, current := range pool.entries {
		if current.leases == 0 && pool.clock.Since(current.used) >= idleTTL {
			delete(pool.entries, key)
			current.retired = true
			retired = append(retired, current)
		}
	}
	pool.mu.Unlock()
	return closeConnections(retired)
}

func (pool *Pool) Status(reference protocol.PluginReference, server, user string) *protocol.PluginMcpRuntime {
	pool.mu.Lock()
	defer pool.mu.Unlock()
	record := pool.status[statusKey{user, reference.Name, server}]
	if record == nil || record.reference != reference || record.runtime == nil {
		return nil
	}
	status := *record.runtime
	return &status
}

func (pool *Pool) Close() error {
	pool.mu.Lock()
	pool.stopped = true
	var retired []*connection
	for current := range pool.owners {
		current.retired = true
		retired = append(retired, current)
	}
	clear(pool.entries)
	pool.mu.Unlock()
	result := closeConnections(retired)
	pool.cancel()
	pool.work.Wait()
	return result
}

func (connection *connection) close() error {
	connection.closeOnce.Do(func() {
		select {
		case <-connection.ready:
		default:
			connection.cancel()
			<-connection.ready
		}
		connection.mu.Lock()
		connection.poisoned = true
		connection.closing = true
		session := connection.session
		connection.mu.Unlock()
		if session != nil {
			connection.closeErr = cleanup(session)
		}
		connection.cancel()
		if connection.release != nil {
			connection.closeErr = errors.Join(connection.closeErr, connection.release())
		}
		connection.pool.mu.Lock()
		delete(connection.pool.owners, connection)
		serviceID := connection.key.User + "\x00" + connection.key.Package + "\x00" + connection.key.Server
		connection.exclusive.refs--
		if connection.exclusive.refs == 0 {
			delete(connection.pool.services, serviceID)
		}
		connection.pool.mu.Unlock()
		close(connection.closeDone)
	})
	<-connection.closeDone
	return connection.closeErr
}

func closeConnections(connections []*connection) error {
	var failures []error
	for _, current := range connections {
		failures = append(failures, current.close())
	}
	return errors.Join(failures...)
}

// cleanup closes a session and waits for its connection to end. Only the
// transport's own close decides the result: how the stream ended, which Wait
// would add, is an error whenever a pipe is closed from this side first.
func cleanup(session *sdk.ClientSession) error {
	if err := session.Close(); err != nil {
		return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "MCP connection cleanup could not be confirmed"}
	}
	return nil
}

func Alias(packageName, server, tool string) string {
	var input []byte
	for _, part := range []string{packageName, server, tool} {
		input = binary.LittleEndian.AppendUint64(input, uint64(len(part)))
		input = append(input, part...)
	}
	var digest [32]byte
	blake3.DeriveKey(digest[:], "Sailry MCP tool name v1", input)
	var label []byte
	for _, character := range packageName + "_" + server + "_" + tool {
		if len(label) == 32 {
			break
		}
		if character >= 'a' && character <= 'z' || character >= 'A' && character <= 'Z' || character >= '0' && character <= '9' || character == '_' || character == '-' {
			label = append(label, byte(character))
		} else {
			label = append(label, '_')
		}
	}
	return fmt.Sprintf("mcp_%s_%s", label, hex.EncodeToString(digest[:12]))
}

func invalid(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}

func cancelled() error {
	return &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "MCP invocation was cancelled"}
}

func unavailable() error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "MCP server is unavailable"}
}

func protocolFault(err error) *protocol.Fault {
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return fault
	}
	return unavailable().(*protocol.Fault)
}
