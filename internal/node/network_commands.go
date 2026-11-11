package node

import (
	"context"
	"database/sql"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func networkCommand(kind string) bool {
	switch kind {
	case "read_network_settings", "save_network_settings", "list_network_presets", "probe_network":
		return true
	}
	return false
}

func (node *Node) readNetwork(ctx context.Context) (protocol.NetworkSettings, error) {
	if node.network == nil {
		return protocol.NetworkSettings{}, unavailable()
	}
	settings := protocol.DefaultNetworkSettings()
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		settings.Revision, settings.Selection, err = store.ReadNetworkSelection(ctx, db)
		return err
	})
	if err != nil {
		return settings, err
	}
	settings.Effective = node.network.NetworkSelection()
	if settings.Selection == nil {
		settings.Selection = settings.Effective
	}
	address, err := node.network.Address(ctx)
	if err != nil {
		return settings, err
	}
	body, err := json.Marshal(address)
	settings.Address = string(body)
	return settings, err
}

func (node *Node) networkIO(ctx context.Context, command protocol.Command) (protocol.Output, error) {
	switch command.Kind {
	case "list_network_presets":
		input := command.Data.(*protocol.CommandListNetworkPresetsPayload)
		value, err := node.networkDirectory.read(ctx, input.Refresh)
		return protocol.Output{Kind: "network_presets", Data: value}, err
	case "probe_network":
		input := command.Data.(*protocol.CommandProbeNetworkPayload)
		value, err := node.networkProbes.read(ctx, input.Selection)
		return protocol.Output{Kind: "network_latency", Data: value}, err
	case "read_network_settings":
		value, err := node.readNetwork(ctx)
		return protocol.Output{Kind: "network_settings", Data: value}, err
	case "save_network_settings":
		if node.network == nil {
			return protocol.Output{}, unavailable()
		}
		if node.network.NetworkSelection() == nil {
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "direct-only endpoint has no relay transport"}
		}
		input := command.Data.(*protocol.CommandSaveNetworkSettingsPayload)
		if _, err := link.NetworkRelayURLs(ctx, input.Selection); err != nil {
			return protocol.Output{}, err
		}
		if !node.networkUpdate.TryLock() {
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "network configuration is changing"}
		}
		defer node.networkUpdate.Unlock()
		if err := node.saveNetwork(ctx, input.ExpectedRevision, *node.network.NetworkSelection(), input.Selection, node.network.SelectNetwork); err != nil {
			return protocol.Output{}, err
		}
		value, err := node.readNetwork(ctx)
		return protocol.Output{Kind: "network_settings", Data: value}, err
	}
	return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "network command expected"}
}

// saveNetwork makes a selection effective and then stores it, so a selection
// that could not be applied is never the one the next start uses. The caller
// holds networkUpdate, the only writer of the stored selection, so the
// revision checked before the relays change is the one the store checks
// again. A selection that was applied but not stored is taken back; when the
// commit itself could not be confirmed the effective selection stays, because
// the store may hold it.
func (node *Node) saveNetwork(ctx context.Context, expected uint64, previous, selection protocol.NetworkSelection, apply func(context.Context, protocol.NetworkSelection) error) error {
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		revision, _, err := store.ReadNetworkSelection(ctx, db)
		if err == nil && revision != expected {
			err = &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "configuration revision changed; reload before editing"}
		}
		return err
	})
	if err != nil {
		return err
	}
	if err := apply(ctx, selection); err != nil {
		return err
	}
	err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		_, err := store.SaveNetworkSelection(ctx, db, expected, selection)
		return err
	})
	if err == nil || nodeFault(err).(*protocol.Fault).Code == protocol.ErrorCodeOutcomeUnknown {
		return err
	}
	if restored := apply(context.WithoutCancel(ctx), previous); restored != nil {
		return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "network selection was applied but could not be stored or taken back; read the network settings before retrying"}
	}
	return err
}
