package node

import "context"

// RefreshIdentity is the post-commit boundary for identity commands. In
// particular, a revoked peer loses its cached live connection immediately.
func (node *Node) RefreshIdentity(ctx context.Context) error {
	if node.network == nil {
		return nil
	}
	return node.network.RefreshPeers(ctx)
}
