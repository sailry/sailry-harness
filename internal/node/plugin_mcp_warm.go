package node

import (
	"context"
	"database/sql"
	"fmt"
	"log/slog"
	"slices"
	"sync"

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
				node.publishMCP(node.plugins.mcp.Changes())
			}
		}
	}()
	if node.plugins.warm == nil {
		return
	}
	node.work.Add(1)
	go func() {
		defer node.work.Done()
		warmed := map[string]string{}
		for {
			node.warmMCP(node.ctx, warmed)
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

func (node *Node) publishMCP(names []string) {
	var changes []protocol.Event
	err := node.profile.Store.Node.Write(node.ctx, func(db *sql.Conn) error {
		changes = changes[:0]
		for _, name := range names {
			info, err := store.ReadPlugin(node.ctx, db, name)
			if err != nil {
				return err
			}
			if info != nil {
				changes = append(changes, protocol.Event{Kind: "plugin_changed", Data: info.Summary})
			}
		}
		_, err := store.RecordEvents(node.ctx, db, node.ID(), changes)
		return err
	})
	if err != nil {
		if node.ctx.Err() == nil {
			slog.Warn("MCP runtime change was not published", "code", nodeFault(err).(*protocol.Fault).Code)
		}
		return
	}
	if len(changes) != 0 {
		node.publish()
	}
}

// warmMCP connects the servers that became startable since the last pass.
// warmed keeps, per package, the version, startable servers and accounts the
// last pass saw, so a server is tried once for each of them and a failed one
// is not retried until something about it changes.
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
		current[reference.Name] = struct{}{}
		signature := fmt.Sprint(reference, names, accounts)
		if warmed[reference.Name] == signature {
			continue
		}
		warmed[reference.Name] = signature
		for _, name := range names {
			servers.Go(func() {
				prepared, err := node.prepareMCPLaunch(ctx, info, settings, name, configuration.Servers[name])
				if err == nil {
					node.startComputer(info, settings, configuration.Servers[name], &prepared)
					var lease *mcp.Lease
					if lease, err = node.plugins.mcp.Acquire(ctx, prepared); err == nil {
						_, err = lease.List(ctx)
						_ = lease.Close()
					}
				}
				if err != nil && ctx.Err() == nil {
					slog.Warn("MCP server did not start", "package", reference.Name, "server", name, "code", nodeFault(err).(*protocol.Fault).Code)
				}
			})
		}
	}
	for name := range warmed {
		if _, exists := current[name]; !exists {
			delete(warmed, name)
		}
	}
	servers.Wait()
}
