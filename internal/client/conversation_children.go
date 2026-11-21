package client

import (
	"cmp"
	"context"
	"reflect"
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
	owner, exists := conversationChildInventory(snapshot, parent)
	if !exists || !registeredChildWorktree(snapshot, owner.Project, child.Run.Worktree) {
		return protocol.Session{}, false
	}
	session, exists := conversationChildInventory(snapshot, child.Run.Session)
	origin := sessionDelegation(session)
	if !exists || session.Worktree != child.Run.Worktree || !equalChildProject(session.Project, owner.Project) ||
		origin == nil || origin.Session != parent || !reflect.DeepEqual(origin.Role, child.Origin.Role) {
		return protocol.Session{}, false
	}
	return session, true
}

func conversationChildInventory(snapshot protocol.Snapshot, id protocol.SessionID) (protocol.Session, bool) {
	for _, session := range snapshot.Sessions {
		if session.ID == id {
			return session, true
		}
	}
	return protocol.Session{}, false
}

func equalChildProject(left, right *protocol.ProjectID) bool {
	return left == nil && right == nil || left != nil && right != nil && *left == *right
}

func registeredChildWorktree(snapshot protocol.Snapshot, project *protocol.ProjectID, id protocol.WorktreeID) bool {
	return slices.ContainsFunc(snapshot.Worktrees, func(worktree protocol.Worktree) bool {
		return worktree.ID == id && equalChildProject(worktree.Project, project)
	})
}

func sessionDelegation(session protocol.Session) *protocol.Delegation {
	if session.Delegation == nil {
		return nil
	}
	return *session.Delegation
}

// ConversationChildTarget joins a validated conversation with the same Node's
// session inventory. A canonical child can precede its inventory event. A
// background candidate can precede or outlive the tail containing its call;
// its origin must be confirmed before admitting a child read or action.
type ConversationChildTarget struct {
	Child      protocol.ConversationChild
	Session    protocol.Session
	Listed     bool
	Background *protocol.ConversationBackgroundTask
}

// MatchesSession checks the scope of an authenticated receipt or a later
// inventory entry, including the founding call rather than a reused run's call.
func (target ConversationChildTarget) MatchesSession(session protocol.Session) bool {
	founding := sessionDelegation(target.Session)
	return founding != nil && session.ID == target.Child.Run.Session && session.Fork == nil &&
		session.Worktree == target.Child.Run.Worktree && equalChildProject(session.Project, target.Session.Project) &&
		session.Revision >= target.Child.Run.Revision && reflect.DeepEqual(sessionDelegation(session), founding)
}

// CheckScope rechecks the captured project, worktree and inventory after a
// read. A canonical receipt can bridge an unlisted child, never a conflict.
func (target ConversationChildTarget) CheckScope(snapshot protocol.Snapshot, node protocol.NodeID) (listed, valid bool) {
	founding := sessionDelegation(target.Session)
	if snapshot.Node != node || founding == nil {
		return false, false
	}
	parent, exists := conversationChildInventory(snapshot, founding.Session)
	if !exists || parent.Delegation != nil || !equalChildProject(parent.Project, target.Session.Project) ||
		!registeredChildWorktree(snapshot, parent.Project, target.Child.Run.Worktree) {
		return false, false
	}
	if session, listed := conversationChildInventory(snapshot, target.Child.Run.Session); listed {
		return true, target.MatchesSession(session)
	}
	return false, !target.Listed
}

func conversationChildOwner(snapshot protocol.Snapshot, conversation protocol.ConversationSnapshot) (protocol.Session, bool) {
	if snapshot.Node == (protocol.NodeID{}) || snapshot.Node != conversation.Node || conversation.Page.Revision == 0 {
		return protocol.Session{}, false
	}
	parent, exists := conversationChildInventory(snapshot, conversation.Page.Session)
	return parent, exists && parent.Delegation == nil
}

