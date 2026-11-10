package mcp

import (
	"context"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"sync"
	"time"

	"github.com/jonboulle/clockwork"
	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

const (
	maxConnections = 32
	idleTTL        = 30 * time.Minute
	maxTools       = 64
	maxToolBytes   = 256 * 1024
	maxMessage     = 8 * 1024 * 1024
)

type Configuration struct {
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
	Open  func(context.Context, Launch, func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error)
}

type connectionKey struct {
	Package, Digest, Server string
	Settings                uint64
	Authorization, Launch   [32]byte
}

type Pool struct {
	ctx      context.Context
	cancel   context.CancelFunc
	clock    clockwork.Clock
	open     func(context.Context, Launch, func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error)
	mu       sync.Mutex
	entries  map[connectionKey]*connection
	owners   map[*connection]struct{}
	current  map[string]protocol.PluginSummary
	services map[string]*serviceLock
	stopped  bool
	known    bool
	work     sync.WaitGroup
}

type serviceLock struct {
	lock chan struct{}
	refs int
}

type connection struct {
	pool        *Pool
	key         connectionKey
	ready       chan struct{}
	session     *sdk.ClientSession
	fault       error
	cancel      context.CancelFunc
	leases      int
	retired     bool
	used        time.Time
	reuse       bool
	exclusive   *serviceLock
	serial      chan struct{}
	mu          sync.Mutex
	active      *activeCall
	poisoned    bool
	closeOnce   sync.Once
	closeDone   chan struct{}
	closeErr    error
	release     func() error
	descriptors []*sdk.Tool
	status      protocol.PluginMcpRuntime
}

type Lease struct {
	pool       *Pool
	connection *connection
	config     Configuration
	closeOnce  sync.Once
	closeErr   error
}

func New(ctx context.Context, options Options) *Pool {
	if options.Clock == nil {
		options.Clock = clockwork.NewRealClock()
	}
	if options.Open == nil {
		options.Open = Open
	}
	lifetime, cancel := context.WithCancel(ctx)
	return &Pool{
		ctx: lifetime, cancel: cancel, clock: options.Clock, open: options.Open,
		entries: map[connectionKey]*connection{}, current: map[string]protocol.PluginSummary{},
		services: map[string]*serviceLock{}, owners: map[*connection]struct{}{},
	}
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
		Package: config.Package.Name, Digest: config.Package.Digest, Settings: config.Package.SettingsRevision,
		Server: config.Server, Authorization: blake3.Sum256(authorization), Launch: blake3.Sum256(launch),
	}, nil
}

