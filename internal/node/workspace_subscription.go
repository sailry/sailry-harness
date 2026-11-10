package node

import (
	"context"
	"sync"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type commandEvents struct {
	node    *Node
	session protocol.SessionID
	updates <-chan []protocol.ProcessInfo
	release func()
	closed  chan struct{}
	once    sync.Once
}

func (s *commandEvents) Close() error { s.once.Do(func() { close(s.closed); s.release() }); return nil }
func (s *commandEvents) Next(ctx context.Context) (protocol.Update, error) {
	select {
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	case <-s.closed:
		return protocol.Update{}, unavailable()
	case <-s.node.closed:
		return protocol.Update{}, unavailable()
	case values, ok := <-s.updates:
		if !ok {
			return protocol.Update{}, unavailable()
		}
		return protocol.Update{Kind: "commands", Data: protocol.UpdateCommandsPayload{Node: s.node.ID(), Session: s.session, Items: values}}, nil
	}
}
func (node *Node) subscribeWorkspace(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	if err := node.begin(); err != nil {
		return nil, err
	}
	defer node.work.Done()
	if topic.Kind == "commands" {
		session, ok := topic.Data.(protocol.SessionID)
		if !ok {
			if value, yes := topic.Data.(*protocol.SessionID); yes && value != nil {
				session = *value
				ok = true
			}
		}
		if !ok {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "command session is required"}
		}
		if err := node.requireWorkspaceSession(ctx, session); err != nil {
			return nil, err
		}
		updates, release := node.processes.Subscribe(session)
		return &commandEvents{node: node, session: session, updates: updates, release: release, closed: make(chan struct{})}, nil
	}
	id, ok := topic.Data.(protocol.WorktreeID)
	if !ok {
		if value, yes := topic.Data.(*protocol.WorktreeID); yes && value != nil {
			id = *value
			ok = true
		}
	}
	if !ok {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "workspace identity is required"}
	}
	root, err := node.workspaceRoot(ctx, id, false)
	if err != nil {
		return nil, err
	}
	return node.watches.subscribe(ctx, root, node.ID(), id, topic.Kind == "repository")
}
