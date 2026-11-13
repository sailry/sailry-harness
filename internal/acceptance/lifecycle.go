package acceptance

import (
	"context"
	"errors"
	"fmt"
	"slices"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const cleanupTimeout = 15 * time.Second

func requireOwned(directory string, node protocol.NodeID, sessions []protocol.SessionID) error {
	ledger, err := loadOwnership(directory, node)
	if err != nil {
		return err
	}
	for _, session := range sessions {
		if !ledger.owns(session) {
			return fmt.Errorf("acceptance session %s is not in the ownership ledger", session.String())
		}
	}
	return nil
}

// Archive finishes and records only the requested test-owned sessions. It uses
// an independent bounded context per session, including after caller cancellation.
// Archiving leaves canonical history available for diagnosis and replay while
// removing the fixture from the ordinary conversation list.
func Archive(ctx context.Context, controller *client.Client, directory string, sessions []protocol.SessionID) error {
	if controller == nil {
		return errors.New("acceptance cleanup requires an existing Client")
	}
	if err := requireOwned(directory, controller.Target(), sessions); err != nil {
		return err
	}
	inspect, cancel := context.WithTimeout(context.WithoutCancel(ctx), cleanupTimeout)
	recovered, recoveryErr := recoverRequests(inspect, controller, directory)
	cancel()
	sessions = append(slices.Clone(sessions), recovered...)
	ledger, err := loadOwnership(directory, controller.Target())
	if err != nil {
		return err
	}
	problems := []error{recoveryErr}
	seen := map[protocol.SessionID]bool{}
	for _, id := range sessions {
		if seen[id] {
			continue
		}
		seen[id] = true
		ledger, err = loadOwnership(directory, controller.Target())
		if err != nil {
			return errors.Join(append(problems, err)...)
		}
		if pendingFor(ledger, id) {
			problems = append(problems, fmt.Errorf("acceptance session %s has an unconfirmed request", id.String()))
			continue
		}
		scoped, cancel := context.WithTimeout(context.WithoutCancel(ctx), cleanupTimeout)
		err := archiveSession(scoped, controller, directory, id)
		cancel()
		if err != nil {
			problems = append(problems, fmt.Errorf("acceptance session %s: %w", id.String(), err))
		}
	}
	return errors.Join(problems...)
}

func archiveSession(ctx context.Context, controller *client.Client, directory string, id protocol.SessionID) error {
	if _, err := stopOwnedSession(ctx, controller, directory, id); err != nil {
		if isNotFound(err) {
			return markRemoved(directory, controller.Target(), []protocol.SessionID{id})
		}
		return err
	}
	// Archive remains reversible and keeps missing tool details in the Node.
	// Return capture failures, but still hide a confirmed idle fixture.
	captureErr := CaptureRecording(ctx, controller, directory, id)
	if err := confirmStopped(ctx, controller, id); err != nil {
		return errors.Join(captureErr, err)
	}
	session, err := readOwnedSession(ctx, controller, id)
	if err != nil {
		return err
	}
	if session.Archived {
		return captureErr
	}
	output, err := Execute(ctx, controller, directory, protocol.Command{Kind: "set_session_archived", Data: protocol.CommandSetSessionArchivedPayload{Session: id, ExpectedRevision: session.Revision, Archived: true}})
	if err != nil {
		return err
	}
	archived, err := outputAs[protocol.Session](output, "session")
	if err != nil || archived.ID != id || !archived.Archived {
		return errors.New("acceptance archive result did not confirm its session")
	}
	return captureErr
}

// Cleanup imports only explicit identities from this directory's ledger and
// previous reports. It preserves bounded recordings before revision-checked
// removal. Failed or unknown operations remain recorded for another inspection;
// no title-based discovery or user-session scan is used to grant ownership.
func Cleanup(ctx context.Context, controller *client.Client, directory string, emit func(CaseResult)) (report Report, resultErr error) {
	report = Report{Version: 1, Suite: "cleanup", Started: time.Now().UTC(), Cases: []CaseResult{}}
	defer func() { report.Finished = time.Now().UTC() }()
	if controller == nil {
		return report, errors.New("acceptance cleanup requires an existing Client")
	}
	report.Node = controller.Target()
	if err := ImportReportOwnership(directory, controller.Target()); err != nil {
		return report, err
	}
	inspect, cancel := context.WithTimeout(context.WithoutCancel(ctx), cleanupTimeout)
	_, recoveryErr := recoverRequests(inspect, controller, directory)
	cancel()
	ledger, err := loadOwnership(directory, controller.Target())
	if err != nil {
		return report, err
	}
	appendResult := func(result CaseResult) {
		report.Cases = append(report.Cases, result)
		if emit != nil {
			emit(cloneResult(result))
		}
	}
	problems := []error{recoveryErr}
	reported := map[protocol.RequestID]bool{}
	reportRequest := func(request ownershipRequest) {
		if reported[request.ID] {
			return
		}
		reported[request.ID] = true
		appendResult(CaseResult{ID: "request-" + request.ID.String(), Group: "cleanup", Title: "Inspect an unresolved acceptance request", Status: "blocked", Detail: "Request outcome remains unknown; its identity was retained", Uncertain: true, Evidence: []Evidence{{Kind: "request", Value: request.Kind + " " + request.ID.String()}}})
	}
	for _, request := range ledger.Requests {
		reportRequest(request)
	}
	for _, owned := range ledger.Sessions {
		started := time.Now()
		result := CaseResult{ID: "cleanup-" + owned.ID.String(), Group: "cleanup", Title: "Remove an owned acceptance session", Session: owned.ID.String(), OwnedSessions: []string{owned.ID.String()}, Status: "passed", Detail: "Owned session was already removed"}
		if !owned.Removed {
			var err error
			current, loadErr := loadOwnership(directory, controller.Target())
			if loadErr != nil {
				err = loadErr
			} else if slices.ContainsFunc(current.Sessions, func(value ownershipSession) bool { return value.ID == owned.ID && value.Removed }) {
				// A preceding parent removal may have included this owned child.
			} else if pendingFor(current, owned.ID) {
				err = errors.New("an unresolved request may still change this session")
			} else {
				scoped, cancel := context.WithTimeout(context.WithoutCancel(ctx), cleanupTimeout)
				err = removeOwnedSession(scoped, controller, directory, current, owned.ID)
				cancel()
			}
			if err != nil {
				result.Status, result.Detail, result.Uncertain = "blocked", safeError(err), true
				problems = append(problems, fmt.Errorf("acceptance session %s: %w", owned.ID.String(), err))
			} else {
				result.Detail = "Owned session stopped, recorded and removed"
			}
		}
		result.Duration = time.Since(started)
		appendResult(result)
	}
	if latest, err := loadOwnership(directory, controller.Target()); err != nil {
		problems = append(problems, err)
	} else {
		for _, request := range latest.Requests {
			reportRequest(request)
		}
	}
	if len(report.Cases) == 0 {
		appendResult(CaseResult{ID: "cleanup", Group: "cleanup", Title: "Clean acceptance sessions", Status: "passed", Detail: "No owned acceptance sessions remain"})
	}
	return report, errors.Join(problems...)
}

func pendingFor(ledger ownershipLedger, id protocol.SessionID) bool {
	return slices.ContainsFunc(ledger.Requests, func(request ownershipRequest) bool {
		return request.Session == nil || *request.Session == id
	})
}

func markRemoved(directory string, node protocol.NodeID, ids []protocol.SessionID) error {
	return updateOwnership(directory, node, func(ledger *ownershipLedger) error {
		for _, id := range ids {
			if !ledger.owns(id) {
				return errors.New("Node removal returned a session outside acceptance ownership")
			}
		}
		for index := range ledger.Sessions {
			if slices.Contains(ids, ledger.Sessions[index].ID) {
				ledger.Sessions[index].Removed = true
			}
		}
		return nil
	})
}

func removeOwnedSession(ctx context.Context, controller *client.Client, directory string, ledger ownershipLedger, id protocol.SessionID) error {
	if _, err := stopOwnedSession(ctx, controller, directory, id); err != nil {
		if isNotFound(err) {
			return markRemoved(directory, controller.Target(), []protocol.SessionID{id})
		}
		return err
	}
	history, truncated, err := captureHistory(ctx, controller, id)
	if err != nil {
		return err
	}
	if err := captureRecordingHistory(ctx, controller, directory, id, history, truncated); err != nil {
		return err
	}
	if truncated {
		return errors.New("acceptance history is incomplete; preserve the session for inspection")
	}
	// remove_session cascades through delegation. Never let that operation
	// silently extend this ledger's explicit authority to an unowned child.
	for _, child := range history.Page.Children {
		if !ledger.owns(child.Run.Session) {
			return errors.New("acceptance session has a child outside its ownership ledger")
		}
	}
	descendants := map[protocol.SessionID]bool{id: true}
	family := []protocol.SessionID{id}
	for _, child := range history.Page.Children {
		if !descendants[child.Run.Session] {
			descendants[child.Run.Session] = true
			family = append(family, child.Run.Session)
		}
	}
	known := controller.Snapshot().Sessions
	for changed := true; changed; {
		changed = false
		for _, candidate := range known {
			if descendants[candidate.ID] || candidate.Delegation == nil || *candidate.Delegation == nil || !descendants[(**candidate.Delegation).Session] {
				continue
			}
			if !ledger.owns(candidate.ID) {
				return errors.New("acceptance removal would include an unowned delegated session")
			}
			descendants[candidate.ID], changed = true, true
			family = append(family, candidate.ID)
		}
	}
	for index := 1; index < len(family); index++ {
		descendant := family[index]
		if _, err := stopOwnedSession(ctx, controller, directory, descendant); err != nil {
			return err
		}
		childHistory, truncated, err := captureHistory(ctx, controller, descendant)
		if err != nil {
			return err
		}
		if err := captureRecordingHistory(ctx, controller, directory, descendant, childHistory, truncated); err != nil {
			return err
		}
		if truncated {
			return errors.New("acceptance descendant history is incomplete; preserve the session for inspection")
		}
		for _, child := range childHistory.Page.Children {
			if !ledger.owns(child.Run.Session) {
				return errors.New("acceptance descendant has a child outside its ownership ledger")
			}
			if !descendants[child.Run.Session] {
				descendants[child.Run.Session] = true
				family = append(family, child.Run.Session)
			}
		}
	}
	for _, descendant := range family {
		if err := confirmStopped(ctx, controller, descendant); err != nil {
			return err
		}
	}
	session, err := readOwnedSession(ctx, controller, id)
	if err != nil {
		return err
	}
	output, err := Execute(ctx, controller, directory, protocol.Command{Kind: "remove_session", Data: protocol.CommandRemoveSessionPayload{Session: id, ExpectedRevision: session.Revision}})
	if err != nil {
		if isNotFound(err) {
			return markRemoved(directory, controller.Target(), []protocol.SessionID{id})
		}
		return err
	}
	removed, err := outputAs[[]protocol.SessionID](output, "sessions_removed")
	if err != nil || !slices.Contains(removed, id) {
		return errors.New("acceptance removal result did not confirm its session")
	}
	return markRemoved(directory, controller.Target(), removed)
}

func isNotFound(err error) bool {
	fault, ok := nodeFault(err)
	return ok && fault.Code == protocol.ErrorCodeNotFound
}

func readOwnedSession(ctx context.Context, controller *client.Client, id protocol.SessionID) (protocol.Session, error) {
	output, err := controller.Do(ctx, protocol.Command{Kind: "read_session", Data: protocol.CommandReadSessionPayload{Session: id}})
	if err != nil {
		return protocol.Session{}, err
	}
	session, err := outputAs[protocol.Session](output, "session")
	if err == nil && session.ID != id {
		err = errors.New("acceptance session read returned another identity")
	}
	return session, err
}

func readOwnedHistory(ctx context.Context, controller *client.Client, id protocol.SessionID) (protocol.ConversationHistory, error) {
	output, err := controller.Do(ctx, protocol.Command{Kind: "read_conversation", Data: protocol.CommandReadConversationPayload{Session: id, Limit: 100}})
	if err != nil {
		return protocol.ConversationHistory{}, err
	}
	history, err := outputAs[protocol.ConversationHistory](output, "conversation")
	if err == nil && history.Page.Session != id {
		err = errors.New("acceptance history belongs to another session")
	}
	return history, err
}

func confirmStopped(ctx context.Context, controller *client.Client, id protocol.SessionID) error {
	history, err := readOwnedHistory(ctx, controller, id)
	if err != nil {
		return err
	}
	if len(history.Page.Queue.Items) > 0 || slices.ContainsFunc(history.Page.Runs, func(run protocol.ConversationRun) bool { return active(run.Status) }) {
		return errors.New("acceptance session started another turn before cleanup completed")
	}
	output, err := controller.Do(ctx, protocol.Command{Kind: "list_commands", Data: protocol.CommandListCommandsPayload{Session: id}})
	if err != nil {
		return err
	}
	processes, err := outputAs[[]protocol.ProcessInfo](output, "commands")
	if err != nil {
		return err
	}
	for _, process := range processes {
		if process.Session != id || process.Status.Kind == "running" || process.Status.Kind == "stopping" {
			return errors.New("acceptance process cessation is not confirmed")
		}
	}
	return nil
}

func stopOwnedSession(ctx context.Context, controller *client.Client, directory string, id protocol.SessionID) (protocol.ConversationHistory, error) {
	if err := requireOwned(directory, controller.Target(), []protocol.SessionID{id}); err != nil {
		return protocol.ConversationHistory{}, err
	}
	if _, err := readOwnedSession(ctx, controller, id); err != nil {
		return protocol.ConversationHistory{}, err
	}
	if _, err := Execute(ctx, controller, directory, protocol.Command{Kind: "stop_commands", Data: protocol.CommandStopCommandsPayload{Session: id}}); err != nil {
		return protocol.ConversationHistory{}, err
	}
	stopped := map[protocol.TurnID]bool{}
	ticker := time.NewTicker(100 * time.Millisecond)
	defer ticker.Stop()
	var history protocol.ConversationHistory
	for {
		var err error
		history, err = readOwnedHistory(ctx, controller, id)
		if err != nil {
			return history, err
		}
		turns := []protocol.TurnID{}
		for _, run := range history.Page.Runs {
			if run.Session != id {
				return history, errors.New("acceptance cleanup encountered another session's turn")
			}
			if active(run.Status) {
				if run.Origin != nil {
					if err := requireOwned(directory, controller.Target(), []protocol.SessionID{*run.Origin}); err != nil {
						return history, errors.New("acceptance cleanup would stop an unowned source turn")
					}
				}
				turns = append(turns, run.Turn)
			}
		}
		for _, queued := range history.Page.Queue.Items {
			turns = append(turns, queued.Turn)
		}
		if len(turns) == 0 {
			break
		}
		for _, turn := range turns {
			if stopped[turn] {
				continue
			}
			stopped[turn] = true
			if _, err := Execute(ctx, controller, directory, protocol.Command{Kind: "stop_turn", Data: protocol.CommandStopTurnPayload{Turn: turn}}); err != nil {
				return history, err
			}
		}
		select {
		case <-ctx.Done():
			return history, ctx.Err()
		case <-ticker.C:
		}
	}
	// A turn may have started a process after the first stop. This second stop
	// is sent only after the first has a confirmed outcome and all turns ended.
	if _, err := Execute(ctx, controller, directory, protocol.Command{Kind: "stop_commands", Data: protocol.CommandStopCommandsPayload{Session: id}}); err != nil {
		return history, err
	}
	for {
		output, err := controller.Do(ctx, protocol.Command{Kind: "list_commands", Data: protocol.CommandListCommandsPayload{Session: id}})
		if err != nil {
			return history, err
		}
		processes, err := outputAs[[]protocol.ProcessInfo](output, "commands")
		if err != nil {
			return history, err
		}
		running := false
		for _, process := range processes {
			if process.Session != id {
				return history, errors.New("acceptance cleanup encountered another session's process")
			}
			running = running || process.Status.Kind == "running" || process.Status.Kind == "stopping"
		}
		if !running {
			return history, nil
		}
		select {
		case <-ctx.Done():
			return history, ctx.Err()
		case <-ticker.C:
		}
	}
}