func ConversationChildTargets(snapshot protocol.Snapshot, conversation protocol.ConversationSnapshot) []ConversationChildTarget {
	page := conversation.Page
	if len(page.Children) == 0 && len(page.Background) == 0 {
		return nil
	}
	parent, ok := conversationChildOwner(snapshot, conversation)
	if !ok {
		return nil
	}
	var result []ConversationChildTarget
	claimed := map[protocol.SessionID]bool{}
	for _, child := range LatestConversationChildren(page.Children) {
		id := child.Run.Session
		claimed[id] = true
		if child.Origin.Session != parent.ID || !validChildTargetRun(child.Run, parent.ID) ||
			!registeredChildWorktree(snapshot, parent.Project, child.Run.Worktree) {
			continue
		}
		session, listed := conversationChildInventory(snapshot, id)
		if listed {
			origin := sessionDelegation(session)
			if origin == nil || origin.Session != parent.ID || origin.Turn == (protocol.TurnID{}) || origin.Entry == "" ||
				!reflect.DeepEqual(origin.Role, child.Origin.Role) ||
				origin.Turn == child.Origin.Turn && origin.Entry == child.Origin.Entry && origin.Index == child.Origin.Index && !reflect.DeepEqual(origin, &child.Origin) {
				continue
			}
		} else {
			// This scope comes from the canonical call, not from the clicked ID.
			session = protocol.Session{ID: id, Worktree: child.Run.Worktree, Project: parent.Project, Delegation: new(&child.Origin)}
		}
		target := ConversationChildTarget{Child: child, Session: session, Listed: listed}
		if !listed || equalChildProject(session.Project, parent.Project) && target.MatchesSession(session) {
			result = append(result, target)
		}
	}
	for _, edge := range page.Background {
		if edge.Kind.Kind != "agent" || edge.State != protocol.ConversationBackgroundStateRunning || edge.Session != parent.ID ||
			edge.Turn == (protocol.TurnID{}) || edge.Revision == 0 || edge.Outcome != nil || edge.FinishedMs != nil {
			continue
		}
		ref, err := payload[protocol.ConversationBackgroundKindAgentPayload](edge.Kind.Data)
		if err != nil || edge.ID != "agent:"+ref.Turn.String() || claimed[ref.Session] {
			continue
		}
		session, listed := conversationChildInventory(snapshot, ref.Session)
		origin, run := sessionDelegation(session), session.Activity.Run
		if !listed || origin == nil || origin.Session != parent.ID || origin.Turn == (protocol.TurnID{}) || origin.Entry == "" ||
			run == nil || run.Session != ref.Session || run.Turn != ref.Turn || !validChildTargetRun(*run, parent.ID) ||
			!activeConversationRun(run.Status) || run.FinishedMs != nil || !equalChildProject(session.Project, parent.Project) ||
			!registeredChildWorktree(snapshot, parent.Project, run.Worktree) ||
			!conversationChildCallOmitted(conversation, edge.Turn) {
			continue
		}
		if edge.Turn == origin.Turn && slices.ContainsFunc(page.Entries, func(entry protocol.ConversationEntry) bool { return entry.ID == origin.Entry }) {
			// Loaded canonical evidence wins, including a call not admitting
			// this child. Do not replace a disagreement with inventory.
			continue
		}
		target := ConversationChildTarget{Child: protocol.ConversationChild{Run: *run, Origin: *origin}, Session: session, Listed: true, Background: &edge}
		if target.MatchesSession(session) {
			claimed[ref.Session] = true
			result = append(result, target)
		}
	}
	slices.SortFunc(result, func(left, right ConversationChildTarget) int {
		return cmp.Compare(left.Child.Run.Sequence, right.Child.Run.Sequence)
	})
	return result
}

// ConversationChildOpenTarget can also use a known founding delegation while
// its conversation publication is in flight. That inventory-only target has
// no run until ConfirmConversationChild proves the exact call.
func ConversationChildOpenTarget(snapshot protocol.Snapshot, conversation protocol.ConversationSnapshot, id protocol.SessionID) (ConversationChildTarget, bool) {
	for _, target := range ConversationChildTargets(snapshot, conversation) {
		if target.Session.ID == id {
			return target, true
		}
	}
	parent, ok := conversationChildOwner(snapshot, conversation)
	session, listed := conversationChildInventory(snapshot, id)
	founding := sessionDelegation(session)
	if !ok || !listed || id == parent.ID || id == (protocol.SessionID{}) || session.Fork != nil || session.Revision == 0 || session.Worktree == (protocol.WorktreeID{}) ||
		founding == nil || founding.Session != parent.ID || founding.Turn == (protocol.TurnID{}) || founding.Entry == "" ||
		!equalChildProject(session.Project, parent.Project) || !registeredChildWorktree(snapshot, parent.Project, session.Worktree) {
		return ConversationChildTarget{}, false
	}
	if slices.ContainsFunc(conversation.Page.Children, func(child protocol.ConversationChild) bool { return child.Run.Session == id }) {
		return ConversationChildTarget{}, false
	}
	for _, edge := range conversation.Page.Background {
		if edge.Kind.Kind == "agent" {
			if ref, err := payload[protocol.ConversationBackgroundKindAgentPayload](edge.Kind.Data); err == nil && ref.Session == id {
				return ConversationChildTarget{}, false
			}
		}
	}
	return ConversationChildTarget{Session: session, Listed: true, Child: protocol.ConversationChild{
		Origin: *founding, Run: protocol.ConversationRun{Session: id, Worktree: session.Worktree},
	}}, true
}