func (pool *Pool) Observe(summary protocol.PluginSummary) error {
	pool.mu.Lock()
	previous, exists := pool.current[summary.Name]
	if exists && (previous.Revision > summary.Revision || previous.Revision == summary.Revision && previous.SettingsRevision > summary.SettingsRevision) {
		pool.mu.Unlock()
		return nil
	}
	pool.current[summary.Name] = summary
	var retired []*connection
	for key, current := range pool.entries {
		if key.Package == summary.Name && (!summary.Enabled || summary.Compatibility.Kind != "compatible" || key.Digest != summary.Digest || key.Settings != summary.SettingsRevision) {
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

func (pool *Pool) FinishInventory() {
	pool.mu.Lock()
	pool.known = true
	pool.mu.Unlock()
}

func (pool *Pool) reusable(reference protocol.PluginReference) bool {
	summary, exists := pool.current[reference.Name]
	if !exists {
		return !pool.known
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
	reuse := config.Options.Reuse && pool.reusable(config.Package)
	var retired []*connection
	for candidate, current := range pool.entries {
		expired := current.leases == 0 && pool.clock.Since(current.used) >= idleTTL
		changed := candidate.Package == key.Package && candidate.Server == key.Server &&
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
		serviceID := config.Package.Name + "\x00" + config.Server
		service := pool.services[serviceID]
		if service == nil {
			service = &serviceLock{lock: make(chan struct{}, 1)}
			pool.services[serviceID] = service
		}
		service.refs++
		opening, cancel := context.WithCancel(pool.ctx)
		current = &connection{
			pool: pool, key: key, ready: make(chan struct{}), closeDone: make(chan struct{}), cancel: cancel,
			used: pool.clock.Now(), reuse: reuse, exclusive: service, serial: make(chan struct{}, 1),
			status:  protocol.PluginMcpRuntime{State: protocol.PluginMcpRuntimeStateConnecting, UpdatedAtMs: uint64(pool.clock.Now().UnixMilli())},
			release: config.Release,
		}
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
	go func() {
		select {
		case <-timer.Chan():
			connection.cancel()
		case <-finished:
		case <-ctx.Done():
		}
	}()
	session, err := connection.pool.open(ctx, launch, connection.elicit)
	close(finished)
	timer.Stop()
	if err == nil && ctx.Err() != nil {
		err = unavailable()
		if session != nil {
			err = errors.Join(err, cleanup(session))
			session = nil
		}
	}
	connection.mu.Lock()
	connection.session = session
	connection.fault = err
	if err != nil {
		connection.status.State = protocol.PluginMcpRuntimeStateFailed
		connection.status.Fault = protocolFault(err)
	} else {
		connection.status.State = protocol.PluginMcpRuntimeStateConnected
	}
	connection.status.UpdatedAtMs = uint64(connection.pool.clock.Now().UnixMilli())
	connection.mu.Unlock()
	close(connection.ready)
}

func (lease *Lease) List(ctx context.Context) ([]*sdk.Tool, error) {
	connection := lease.connection
	if err := connection.lock(ctx, connection.serial); err != nil {
		return nil, err
	}
	defer func() { <-connection.serial }()
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
	var tools []*sdk.Tool
	names, cursors := map[string]bool{}, map[string]bool{}
	cursor := ""
	total := 0
	for {
		page, err := connection.session.ListTools(listing, &sdk.ListToolsParams{Cursor: cursor})
		if err != nil {
			return nil, unavailable()
		}
		for _, tool := range page.Tools {
			if selected := lease.config.Tools; selected != nil {
				found := false
				for _, name := range *selected {
					found = found || name == tool.Name
				}
				if !found {
					continue
				}
			}
			body, err := json.Marshal(tool)
			if err != nil {
				return nil, unavailable()
			}
			total += len(body)
			if len(tools) >= maxTools || total > maxToolBytes || tool.Name == "" || names[tool.Name] {
				return nil, unavailable()
			}
			names[tool.Name] = true
			tools = append(tools, tool)
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
	connection.descriptors = tools
	connection.status.Tools = uint32(len(tools))
	connection.status.UpdatedAtMs = uint64(lease.pool.clock.Now().UnixMilli())
	connection.mu.Unlock()
	return tools, nil
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

func (pool *Pool) Invalidate(packageName string) error {
	pool.mu.Lock()
	var retired []*connection
	for key, current := range pool.entries {
		if packageName == "" || key.Package == packageName {
			delete(pool.entries, key)
			current.retired = true
			if current.leases == 0 {
				retired = append(retired, current)
			}
		}
	}
	if summary, ok := pool.current[packageName]; ok {
		summary.Enabled = false
		pool.current[packageName] = summary
	}
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

func (pool *Pool) Status(reference protocol.PluginReference, server string) *protocol.PluginMcpRuntime {
	pool.mu.Lock()
	defer pool.mu.Unlock()
	for key, current := range pool.entries {
		if key.Package == reference.Name && key.Digest == reference.Digest && key.Settings == reference.SettingsRevision && key.Server == server {
			current.mu.Lock()
			status := current.status
			current.mu.Unlock()
			return &status
		}
	}
	return nil
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
		serviceID := connection.key.Package + "\x00" + connection.key.Server
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

func cleanup(session *sdk.ClientSession) error {
	if err := errors.Join(session.Close(), session.Wait()); err != nil {
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
