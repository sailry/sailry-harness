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
	lane := runLane(session.Activity)
	// Background work outlives its turn: the session keeps running until it
	// finishes, rather than showing the turn's outcome while it still works.
	if session.Activity.Background > 0 && lane != LaneWaiting {
		return LaneRunning
	}
	return lane
}

func runLane(activity protocol.ActivitySummary) Lane {
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

// AggregateLanes returns the display lane of each catalog session in input order.
func AggregateLanes(sessions []protocol.Session) []Lane {
	const (
		background = 1 + iota
		waiting
		running
	)
	lanes := make([]Lane, len(sessions))
	var indices map[protocol.SessionID]int
	var descendants []uint8
	for i := range sessions {
		s := &sessions[i]
		own := runLane(s.Activity)
		lanes[i] = own
		if s.Activity.Background > 0 && own != LaneWaiting {
			lanes[i] = LaneRunning
		}
		if s.Delegation == nil {
			continue
		}
		var strength uint8
		switch {
		case own == LaneRunning:
			strength = running
		case own == LaneWaiting:
			strength = waiting
		case s.Activity.Background > 0:
			strength = background
		default:
			continue
		}
		if indices == nil {
			indices = make(map[protocol.SessionID]int, len(sessions))
			for j := range sessions {
				indices[sessions[j].ID] = j
			}
			descendants = make([]uint8, len(sessions))
		}
		// A stronger signal has already reached every ancestor above this one.
		// Each edge is visited at most three times successfully, even in a cycle.
		for s.Delegation != nil {
			parent, ok := indices[(*s.Delegation).Session]
			if !ok || descendants[parent] >= strength {
				break
			}
			project := sessions[parent].Project
			if s.Project != project && (s.Project == nil || project == nil || *s.Project != *project) {
				break
			}
			descendants[parent] = strength
			s = &sessions[parent]
		}
	}
	for i, strength := range descendants {
		if strength == 0 {
			continue
		}
		if own := runLane(sessions[i].Activity); own == LaneRunning || own == LaneWaiting {
			continue
		}
		if strength == waiting {
			lanes[i] = LaneWaiting
		} else {
			lanes[i] = LaneRunning
		}
	}
	return lanes
}
