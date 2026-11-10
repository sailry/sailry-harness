package link

import "context"

func (link *Link) WatchPeers(ctx context.Context, callback func()) error {
	if callback == nil {
		return invalidLink("peer observer callback is required")
	}
	for {
		link.mu.Lock()
		if link.peerChanged == nil {
			link.peerChanged = make(chan struct{})
		}
		changed := link.peerChanged
		link.mu.Unlock()
		callback()
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-link.ctx.Done():
			return linkNetwork(link.ctx.Err())
		case <-changed:
		}
	}
}

func (link *Link) notifyPeers() {
	link.mu.Lock()
	if link.peerChanged != nil {
		close(link.peerChanged)
		link.peerChanged = make(chan struct{})
	}
	link.mu.Unlock()
}
