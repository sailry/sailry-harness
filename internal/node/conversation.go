package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) conversationEvents(changes []protocol.Event) {
	node.externalBrowserEvents(changes)
	node.processSessionEvents(changes, node.removeSessionWorkspaces)
	node.removedSessionFiles(changes)
	sessions := map[protocol.SessionID]bool{}
	refreshStatistics := false
	for _, change := range changes {
		switch change.Kind {
		case "provider_changed", "provider_removed", "compaction_settings_changed", "model_catalog_changed":
			refreshStatistics = true
		}
		switch v := change.Data.(type) {
		case protocol.Session:
			sessions[v.ID] = true
		case *protocol.Session:
			sessions[v.ID] = true
		case protocol.EventConversationChangedPayload:
			sessions[v.Session] = true
		case protocol.EventTurnQueuedPayload:
			sessions[v.Turn.Session] = true
		case protocol.EventTaskStartedPayload:
			sessions[v.Turn.Session] = true
		case protocol.EventSessionRewoundPayload:
			if v.Session != nil {
				node.feed.Reset(v.Session.ID)
				sessions[v.Session.ID] = true
			}
		}
	}
	for session := range sessions {
		node.agent.RefreshCommitted(session)
	}
	if refreshStatistics {
		for _, session := range node.feed.Sessions() {
			if !sessions[session] {
				node.agent.RefreshStatisticsCommitted(session)
			}
		}
	}
}
func (node *Node) subscribeConversation(ctx context.Context, session protocol.SessionID) (link.Subscription, error) {
	if err := node.begin(); err != nil {
		return nil, err
	}
	defer node.work.Done()
	unlock := node.agent.LockPublication()
	defer unlock()
	reservation := node.feed.Reserve(session, node.closed)
	var snapshot protocol.ConversationSnapshot
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		snapshot, err = store.ReadConversationSnapshot(ctx, db, session, 20, reservation.Drafts)
		return err
	})
	if err != nil {
		_ = reservation.Close()
		return nil, nodeFault(err)
	}
	// The reservation receives the statistics frame even if it is ready before
	// the controller reads its history. Aggregate outside the publication lock.
	node.agent.RequestStatisticsCommitted(session)
	return reservation.Subscribe(snapshot), nil
}
