package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"

	dispatcher "github.com/sailry/sailry-harness/internal/node/dispatch"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func listDispatchJobs(ctx context.Context, db *sql.Conn, packageName string, before *uint64, limit uint16) (protocol.DispatchPage, error) {
	if limit < 1 || limit > 100 || before != nil && (*before == 0 || *before > math.MaxInt64) {
		return protocol.DispatchPage{}, invalid("invalid dispatch page bounds")
	}
	cursor := uint64(math.MaxInt64)
	if before != nil {
		cursor = *before
	}
	rows, err := db.QueryContext(ctx, "SELECT sequence,body FROM dispatch_jobs WHERE package=?1 AND sequence<?2 ORDER BY sequence DESC LIMIT ?3", packageName, cursor, int(limit)+1)
	if err != nil {
		return protocol.DispatchPage{}, err
	}
	defer rows.Close()
	page := protocol.DispatchPage{Jobs: []protocol.DispatchJob{}}
	for rows.Next() {
		var sequence uint64
		var body []byte
		if err := rows.Scan(&sequence, &body); err != nil {
			return protocol.DispatchPage{}, err
		}
		if len(page.Jobs) == int(limit) {
			page.NextBefore = &cursor
			break
		}
		var job protocol.DispatchJob
		if err := json.Unmarshal(body, &job); err != nil {
			return protocol.DispatchPage{}, err
		}
		page.Jobs = append(page.Jobs, job)
		cursor = sequence
	}
	return page, rows.Err()
}

func putDispatchJob(ctx context.Context, db *sql.Conn, job protocol.DispatchJob) error {
	body, err := encode(job)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "UPDATE dispatch_jobs SET status=?2,body=?3 WHERE id=?1", job.ID.String(), string(job.Status), body)
	return err
}

func cancelDispatchJob(ctx context.Context, db *sql.Conn, packageName string, identity protocol.JobID, timestamp int64) (protocol.DispatchJob, error) {
	job, err := dispatchRequired[protocol.DispatchJob](ctx, db, "dispatch_jobs", packageName, "id", identity.String())
	if err != nil {
		return protocol.DispatchJob{}, err
	}
	if job.Status != protocol.DispatchStatusQueued {
		return protocol.DispatchJob{}, fault(protocol.ErrorCodeConflict, "only queued callbacks can be cancelled")
	}
	job.Status, job.FinishedMs = protocol.DispatchStatusCancelled, &timestamp
	if err := putDispatchJob(ctx, db, job); err != nil {
		return protocol.DispatchJob{}, err
	}
	return job, pruneDispatchJobs(ctx, db)
}

func cancelDispatchPending(ctx context.Context, db *sql.Conn, packageName, handler string, timestamp int64) error {
	jobs, err := readBodies[protocol.DispatchJob](ctx, db, "SELECT body FROM dispatch_jobs WHERE package=?1 AND status='queued' AND json_extract(body,'$.handler')=?2", packageName, handler)
	if err != nil {
		return err
	}
	for _, job := range jobs {
		job.Status, job.FinishedMs = protocol.DispatchStatusCancelled, &timestamp
		if err := putDispatchJob(ctx, db, job); err != nil {
			return err
		}
	}
	return pruneDispatchJobs(ctx, db)
}

func dispatchResult(ctx context.Context, db *sql.Conn, node protocol.NodeID, packageName string, identity protocol.JobID) (protocol.RequestOutcome, error) {
	job, err := dispatchRequired[protocol.DispatchJob](ctx, db, "dispatch_jobs", packageName, "id", identity.String())
	if err != nil {
		return protocol.RequestOutcome{}, err
	}
	var status string
	var result sql.NullString
	err = db.QueryRowContext(ctx, "SELECT status,result FROM requests WHERE id=?1 AND caller=?2", job.Request.String(), node[:]).Scan(&status, &result)
	if errors.Is(err, sql.ErrNoRows) {
		return protocol.RequestOutcome{Kind: "not_admitted"}, nil
	}
	if err != nil {
		return protocol.RequestOutcome{}, err
	}
	switch status {
	case "admitted", "unknown":
		return protocol.RequestOutcome{Kind: status}, nil
	case "completed":
		outcome, err := decodeDispatchReceipt(result)
		if err != nil {
			return protocol.RequestOutcome{}, err
		}
		return protocol.RequestOutcome{Kind: "completed", Data: outcome}, nil
	default:
		return protocol.RequestOutcome{}, fault(protocol.ErrorCodeInternal, "invalid stored request status")
	}
}

