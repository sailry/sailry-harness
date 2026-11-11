package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"slices"
	"sort"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func publishDispatch(ctx context.Context, db *sql.Conn, event protocol.DispatchEvent, timestamp int64) ([]protocol.JobID, []string, error) {
	type record struct{ body, reference []byte }
	rows, err := db.QueryContext(ctx, "SELECT body,reference FROM dispatch_handlers WHERE source=?1 AND topic=?2 ORDER BY rowid", event.Source.Package, event.Source.Topic)
	if err != nil {
		return nil, nil, err
	}
	records := []record{}
	for rows.Next() {
		var item record
		if err = rows.Scan(&item.body, &item.reference); err != nil {
			break
		}
		records = append(records, item)
	}
	readErr := rows.Err()
	closeErr := rows.Close()
	if err = errors.Join(err, readErr, closeErr); err != nil {
		return nil, nil, err
	}
	jobs := []protocol.JobID{}
	owners := []string{event.Source.Package}
	for _, item := range records {
		var handler protocol.DispatchHandler
		var reference *protocol.PluginReference
		if err := json.Unmarshal(item.body, &handler); err != nil {
			return nil, nil, err
		}
		if err := json.Unmarshal(item.reference, &reference); err != nil {
			return nil, nil, err
		}
		if !handler.Enabled {
			continue
		}
		if reference != nil {
			info, err := ReadPlugin(ctx, db, reference.Name)
			if err != nil {
				return nil, nil, err
			}
			if info == nil || !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" {
				continue
			}
			current := dispatchReference(info.Summary)
			reference = &current
			owners = append(owners, current.Name)
		} else if err := dispatchNativeHandler(handler); err != nil {
			return nil, nil, err
		}
		job, err := enqueueDispatch(ctx, db, reference, handler, event, timestamp)
		if err != nil {
			return nil, nil, err
		}
		jobs = append(jobs, job.ID)
	}
	body, err := encode(event)
	if err != nil {
		return nil, nil, err
	}
	sequence, err := NextCounter(ctx, db, CounterDispatchEvents)
	if err != nil {
		return nil, nil, err
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO dispatch_events(id,body,sequence) VALUES(?1,?2,?3)", event.ID.String(), body, sequence); err != nil {
		return nil, nil, err
	}
	if _, err := db.ExecContext(ctx, "DELETE FROM dispatch_events WHERE sequence NOT IN (SELECT sequence FROM dispatch_events ORDER BY sequence DESC LIMIT 2048)"); err != nil {
		return nil, nil, err
	}
	sort.Strings(owners)
	return jobs, slices.Compact(owners), nil
}

func enqueueDispatch(ctx context.Context, db *sql.Conn, reference *protocol.PluginReference, handler protocol.DispatchHandler, event protocol.DispatchEvent, timestamp int64) (protocol.DispatchJob, error) {
	owner := dispatchNativeOwner
	if reference != nil {
		owner = reference.Name
	}
	job := protocol.DispatchJob{
		ID: protocol.NewJobID(), Event: event, Handler: handler.Name, Queue: handler.Queue,
		Request: protocol.NewRequestID(), Status: protocol.DispatchStatusQueued, CreatedMs: timestamp,
	}
	// A scheduled occurrence never overlaps its own handler's previous run: it
	// is recorded as skipped instead of started. An explicit enqueue or a
	// published event is not a schedule and always runs.
	if event.Schedule != nil {
		var busy bool
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM dispatch_jobs WHERE status IN ('queued','running') AND package=?1 AND json_extract(body,'$.handler')=?2)", owner, handler.Name).Scan(&busy); err != nil {
			return protocol.DispatchJob{}, err
		}
		if busy {
			job.Status, job.FinishedMs = protocol.DispatchStatusCancelled, &timestamp
			job.Error = &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "scheduled occurrence was skipped: the previous run has not finished"}
		}
	}
	if job.Status == protocol.DispatchStatusQueued {
		var pending int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM dispatch_jobs WHERE status='queued'").Scan(&pending); err != nil {
			return protocol.DispatchJob{}, err
		}
		if pending >= dispatchMaxPending {
			return protocol.DispatchJob{}, fault(protocol.ErrorCodeBusy, "pending dispatch capacity exhausted")
		}
	}
	body, err := encode(job)
	if err != nil {
		return protocol.DispatchJob{}, err
	}
	callback, err := encode(handler.Callback)
	if err != nil {
		return protocol.DispatchJob{}, err
	}
	capture, err := encode(reference)
	if err != nil {
		return protocol.DispatchJob{}, err
	}
	sequence, err := NextCounter(ctx, db, CounterDispatchJobs)
	if err != nil {
		return protocol.DispatchJob{}, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO dispatch_jobs(id,package,queue,status,body,callback,reference,sequence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
		job.ID.String(), owner, job.Queue, string(job.Status), body, callback, capture, sequence); err != nil {
		return protocol.DispatchJob{}, err
	}
	if job.Status != protocol.DispatchStatusQueued {
		err = pruneDispatchJobs(ctx, db)
	}
	return job, err
}
