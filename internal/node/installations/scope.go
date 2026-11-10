package installations

import (
	"slices"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (queue *Queue) SubscribeFor(caller protocol.NodeID, requests []protocol.RequestID, scope protocol.PluginContext) (link.Subscription, error) {
	if err := validateRequests(requests); err != nil {
		return nil, err
	}
	var captured protocol.PluginContext
	if err := decode(scope, &captured); err != nil {
		return nil, err
	}
	queue.mu.Lock()
	defer queue.mu.Unlock()
	if _, err := queue.read(caller, requests, &captured, false); err != nil {
		return nil, err
	}
	return &subscription{queue: queue, caller: caller, requests: slices.Clone(requests), plugin: &captured, initial: true, closed: make(chan struct{})}, nil
}

func validateRequests(requests []protocol.RequestID) error {
	if len(requests) == 0 || len(requests) > 64 {
		return invalid("installation request IDs must be bounded and unique")
	}
	selected := make(map[protocol.RequestID]bool, len(requests))
	for _, request := range requests {
		if selected[request] {
			return invalid("installation request IDs must be bounded and unique")
		}
		selected[request] = true
	}
	return nil
}