func ReadDispatchTurn(ctx context.Context, db *sql.Conn, node protocol.NodeID, packageName string, identity protocol.JobID) (protocol.TurnID, error) {
	outcome, err := dispatchResult(ctx, db, node, packageName, identity)
	if err != nil {
		return protocol.TurnID{}, err
	}
	if outcome.Kind == "completed" {
		result, ok := dispatchValue[protocol.Result[protocol.Output, protocol.Fault]](outcome.Data)
		if ok && result.Ok != nil && result.Ok.Kind == "queued_turn" {
			turn, ok := dispatchValue[protocol.QueuedTurn](result.Ok.Data)
			if ok {
				return turn.ID, nil
			}
		}
	}
	return protocol.TurnID{}, fault(protocol.ErrorCodeConflict, "callback has not admitted a turn")
}

func decodeDispatchReceipt(value sql.NullString) (protocol.Result[protocol.Output, protocol.Fault], error) {
	var result protocol.Result[protocol.Output, protocol.Fault]
	if !value.Valid || json.Unmarshal([]byte(value.String), &result) != nil {
		return result, fault(protocol.ErrorCodeInternal, "stored request outcome is invalid")
	}
	return result, nil
}

// claimDispatchJobs starts every queued job. A job whose package or callback
// is no longer authorized fails here; its notices are the returned events.
func claimDispatchJobs(ctx context.Context, db *sql.Conn, node protocol.NodeID, timestamp int64) ([]dispatcher.Work, []string, []protocol.Event, error) {
	type record struct{ owner, identity string }
	rows, err := db.QueryContext(ctx, "SELECT package,id FROM dispatch_jobs WHERE status='queued' ORDER BY sequence LIMIT 128")
	if err != nil {
		return nil, nil, nil, err
	}
	records := []record{}
	for rows.Next() {
		var item record
		if err = rows.Scan(&item.owner, &item.identity); err != nil {
			break
		}
		records = append(records, item)
	}
	readErr := rows.Err()
	closeErr := rows.Close()
	if err = errors.Join(err, readErr, closeErr); err != nil {
		return nil, nil, nil, err
	}
	work := []dispatcher.Work{}
	owners := []string{}
	notices := []protocol.Event{}
	for _, item := range records {
		job, callback, reference, err := readDispatchInvocation(ctx, db, item.identity)
		if err != nil {
			return nil, nil, nil, err
		}
		var authorityErr error
		if reference != nil {
			_, authorityErr = dispatchPackageVersion(ctx, db, *reference)
		} else {
			authorityErr = dispatchNativeCallback(item.owner, job, callback)
		}
		var request protocol.Request
		if authorityErr == nil {
			request, authorityErr = dispatchRequest(node, job, callback, reference)
		}
		owners = append(owners, item.owner)
		if authorityErr == nil {
			job.Status, job.StartedMs = protocol.DispatchStatusRunning, &timestamp
			work = append(work, dispatcher.Work{Job: job.ID, Request: request, Completion: callback.Completion})
		} else {
			job.Status, job.Error, job.FinishedMs = protocol.DispatchStatusFailed, dispatchFault(authorityErr), &timestamp
		}
		if err := putDispatchJob(ctx, db, job); err != nil {
			return nil, nil, nil, err
		}
		notice, err := dispatchNotice(ctx, db, job, reference, timestamp)
		if err != nil {
			return nil, nil, nil, err
		}
		notices = append(notices, notice...)
	}
	if len(owners) != 0 {
		if err := pruneDispatchJobs(ctx, db); err != nil {
			return nil, nil, nil, err
		}
	}
	return work, owners, notices, nil
}

func dispatchPackageVersion(ctx context.Context, db *sql.Conn, reference protocol.PluginReference) (protocol.PluginInfo, error) {
	if _, err := dispatchCurrent(ctx, db, reference.Name); err != nil {
		return protocol.PluginInfo{}, err
	}
	return ReadPluginVersion(ctx, db, reference)
}

func DispatchPackage(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, submitted protocol.Request) (*protocol.PluginInfo, error) {
	if caller != node {
		return nil, nil
	}
	var identity string
	err := db.QueryRowContext(ctx, "SELECT id FROM dispatch_jobs WHERE status='running' AND json_extract(body,'$.request')=?1", submitted.ID.String()).Scan(&identity)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	job, callback, reference, err := readDispatchInvocation(ctx, db, identity)
	if err != nil {
		return nil, err
	}
	expected, err := dispatchRequest(node, job, callback, reference)
	if err != nil {
		return nil, err
	}
	original, err := encode(expected)
	if err != nil {
		return nil, err
	}
	actual, err := encode(submitted)
	if err != nil {
		return nil, err
	}
	if !bytes.Equal(original, actual) {
		return nil, fault(protocol.ErrorCodePermissionDenied, "dispatch callback does not match its claimed job")
	}
	if reference != nil {
		info, err := dispatchPackageVersion(ctx, db, *reference)
		return &info, err
	}
	return nil, dispatchNativeCallback(dispatchNativeOwner, job, callback)
}

