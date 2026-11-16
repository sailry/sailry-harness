package link

import (
	"context"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func cloneHostRemoval(value HostRemovalAttempt) (HostRemovalAttempt, error) {
	body, err := json.Marshal(value)
	if err != nil {
		return HostRemovalAttempt{}, err
	}
	var cloned HostRemovalAttempt
	err = json.Unmarshal(body, &cloned)
	return cloned, err
}

func (store *controllerStore) PrepareHostRemoval(ctx context.Context, host protocol.NodeID, request protocol.Request) (HostRemovalAttempt, error) {
	var result HostRemovalAttempt
	err := store.update(ctx, func(peers *[]controllerPeer) error {
		for index := range *peers {
			peer := &(*peers)[index]
			if peer.ID != host {
				continue
			}
			for _, previous := range peer.Removals {
				if previous.Active {
					result = previous
					return nil
				}
			}
			var err error
			result, err = cloneHostRemoval(HostRemovalAttempt{Request: request, State: "pending", Active: true})
			if err != nil {
				return err
			}
			peer.Removals = append(peer.Removals, result)
			return nil
		}
		return pairingRejected()
	})
	if err != nil {
		return result, err
	}
	return cloneHostRemoval(result)
}

func (store *controllerStore) RecordHostRemoval(ctx context.Context, host protocol.NodeID, request protocol.RequestID, state string, failure *protocol.Fault) error {
	if !ValidHostRemovalState(state) {
		return invalidLink("invalid host removal state")
	}
	return store.update(ctx, func(peers *[]controllerPeer) error {
		for peer := range *peers {
			if (*peers)[peer].ID != host {
				continue
			}
			for removal := range (*peers)[peer].Removals {
				value := &(*peers)[peer].Removals[removal]
				if value.Request.ID == request {
					value.State, value.Fault = state, failure
					return nil
				}
			}
		}
		return invalidLink("host removal request is unavailable")
	})
}

func (store *controllerStore) HostRemovals(ctx context.Context) ([]HostRemovalAttempt, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	store.mu.Lock()
	defer store.mu.Unlock()
	values := []HostRemovalAttempt{}
	for _, peer := range store.peers {
		for _, value := range peer.Removals {
			cloned, err := cloneHostRemoval(value)
			if err != nil {
				return nil, err
			}
			values = append(values, cloned)
		}
	}
	return values, nil
}
