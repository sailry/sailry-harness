package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"log/slog"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// RefreshIdentity is the post-commit boundary for identity commands. In
// particular, a revoked peer loses its cached live connection immediately.
func (node *Node) RefreshIdentity(ctx context.Context) error {
	if node.network == nil {
		return nil
	}
	return node.network.RefreshPeers(ctx)
}

func (node *Node) startIdentitySync() error {
	return node.tasks.Submit(func(ctx context.Context) {
		ticker := time.NewTicker(5 * time.Second)
		defer ticker.Stop()
		for {
			if err := node.SyncIdentity(ctx); err != nil && ctx.Err() == nil {
				slog.Debug("Identity synchronization unavailable", "error", err)
			}
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
			}
		}
	})
}

// SyncIdentity reconciles the source's own direct hosts and durable deliveries.
// Other inbound users do not travel over this Node's single Link identity.
func (node *Node) SyncIdentity(ctx context.Context) error {
	if node.network == nil {
		return nil
	}
	self, err := node.network.Address(ctx)
	if err != nil {
		return err
	}
	peers, err := node.network.Peers(ctx)
	if err != nil {
		return err
	}
	relationships, err := node.network.Relationships(ctx)
	if err != nil {
		return err
	}
	direct := map[protocol.NodeID]bool{}
	for _, value := range relationships {
		if value.Direct && value.DirectDirection.AllowsOutbound() && !value.Removed {
			direct[value.ID] = true
		}
	}
	hosts := []link.EndpointAddress{}
	directory := []protocol.DirectoryHost{}
	for _, address := range append([]link.EndpointAddress{self}, peers...) {
		if address.ID != self.ID && (!direct[address.ID] || !node.network.Direction(address.ID).AllowsOutbound()) {
			continue
		}
		body, err := json.Marshal(address)
		if err != nil {
			return err
		}
		directory = append(directory, protocol.DirectoryHost{Node: address.ID, Address: string(body)})
		if address.ID != self.ID {
			hosts = append(hosts, address)
		}
	}
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		if err := store.SaveIdentityDirectory(ctx, db, directory); err != nil {
			return err
		}
		return store.QueueIdentitySponsors(ctx, db, node.ID(), hosts)
	}); err != nil {
		return nodeFault(err)
	}
	var deliveries []store.IdentityDelivery
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		deliveries, err = store.IdentityDeliveries(ctx, db)
		return err
	}); err != nil {
		return nodeFault(err)
	}
	for _, delivery := range deliveries {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		if err := node.deliverIdentity(ctx, delivery); err != nil && ctx.Err() != nil {
			return err
		}
	}
	// A Node may also control a my-device source. This imports only hosts and
	// confirms the source's explicit sponsorship; it never forwards business IO.
	for _, address := range peers {
		if !node.network.Direction(address.ID).AllowsOutbound() {
			continue
		}
		remote := client.New(node.network.Remote(address))
		_ = client.SyncIdentity(ctx, node.network, remote)
		_ = remote.Close()
	}
	return nil
}

func (node *Node) markIdentityDelivery(ctx context.Context, request protocol.RequestID, state string, failure *protocol.Fault) (bool, error) {
	var changed bool
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		changed, err = store.MarkIdentityDelivery(ctx, db, request, state, failure)
		return err
	})
	return changed, nodeFault(err)
}

func (node *Node) deliverIdentity(ctx context.Context, delivery store.IdentityDelivery) error {
	remote := client.New(node.network.Remote(delivery.Address))
	defer remote.Close()
	if !node.network.Direction(delivery.Address.ID).AllowsOutbound() {
		_, err := node.markIdentityDelivery(ctx, delivery.Request.ID, "blocked", &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "delivery host is no longer paired"})
		return err
	}
	if delivery.State == "unknown" {
		resolution, err := remote.Resolve(ctx, delivery.Request)
		if err != nil {
			return err
		}
		switch resolution.State {
		case client.RequestApplied:
			_, err := node.markIdentityDelivery(ctx, delivery.Request.ID, "completed", nil)
			return err
		case client.RequestFailed, client.RequestUnknown:
			_, err := node.markIdentityDelivery(ctx, delivery.Request.ID, "blocked", resolution.Fault)
			return err
		case client.RequestPending:
			return nil
		case client.RequestAbsent: // The exact original request may be sent.
		}
	}
	active, err := node.markIdentityDelivery(ctx, delivery.Request.ID, "unknown", nil)
	if err != nil || !active {
		return err
	}
	_, err = remote.Execute(ctx, delivery.Request)
	if err == nil {
		_, err = node.markIdentityDelivery(ctx, delivery.Request.ID, "completed", nil)
		return err
	}
	failure := protocol.Fault{Code: protocol.ErrorCodeInternal, Message: err.Error()}
	var linked *link.FaultError
	var direct *protocol.Fault
	if errors.As(err, &linked) {
		failure = linked.Fault
	} else if errors.As(err, &direct) {
		failure = *direct
	}
	state := "blocked"
	if failure.Code == protocol.ErrorCodeOutcomeUnknown || failure.Code == protocol.ErrorCodeUnavailable || failure.Code == protocol.ErrorCodeBusy || ctx.Err() != nil {
		state = "unknown"
	}
	_, markErr := node.markIdentityDelivery(context.WithoutCancel(ctx), delivery.Request.ID, state, &failure)
	return errors.Join(err, markErr)
}