func validChildTargetRun(run protocol.ConversationRun, parent protocol.SessionID) bool {
	return run.Session != (protocol.SessionID{}) && run.Session != parent && run.Turn != (protocol.TurnID{}) &&
		run.Worktree != (protocol.WorktreeID{}) && run.Kind == protocol.ConversationRunKindTask && run.Sequence != 0 &&
		run.Revision != 0 && run.Origin == nil && run.Joined == nil
}

func conversationChildCallOmitted(conversation protocol.ConversationSnapshot, turn protocol.TurnID) bool {
	for _, run := range conversation.Page.Runs {
		if run.Turn == turn {
			return run.Kind == protocol.ConversationRunKindTask && run.Origin == nil && run.Joined == nil && slices.Contains(conversation.Missing, turn)
		}
	}
	return conversation.Page.NextBefore != nil
}

// ConfirmConversationChild reads only the owning turn(s) at the captured
// history revision. It does not merge them into the parent's displayed history.
func (c *Client) ConfirmConversationChild(ctx context.Context, node protocol.NodeID, revision uint64, target ConversationChildTarget) (protocol.ConversationChild, error) {
	founding := sessionDelegation(target.Session)
	if node != c.Target() || revision == 0 || !target.Listed || founding == nil {
		return protocol.ConversationChild{}, fault(protocol.ErrorCodeWrongTarget, "child history proof has no matching scope")
	}
	if target.Background == nil {
		return c.readChildOrigin(ctx, founding.Session, founding.Turn, revision, target, founding, nil)
	}
	parent, current := founding.Session, target.Background.Turn
	if founding.Turn != current {
		if _, err := c.readChildOrigin(ctx, parent, founding.Turn, revision, target, founding, nil); err != nil {
			return protocol.ConversationChild{}, err
		}
		founding = nil
	}
	return c.readChildOrigin(ctx, parent, current, revision, target, founding, &target.Child.Run)
}

func (c *Client) readChildOrigin(ctx context.Context, parent protocol.SessionID, turn protocol.TurnID, revision uint64, target ConversationChildTarget, founding *protocol.Delegation, run *protocol.ConversationRun) (protocol.ConversationChild, error) {
	var child protocol.ConversationChild
	var before *uint64
	for {
		history, err := c.readConversationTurn(ctx, parent, turn, revision, before, turnHistoryPageLimit)
		if err != nil {
			return child, err
		}
		if history.Revision != revision || history.Run.Origin != nil || history.Run.Kind != protocol.ConversationRunKindTask || history.Run.Joined != nil {
			return child, fault(protocol.ErrorCodeWrongTarget, "child history does not match its parent turn and revision")
		}
		for _, candidate := range history.Children {
			if candidate.Run.Session != target.Session.ID || candidate.Run.Worktree != target.Session.Worktree ||
				candidate.Origin.Session != parent || !reflect.DeepEqual(candidate.Origin.Role, target.Child.Origin.Role) {
				continue
			}
			if founding != nil && reflect.DeepEqual(&candidate.Origin, founding) && candidate.Run.Joined == nil {
				founding = nil
				if run == nil {
					child = candidate
				}
			}
			if run != nil && candidate.Run.Joined == nil && sameRun(candidate.Run, *run) {
				child = candidate
			}
		}
		if founding != nil && slices.ContainsFunc(history.Entries, func(entry protocol.ConversationEntry) bool { return entry.ID == founding.Entry }) {
			return child, fault(protocol.ErrorCodeNotFound, "child founding call does not match its canonical delegation")
		}
		if founding == nil && (run == nil || child.Run.Turn == run.Turn) {
			return child, nil
		}
		if history.NextBefore == nil {
			return child, fault(protocol.ErrorCodeNotFound, "child has no matching canonical delegation call")
		}
		before = history.NextBefore
	}
}
