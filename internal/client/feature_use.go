package client

import (
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// CountFeature counts one use of a desktop feature on this client's Node,
// which must be the application's own: a Node refuses the count from any
// other controller and for any identifier outside its closed list. The
// command carries the identifier alone.
func (client *Client) CountFeature(ctx context.Context, feature protocol.FeatureUse) error {
	_, err := client.Do(ctx, protocol.Command{Kind: "count_feature", Data: protocol.CommandCountFeaturePayload{Feature: feature}})
	return err
}

// OnDispatch calls observe with the kind of every command this client
// dispatches from now on, before it is sent. It sees no payload.
func (client *Client) OnDispatch(observe func(kind string)) {
	client.mu.Lock()
	client.dispatched = observe
	client.mu.Unlock()
}
