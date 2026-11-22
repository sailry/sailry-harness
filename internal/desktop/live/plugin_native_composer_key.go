package live

import (
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A binding captures one source and Host. The Node revision and delivered
// results cover providers and the package catalog. Frame-local choices,
// history publications and live connection data can change between them.
type pluginComposerKey struct {
	valid                    bool
	failure                  error
	revision, delivered      uint64
	session, bases, requests uint64
	extension                *protocol.PluginExtension
	baseExists               bool
	model                    string
	mode                     composer.Mode
	connected, delegated     bool
	saving, creating         bool
	hasMessages, sidebar     bool
	history                  *composerHistory
	connection               pluginComposerConnection
	registry                 *plugins.Registry
	contributions            uint64
	command, arguments       string
}

func (state *pluginNativeState) composerKey(revision uint64, extension *protocol.PluginExtension, view *viewmodel.Conversation, comp *composer.State, session protocol.Session, readView bool) pluginComposerKey {
	r := state.runtime
	root := r.localRuntime()
	_, baseExists := r.composerBases[comp]
	key := pluginComposerKey{
		valid: true, revision: revision, delivered: root.delivered, extension: extension,
		session: session.Revision, bases: r.composerBaseRevision, baseExists: baseExists,
		model: comp.Model, mode: comp.Mode, connected: r.controller.Connected(),
	}
	if !readView {
		return key
	}
	source := state.composer.source
	key.requests, key.delegated = r.commandRevision, delegated(session)
	key.saving, key.creating = r.composerSaving[source], r.conversationCreating[source] || r.conversationCreations[source] != nil
	key.hasMessages, key.sidebar = view != nil && len(view.Turns) > 0, r.App.NavShown()
	if state.scope.Session != nil {
		key.history = r.composerHistory(*state.scope.Session)
	}
	if r == root && key.connected {
		key.connection.route, key.connection.hasRoute = "local", true
	} else if network := root.network; network != nil {
		key.connection = network.composerConnection(r.controller.Target())
	}
	if name, arguments, found := pluginNativeLeadingCommand(comp.Draft); found {
		if registry := state.composerRegistry(); registry != nil {
			key.registry, key.contributions = registry, registry.Revision()
			key.command, key.arguments = name, arguments
		}
	}
	return key
}

type pluginComposerConnection struct {
	route            link.Route
	rttMS            uint64
	hasRoute, hasRTT bool
}

func (network *Network) composerConnection(id protocol.NodeID) pluginComposerConnection {
	network.mu.Lock()
	defer network.mu.Unlock()
	var result pluginComposerConnection
	if peer := network.peers[id]; peer != nil {
		if route := peer.connection.Route; route != nil {
			result.route, result.hasRoute = *route, true
		}
		if rtt := peer.connection.RTTMS; rtt != nil {
			result.rttMS, result.hasRTT = *rtt, true
		}
	}
	return result
}

func (connection pluginComposerConnection) projection() map[string]any {
	value := map[string]any{"route": nil, "rtt_ms": nil}
	if connection.hasRoute {
		value["route"] = connection.route
	}
	if connection.hasRTT {
		value["rtt_ms"] = connection.rttMS
	}
	return value
}
