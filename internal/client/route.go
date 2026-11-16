package client

import "github.com/sailry/sailry-harness/internal/protocol"

// Route distinguishes a controller entry from the actual resource Node.
// Direct and local transports use the same identity for both.
func (client *Client) Route() protocol.RouteKey {
	if routed, ok := client.transport.(interface{ Route() protocol.RouteKey }); ok {
		return routed.Route()
	}
	return protocol.RouteKey{Entry: client.Target(), Target: client.Target()}
}
