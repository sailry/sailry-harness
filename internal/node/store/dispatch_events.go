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
			info, err := readBody[protocol.PluginInfo](ctx, db, "SELECT body FROM plugins WHERE name=?1 AND installed=1", reference.Name)
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
	var pending int
	if err := db.QueryRowContext(ctx, "SELECT count(*) FROM dispatch_jobs WHERE status='queued'").Scan(&pending); err != nil {
		return protocol.DispatchJob{}, err
	}
	if pending >= dispatchMaxPending {
		return protocol.DispatchJob{}, fault(protocol.ErrorCodeBusy, "pending dispatch capacity exhausted")
	}
	job := protocol.DispatchJob{
		ID: protocol.NewJobID(), Event: event, Handler: handler.Name, Queue: handler.Queue,
		Request: protocol.NewRequestID(), Status: protocol.DispatchStatusQueued, CreatedMs: timestamp,
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
	owner := dispatchNativeOwner
	if reference != nil {
		owner = reference.Name
	}
	_, err = db.ExecContext(ctx, "INSERT INTO dispatch_jobs(id,package,queue,status,body,callback,reference,sequence) VALUES(?1,?2,?3,'queued',?4,?5,?6,?7)",
		job.ID.String(), owner, job.Queue, body, callback, capture, sequence)
	return job, err
}
