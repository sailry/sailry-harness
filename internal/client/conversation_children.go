package client

import (
	"cmp"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func LatestConversationChildren(children []protocol.ConversationChild) []protocol.ConversationChild {
	selected := make(map[protocol.SessionID]protocol.ConversationChild)
	for _, child := range children {
		if child.Run.Joined != nil {
			continue
		}
		previous, exists := selected[child.Run.Session]
		if !exists || compareConversationChild(child, previous) > 0 {
			selected[child.Run.Session] = child
		}
	}
	result := make([]protocol.ConversationChild, 0, len(selected))
	for _, child := range selected {
		result = append(result, child)
	}
	slices.SortFunc(result, func(left, right protocol.ConversationChild) int {
		return cmp.Compare(left.Run.Sequence, right.Run.Sequence)
	})
	return result
}

func compareConversationChild(left, right protocol.ConversationChild) int {
	leftActive := left.Run.Status == "running" || left.Run.Status == "stopping"
	rightActive := right.Run.Status == "running" || right.Run.Status == "stopping"
	if leftActive != rightActive {
		if leftActive {
			return 1
		}
		return -1
	}
	if (left.Run.Status == "queued") != (right.Run.Status == "queued") {
		if left.Run.Status != "queued" {
			return 1
		}
		return -1
	}
	return cmp.Compare(left.Run.Sequence, right.Run.Sequence)
}

func ConversationChildSession(snapshot protocol.Snapshot, parent protocol.SessionID, child protocol.ConversationChild) (protocol.Session, bool) {
	if child.Origin.Session != parent {
		return protocol.Session{}, false
	}
	parentIndex := slices.IndexFunc(snapshot.Sessions, func(session protocol.Session) bool { return session.ID == parent })
	if parentIndex < 0 {
		return protocol.Session{}, false
	}
	project := snapshot.Sessions[parentIndex].Project
	for _, session := range snapshot.Sessions {
		if session.ID != child.Run.Session || session.Worktree != child.Run.Worktree || (session.Project == nil) != (project == nil) || project != nil && *session.Project != *project {
			continue
		}
		if session.Delegation == nil || *session.Delegation == nil {
			continue
		}
		origin := *session.Delegation
		if origin.Session != parent || (origin.Role == nil) != (child.Origin.Role == nil) || origin.Role != nil && *origin.Role != *child.Origin.Role {
			continue
		}
		registered := slices.ContainsFunc(snapshot.Worktrees, func(worktree protocol.Worktree) bool {
			return worktree.ID == session.Worktree && (worktree.Project == nil) == (project == nil) && (project == nil || *worktree.Project == *project)
		})
		if registered {
			return session, true
		}
	}
	return protocol.Session{}, false
}
