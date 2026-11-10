package hostctl

import (
	"context"
	"errors"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type observedTurn struct {
	session protocol.SessionID
	turn    protocol.TurnID
}

func observeTask(ctx context.Context, controller *client.Client, session protocol.SessionID, turn protocol.TurnID) TaskResult {
	node := controller.Target()
	result := TaskResult{Version: 1, Node: &node, Session: &session, Turn: &turn, State: "observing", Approvals: []protocol.ConversationApproval{}, Questions: []protocol.ConversationQuestion{}}
	finishContext := func() TaskResult {
		if errors.Is(ctx.Err(), context.DeadlineExceeded) {
			result.State = "timed_out"
		} else {
			result.State = "observation_cancelled"
		}
		return result
	}
	run, _, err := locateTurn(ctx, controller, session, turn)
	if err != nil {
		if ctx.Err() != nil {
			return finishContext()
		}
		return result.uncertain(asFault(err).Message)
	}
	result.Revision = &run.Revision
	scoped, cancel := context.WithCancel(ctx)
	defer cancel()
	wake := make(chan struct{}, 1)
	changed := func() {
		select {
		case wake <- struct{}{}:
		default:
		}
	}
	observed := map[observedTurn]func(){}
	defer func() {
		for _, release := range observed {
			release()
		}
	}()
	add := func(source observedTurn) error {
		if _, exists := observed[source]; exists {
			return nil
		}
		release, err := controller.WatchConversation(scoped, source.session, changed)
		if err == nil {
			observed[source] = release
		}
		return err
	}
	if err := add(observedTurn{session, turn}); err != nil {
		return result.uncertain(asFault(err).Message)
	}
	for {
		select {
		case <-ctx.Done():
			return finishContext()
		case <-wake:
		}
		var parent *protocol.ConversationPage
		childrenReady := true
		added := false
		for source := range observed {
			state, exists := controller.Conversation(source.session)
			if !exists {
				childrenReady = false
				continue
			}
			if state.OlderErr != nil {
				return result.uncertain(asFault(state.OlderErr).Message)
			}
			if state.Err != nil && asFault(state.Err).Code != protocol.ErrorCodeUnavailable && asFault(state.Err).Code != protocol.ErrorCodeBusy {
				return result.uncertain(asFault(state.Err).Message)
			}
			if !state.Connected || state.Loading {
				childrenReady = false
				continue
			}
			page := state.Snapshot.Page
			contains := slices.ContainsFunc(page.Runs, func(run protocol.ConversationRun) bool { return run.Turn == source.turn })
			incomplete := slices.Contains(state.Snapshot.Missing, source.turn)
			if !contains || incomplete {
				if page.NextBefore != nil || len(state.Snapshot.Missing) > 0 {
					if err := controller.LoadOlderConversation(scoped, source.session); err != nil {
						if ctx.Err() != nil {
							return finishContext()
						}
						return result.uncertain(asFault(err).Message)
					}
					changed()
					childrenReady = false
					continue
				}
				return result.uncertain("turn is not in this conversation")
			}
			current := findRun(page, source.turn)
			if current != nil && !activeStatus(current.Status) {
				_, durable, err := locateTurn(scoped, controller, source.session, source.turn)
				if err != nil {
					if ctx.Err() != nil {
						return finishContext()
					}
					return result.uncertain(asFault(err).Message)
				}
				page = durable
				current = findRun(page, source.turn)
			}
			approvals, questions := pendingReviews(page, source.turn)
			if len(approvals) > 0 || len(questions) > 0 {
				result.Approvals, result.Questions = approvals, questions
				result.State = "awaiting_approval"
				if len(approvals) == 0 {
					result.State = "awaiting_input"
				}
				if source.session == session && source.turn == turn {
					result.Text = turnText(page, turn)
				}
				return result
			}
			for _, child := range page.Children {
				if child.Origin.Turn != source.turn {
					continue
				}
				key := observedTurn{child.Run.Session, child.Run.Turn}
				if _, exists := observed[key]; !exists {
					if err := add(key); err != nil {
						return result.uncertain(asFault(err).Message)
					}
					added = true
				}
				if activeStatus(child.Run.Status) {
					childrenReady = false
				}
			}
			if source.session == session && source.turn == turn {
				result.Text = turnText(page, turn)
				parent = &page
			} else if current != nil && activeStatus(current.Status) {
				childrenReady = false
			}
		}
		if added {
			changed()
			continue
		}
		if parent == nil {
			continue
		}
		if parent.Queue.Paused && slices.ContainsFunc(parent.Queue.Items, func(item protocol.ConversationPending) bool { return item.Turn == turn }) {
			result.State = "paused"
			return result
		}
		current := findRun(*parent, turn)
		if current == nil || activeStatus(current.Status) || current.Status == protocol.ConversationStatusCompleted && !childrenReady {
			continue
		}
		result.State, result.Error = string(current.Status), current.Error
		return result
	}
}

func locateTurn(ctx context.Context, controller *client.Client, session protocol.SessionID, turn protocol.TurnID) (protocol.ConversationRun, protocol.ConversationPage, error) {
	var before *protocol.TurnID
	for {
		history, err := controller.ReadConversation(ctx, session, before)
		if err != nil {
			return protocol.ConversationRun{}, protocol.ConversationPage{}, err
		}
		if run := findRun(history.Page, turn); run != nil {
			return *run, history.Page, nil
		}
		if history.Page.NextBefore == nil || before != nil && *before == *history.Page.NextBefore {
			return protocol.ConversationRun{}, protocol.ConversationPage{}, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "turn is not in this conversation"}
		}
		before = history.Page.NextBefore
	}
}

func activeStatus(status protocol.ConversationStatus) bool {
	return status == protocol.ConversationStatusQueued || status == protocol.ConversationStatusRunning || status == protocol.ConversationStatusStopping
}

func findRun(page protocol.ConversationPage, turn protocol.TurnID) *protocol.ConversationRun {
	for _, run := range page.Runs {
		if run.Turn == turn {
			return &run
		}
	}
	return nil
}

func pendingReviews(page protocol.ConversationPage, turn protocol.TurnID) ([]protocol.ConversationApproval, []protocol.ConversationQuestion) {
	approvals := []protocol.ConversationApproval{}
	questions := []protocol.ConversationQuestion{}
	for _, approval := range page.Approvals {
		if approval.Turn == turn && approval.State == protocol.ConversationApprovalStatePending {
			approvals = append(approvals, approval)
		}
	}
	for _, question := range page.Questions {
		if question.Turn == turn && question.State.Kind == "pending" {
			questions = append(questions, question)
		}
	}
	return approvals, questions
}

func turnText(page protocol.ConversationPage, turn protocol.TurnID) string {
	var result strings.Builder
	for _, entry := range page.Entries {
		if entry.Turn != turn || entry.Author == "user" {
			continue
		}
		var parts []string
		for _, part := range entry.Parts {
			if part.Kind != "text" {
				continue
			}
			text, err := payload[string](part.Data)
			if err == nil {
				parts = append(parts, text)
			}
		}
		if len(parts) > 0 {
			if result.Len() > 0 {
				result.WriteByte('\n')
			}
			result.WriteString(strings.Join(parts, ""))
		}
	}
	return result.String()
}