func readDispatchInvocation(ctx context.Context, db *sql.Conn, identity string) (protocol.DispatchJob, protocol.DispatchCallback, *protocol.PluginReference, error) {
	var body, encodedCallback, encodedReference []byte
	if err := db.QueryRowContext(ctx, "SELECT body,callback,reference FROM dispatch_jobs WHERE id=?1", identity).Scan(&body, &encodedCallback, &encodedReference); err != nil {
		return protocol.DispatchJob{}, protocol.DispatchCallback{}, nil, err
	}
	var job protocol.DispatchJob
	var callback protocol.DispatchCallback
	var reference *protocol.PluginReference
	if err := json.Unmarshal(body, &job); err != nil {
		return job, callback, reference, err
	}
	if err := json.Unmarshal(encodedCallback, &callback); err != nil {
		return job, callback, reference, err
	}
	err := json.Unmarshal(encodedReference, &reference)
	return job, callback, reference, err
}

func FinishDispatch(ctx context.Context, db *sql.Conn, identity protocol.JobID, result error, timestamp int64) ([]protocol.Event, error) {
	var owner string
	if err := db.QueryRowContext(ctx, "SELECT package FROM dispatch_jobs WHERE id=?1", identity.String()).Scan(&owner); err != nil {
		// An uncertain commit may already have ended this job and pruned it
		// under the history limit. There is no running owner left to settle.
		if errors.Is(err, sql.ErrNoRows) {
			return nil, nil
		}
		return nil, err
	}
	job, err := dispatchRequired[protocol.DispatchJob](ctx, db, "dispatch_jobs", owner, "id", identity.String())
	if err != nil {
		return nil, err
	}
	if job.Status != protocol.DispatchStatusRunning {
		switch job.Status {
		case protocol.DispatchStatusCompleted, protocol.DispatchStatusFailed, protocol.DispatchStatusCancelled, protocol.DispatchStatusUnknown:
			// Only the end write is retried. Preserve the first committed
			// outcome, timestamp and notice after an uncertain commit.
			return nil, nil
		}
		return nil, fault(protocol.ErrorCodeConflict, "callback is not running")
	}
	job.Error, job.FinishedMs = dispatchFault(result), &timestamp
	switch {
	case result == nil:
		job.Status = protocol.DispatchStatusCompleted
	case job.Error.Code == protocol.ErrorCodeOutcomeUnknown:
		job.Status = protocol.DispatchStatusUnknown
	case job.Error.Code == protocol.ErrorCodeCancelled:
		job.Status = protocol.DispatchStatusCancelled
	default:
		job.Status = protocol.DispatchStatusFailed
	}
	if err := putDispatchJob(ctx, db, job); err != nil {
		return nil, err
	}
	_, _, reference, err := readDispatchInvocation(ctx, db, identity.String())
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return nil, err
	}
	notices, err := dispatchNotice(ctx, db, job, reference, timestamp)
	if err != nil {
		return nil, err
	}
	if err := pruneDispatchJobs(ctx, db); err != nil {
		return nil, err
	}
	return append([]protocol.Event{dispatchChanged([]string{owner})}, notices...), nil
}

func pruneDispatchJobs(ctx context.Context, db *sql.Conn) error {
	_, err := db.ExecContext(ctx, "DELETE FROM dispatch_jobs WHERE status NOT IN ('queued','running') AND sequence NOT IN (SELECT sequence FROM dispatch_jobs ORDER BY sequence DESC LIMIT 2048)")
	return err
}

