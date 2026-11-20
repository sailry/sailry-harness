package node

import (
	"context"
	"database/sql"
	"fmt"
	"log/slog"
	"slices"
	"sync"

	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// startMCP keeps MCP servers current outside a turn. One loop publishes the
// package whose server runtime changed, so a controller reads the new state.
// The other, when the pool asks for it, connects the shared servers a package
// may start: once at start and again when packages or their accounts change.
func (node *Node) startMCP() {
	node.work.Add(1)
	go func() {
		defer node.work.Done()
		for {
			select {
			case <-node.ctx.Done():
				return
			case <-node.plugins.mcp.Changed():
				node.publishMCP(node.plugins.mcp.ScopedChanges())
			}
		}
	}()
	if node.plugins.warm == nil {
		return
	}
	node.work.Add(1)
	go func() {
		defer node.work.Done()
		warmed := map[protocol.UserID]map[string]string{}
		for {
			node.warmUserMCP(node.ctx, warmed)
			select {
			case <-node.ctx.Done():
				return
			case <-node.plugins.warm:
			}
		}
	}()
}

func (node *Node) wakeMCP() {
	if node.plugins == nil || node.plugins.warm == nil {
		return
	}
	select {
	case node.plugins.warm <- struct{}{}:
	default:
	}
}

func (node *Node) publishMCP(pending []mcp.Change) {
	var users []protocol.User
	if err := node.profile.Store.Node.Read(node.ctx, func(db *sql.Conn) error { var err error; users, err = store.ListUsers(node.ctx, db); return err }); err != nil {
		return
	}
	for _, user := range users {
		if user.State != "active" {
			continue
		}
		ctx := store.WithCapturedUser(store.WithUser(node.ctx, user), user.ID)
		var changes []protocol.Event
		err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			for _, change := range pending {
				if change.User != user.ID.String() {
					continue
				}
				info, err := store.ReadPlugin(ctx, db, change.Package)
				if err != nil {
					return err
				}
				if info != nil {
					changes = append(changes, protocol.Event{Kind: "plugin_changed", Data: info.Summary})
				}
			}
			_, err := store.RecordEvents(ctx, db, node.ID(), changes)
			return err
		})
		if err != nil {
			if ctx.Err() == nil {
				slog.Warn("MCP runtime change was not published", "code", nodeFault(err).(*protocol.Fault).Code)
			}
		} else if len(changes) != 0 {
			node.publish()
		}
	}
}

func (node *Node) warmUserMCP(ctx context.Context, warmed map[protocol.UserID]map[string]string) {
	var users []protocol.User
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error { var err error; users, err = store.ListUsers(ctx, db); return err }); err != nil {
		return
	}
	active := map[protocol.UserID]bool{}
	for _, user := range users {
		if user.State != "active" {
			continue
		}
		active[user.ID] = true
		inventory := warmed[user.ID]
		if inventory == nil {
			inventory = map[string]string{}
			warmed[user.ID] = inventory
		}
		node.warmMCP(store.WithCapturedUser(store.WithUser(ctx, user), user.ID), inventory)
	}
	for user := range warmed {
		if !active[user] {
			delete(warmed, user)
		}
	}
}

// warmFailureLevel is the level of a server the warm-up could not start. A
// server that waits for its user's account or settings, such as a bundled one
// nobody connected, has not failed: only a configured server that does not
// start is a warning.
func warmFailureLevel(code protocol.ErrorCode) slog.Level {
	if code == protocol.ErrorCodeNotConfigured {
		return slog.LevelDebug
	}
	return slog.LevelWarn
}

// warmMCP connects the servers that became startable since the last pass.
// warmed keeps, per server, the package version and accounts under which it
// last connected, so a connected server is not tried again until something
// about it changes. A failed server is forgotten: the next pass tries it again
// instead of leaving it failed until the user binds its account again.
func (node *Node) warmMCP(ctx context.Context, warmed map[string]string) {
	var installed []protocol.PluginInfo
	var connections []protocol.PluginConnectorConnection
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		if installed, err = store.ReadPluginInfos(ctx, db); err != nil {
			return err
		}
		connections, err = store.Connections(ctx, db)
		return err
	})
	if err != nil {
		return
	}
	current := map[string]struct{}{}
	var servers sync.WaitGroup
	var mu sync.Mutex
	var failed []string
	for _, listed := range installed {
		if !listed.Summary.Enabled || listed.Summary.Compatibility.Kind != "compatible" || len(listed.Mcp) == 0 {
			continue
		}
		reference := plugins.PackageReference(listed)
		info, settings, configuration, err := node.capturedPluginMCP(ctx, reference, true)
		if err != nil || node.projectPluginMCP(ctx, &info) != nil {
			continue
		}
		var names []string
		for _, entry := range info.Mcp {
			server, declared := configuration.Servers[entry.Name]
			if declared && entry.Availability == protocol.PluginMcpAvailabilityAvailable && mcp.ServerOptions(server).Reuse {
				names = append(names, entry.Name)
			}
		}
		slices.Sort(names)
		var accounts []string
		for _, connection := range connections {
			if connection.Package.Name == reference.Name {
				accounts = append(accounts, fmt.Sprintf("%s/%d", connection.ID, connection.Revision))
			}
		}
		slices.Sort(accounts)
		signature := fmt.Sprint(reference, accounts)
		for _, name := range names {
			key := reference.Name + "\x00" + name
			current[key] = struct{}{}
			if warmed[key] == signature {
				continue
			}
			warmed[key] = signature
			servers.Go(func() {
				prepared, err := node.prepareMCPLaunch(ctx, info, settings, name, configuration.Servers[name])
				if err == nil {
					node.startComputer(info, configuration.Servers[name], &prepared)
					var lease *mcp.Lease
					if lease, err = node.plugins.mcp.Acquire(ctx, prepared); err == nil {
						_, err = lease.List(ctx)
						_ = lease.Close()
					}
				}
				if err != nil && ctx.Err() == nil {
					mu.Lock()
					failed = append(failed, key)
					mu.Unlock()
					code := nodeFault(err).(*protocol.Fault).Code
					slog.Log(ctx, warmFailureLevel(code), "MCP server did not start", "package", reference.Name, "server", name, "code", code)
					logging.Emit(warmFailureLevel(code), slog.String("area", "plugins"), slog.String("phase", logging.Failed), slog.String("operation", "warm_mcp"),
						slog.String("package", reference.Name), slog.String("server", name), slog.String("code", string(code)))
				}
			})
		}
	}
	servers.Wait()
	for key := range warmed {
		if _, exists := current[key]; !exists {
			delete(warmed, key)
		}
	}
	for _, key := range failed {
		delete(warmed, key)
	}
}
