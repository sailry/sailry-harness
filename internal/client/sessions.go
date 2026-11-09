package client

import (
	"cmp"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Recent returns visible conversations in stable newest-created order.
func Recent(snapshot protocol.Snapshot) []protocol.Session {
	sessions := make([]protocol.Session, 0, len(snapshot.Sessions))
	for _, session := range snapshot.Sessions {
		if session.Archived || session.Delegation != nil {
			continue
		}
		if session.Project != nil && !slices.ContainsFunc(snapshot.Projects, func(project protocol.Project) bool { return project.ID == *session.Project }) {
			continue
		}
		sessions = append(sessions, session)
	}
	slices.SortStableFunc(sessions, func(a, b protocol.Session) int { return cmp.Compare(b.CreationOrder, a.CreationOrder) })
	return sessions
}

type Lane uint8

const (
	LaneIdle Lane = iota
	LaneRunning
	LaneWaiting
	LaneCompleted
	LaneFailed
)

func LaneOf(session protocol.Session) Lane {
	activity := session.Activity
	if activity.Run == nil {
		return LaneIdle
	}
	switch activity.Run.Status {
	case protocol.ConversationStatusStopping:
		return LaneRunning
	case protocol.ConversationStatusRunning:
		if activity.Waiting != nil {
			return LaneWaiting
		}
		return LaneRunning
	case protocol.ConversationStatusCompleted:
		if activity.Attention.Unread {
			return LaneCompleted
		}
	case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted:
		if activity.Attention.Unread {
			return LaneFailed
		}
	}
	return LaneIdle
}

// AggregateLane includes owned descendants without changing a run's status.
func AggregateLane(session protocol.Session, sessions []protocol.Session) Lane {
	own := LaneOf(session)
	if own == LaneRunning || own == LaneWaiting {
		return own
	}
	scoped := make(map[protocol.SessionID]protocol.Session)
	for _, candidate := range sessions {
		if candidate.Project == nil && session.Project == nil || candidate.Project != nil && session.Project != nil && *candidate.Project == *session.Project {
			scoped[candidate.ID] = candidate
		}
	}
	for _, child := range scoped {
		activity := LaneOf(child)
		if activity != LaneRunning && activity != LaneWaiting {
			continue
		}
		for steps := 0; steps < len(scoped) && child.Delegation != nil; steps++ {
			parent := (*child.Delegation).Session
			if parent == session.ID {
				if activity == LaneRunning {
					return LaneRunning
				}
				own = LaneWaiting
				break
			}
			var exists bool
			child, exists = scoped[parent]
			if !exists {
				break
			}
		}
	}
	return own
}