func recoverDispatchJobs(ctx context.Context, db *sql.Conn, node protocol.NodeID, timestamp int64) ([]string, []protocol.Event, error) {
	jobs, err := readBodies[protocol.DispatchJob](ctx, db, "SELECT body FROM dispatch_jobs WHERE status='running'")
	if err != nil {
		return nil, nil, err
	}
	owners := []string{}
	notices := []protocol.Event{}
	for _, job := range jobs {
		_, callback, _, err := readDispatchInvocation(ctx, db, job.ID.String())
		if err != nil {
			return nil, nil, err
		}
		var stored sql.NullString
		err = db.QueryRowContext(ctx, "SELECT result FROM requests WHERE id=?1 AND caller=?2", job.Request.String(), node[:]).Scan(&stored)
		if err != nil && !errors.Is(err, sql.ErrNoRows) {
			return nil, nil, err
		}
		var outcome error
		if err == nil && stored.Valid {
			result, err := decodeDispatchReceipt(stored)
			if err != nil {
				return nil, nil, err
			}
			if result.Err != nil {
				outcome = result.Err
			} else if callback.Completion == protocol.DispatchCompletionTurn {
				if result.Ok == nil || result.Ok.Kind != "queued_turn" {
					outcome = invalid("turn callback did not admit a turn")
				} else {
					turn, ok := dispatchValue[protocol.QueuedTurn](result.Ok.Data)
					if !ok {
						return nil, nil, fault(protocol.ErrorCodeInternal, "stored turn admission is invalid")
					}
					run, err := ReadAgentRun(ctx, db, turn.ID)
					if err != nil {
						outcome = err
					} else {
						finished, result := dispatcher.TurnResult(run)
						if !finished {
							continue
						}
						outcome = result
					}
				}
			}
		} else {
			outcome = fault(protocol.ErrorCodeOutcomeUnknown, "callback was interrupted; its side effects were not replayed")
		}
		events, err := FinishDispatch(ctx, db, job.ID, outcome, timestamp)
		if err != nil {
			return nil, nil, err
		}
		for _, event := range events {
			if changed, ok := event.Data.(protocol.EventDispatchChangedPayload); ok {
				owners = append(owners, changed.Packages...)
			} else {
				notices = append(notices, event)
			}
		}
	}
	return owners, notices, nil
}

func observeDispatchJobs(ctx context.Context, db *sql.Conn, node protocol.NodeID) ([]dispatcher.Work, error) {
	rows, err := db.QueryContext(ctx, "SELECT j.body,j.callback,r.body FROM dispatch_jobs j JOIN requests r ON r.id=json_extract(j.body,'$.request') AND r.caller=?1 WHERE j.status='running' AND r.status='completed'", node[:])
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	work := []dispatcher.Work{}
	for rows.Next() {
		var encodedJob, encodedCallback, encodedRequest []byte
		if err := rows.Scan(&encodedJob, &encodedCallback, &encodedRequest); err != nil {
			return nil, err
		}
		var job protocol.DispatchJob
		var callback protocol.DispatchCallback
		var request protocol.Request
		if err := json.Unmarshal(encodedJob, &job); err != nil {
			return nil, err
		}
		if err := json.Unmarshal(encodedCallback, &callback); err != nil {
			return nil, err
		}
		if err := json.Unmarshal(encodedRequest, &request); err != nil {
			return nil, err
		}
		work = append(work, dispatcher.Work{Job: job.ID, Request: request, Completion: callback.Completion})
	}
	return work, rows.Err()
}

func DispatchTick(ctx context.Context, db *sql.Conn, node protocol.NodeID, timestamp int64, recovering bool) (dispatcher.Batch, []protocol.Event, error) {
	batch := dispatcher.Batch{Work: []dispatcher.Work{}}
	owners := []string{}
	events := []protocol.Event{}
	if recovering {
		changed, notices, err := recoverDispatchJobs(ctx, db, node, timestamp)
		if err != nil {
			return batch, nil, err
		}
		owners, events = append(owners, changed...), append(events, notices...)
		observers, err := observeDispatchJobs(ctx, db, node)
		if err != nil {
			return batch, nil, err
		}
		batch.Work = append(batch.Work, observers...)
	}
	due, err := dueDispatchSchedules(ctx, db, timestamp)
	if err != nil {
		return batch, nil, err
	}
	owners = append(owners, due...)
	work, changed, notices, err := claimDispatchJobs(ctx, db, node, timestamp)
	if err != nil {
		return batch, nil, err
	}
	events = append(events, notices...)
	batch.Work = append(batch.Work, work...)
	owners = append(owners, changed...)
	batch.NextMs, err = nextDispatchSchedule(ctx, db, timestamp)
	if err != nil {
		return batch, nil, err
	}
	if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM dispatch_jobs WHERE status='queued')").Scan(&batch.Pending); err != nil {
		return batch, nil, err
	}
	if len(owners) != 0 {
		events = append(events, dispatchChanged(owners))
	}
	return batch, events, nil
}
