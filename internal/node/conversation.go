package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"slices"

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
	snapshot := protocol.DefaultConversationSnapshot()
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		history, err := store.ReadConversation(ctx, db, session, nil, 20)
		if err != nil {
			return err
		}
		snapshot.Page, snapshot.Missing = history.Page, history.Missing
		statistics, statisticsErr := store.ReadAgentStatistics(ctx, db, session)
		if statisticsErr == nil {
			snapshot.Statistics = &statistics
		}
		// Account for the rest of the enclosing snapshot when fitting the page.
		snapshot.Drafts = reservation.Drafts
		overhead := snapshot
		overhead.Page = protocol.DefaultConversationPage()
		body, err := json.Marshal(overhead)
		if err != nil {
			return err
		}
		missing, err := store.FitConversationPage(&snapshot.Page, len(body))
		if err != nil {
			return err
		}
		for _, turn := range missing {
			if !slices.Contains(snapshot.Missing, turn) {
				snapshot.Missing = append(snapshot.Missing, turn)
			}
		}
		return nil
	})
	if err != nil {
		_ = reservation.Close()
		return nil, nodeFault(err)
	}
	if snapshot.Statistics == nil {
		node.agent.RequestStatisticsCommitted(session)
	}
	return reservation.Subscribe(snapshot), nil
}
