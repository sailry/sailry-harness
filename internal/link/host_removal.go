package link

import (
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// HostRemovalAttempt retains the exact remote self-revocation request even
// after local trust is removed. It is diagnostic history, never a retry worker.
type HostRemovalAttempt struct {
	Request protocol.Request `json:"request"`
	State   string           `json:"state"`
	Fault   *protocol.Fault  `json:"fault,omitempty"`
	Active  bool             `json:"active"`
}

type HostRemovalStore interface {
	PrepareHostRemoval(context.Context, protocol.NodeID, protocol.Request) (HostRemovalAttempt, error)
	RecordHostRemoval(context.Context, protocol.NodeID, protocol.RequestID, string, *protocol.Fault) error
	HostRemovals(context.Context) ([]HostRemovalAttempt, error)
}

func (link *Link) PrepareHostRemoval(ctx context.Context, host protocol.NodeID) (HostRemovalAttempt, error) {
	store, ok := link.store.(HostRemovalStore)
	if !ok {
		return HostRemovalAttempt{}, invalidLink("peer store does not retain host removal requests")
	}
	self, err := link.Address(ctx)
	if err != nil {
		return HostRemovalAttempt{}, err
	}
	request := protocol.NewRequest(host, protocol.Command{Kind: "identity", Data: protocol.IdentityCommand{Kind: "revoke_device", Data: protocol.RevokeDevice{Device: self.ID, ConfirmLast: true}}})
	attempt, err := store.PrepareHostRemoval(ctx, host, request)
	if err != nil {
		return attempt, err
	}
	if !validHostRemoval(attempt, host, self.ID) {
		return attempt, invalidLink("stored host removal request does not match this controller")
	}
	return attempt, nil
}

func (link *Link) RecordHostRemoval(ctx context.Context, host protocol.NodeID, request protocol.RequestID, state string, failure *protocol.Fault) error {
	store, ok := link.store.(HostRemovalStore)
	if !ok {
		return invalidLink("peer store does not retain host removal requests")
	}
	return store.RecordHostRemoval(ctx, host, request, state, failure)
}

func (link *Link) HostRemovals(ctx context.Context) ([]HostRemovalAttempt, error) {
	store, ok := link.store.(HostRemovalStore)
	if !ok {
		return nil, invalidLink("peer store does not retain host removal requests")
	}
	return store.HostRemovals(ctx)
}

func ValidHostRemovalState(state string) bool {
	return state == "pending" || state == "unknown" || state == "completed" || state == "failed"
}

func validHostRemoval(attempt HostRemovalAttempt, host, self protocol.NodeID) bool {
	if !attempt.Active || !ValidHostRemovalState(attempt.State) || attempt.Request.Version != protocol.VERSION || attempt.Request.Target != host || attempt.Request.ID == (protocol.RequestID{}) || attempt.Request.Command.Kind != "identity" || attempt.Request.Plugin != nil {
		return false
	}
	var action protocol.IdentityCommand
	switch value := attempt.Request.Command.Data.(type) {
	case protocol.IdentityCommand:
		action = value
	case *protocol.IdentityCommand:
		if value == nil {
			return false
		}
		action = *value
	default:
		return false
	}
	if action.Kind != "revoke_device" {
		return false
	}
	var revoke protocol.RevokeDevice
	switch value := action.Data.(type) {
	case protocol.RevokeDevice:
		revoke = value
	case *protocol.RevokeDevice:
		if value == nil {
			return false
		}
		revoke = *value
	default:
		return false
	}
	return revoke.Device == self && revoke.ConfirmLast && !revoke.Sponsored && revoke.Generation == 0
}
