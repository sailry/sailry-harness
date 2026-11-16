package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func readHostRemoval(row identityScanner) (link.HostRemovalAttempt, error) {
	var value link.HostRemovalAttempt
	var body, failure []byte
	if err := row.Scan(&body, &value.State, &failure, &value.Active); err != nil {
		return value, err
	}
	if err := json.Unmarshal(body, &value.Request); err != nil {
		return value, err
	}
	if len(failure) != 0 {
		if err := json.Unmarshal(failure, &value.Fault); err != nil {
			return value, err
		}
	}
	return value, nil
}

func PrepareHostRemoval(ctx context.Context, db *sql.Conn, host protocol.NodeID, request protocol.Request) (link.HostRemovalAttempt, error) {
	value, err := readHostRemoval(db.QueryRowContext(ctx, "SELECT body,state,fault,active FROM identity_host_removals WHERE host=?1 AND active=1", host[:]))
	if err == nil {
		return value, nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return value, err
	}
	body, err := json.Marshal(request)
	if err != nil {
		return value, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO identity_host_removals(request,host,body,state,active) VALUES(?1,?2,?3,'pending',1)", request.ID.String(), host[:], body)
	return link.HostRemovalAttempt{Request: request, State: "pending", Active: true}, err
}

func RecordHostRemoval(ctx context.Context, db *sql.Conn, host protocol.NodeID, request protocol.RequestID, state string, failure *protocol.Fault) error {
	if !link.ValidHostRemovalState(state) {
		return invalid("invalid host removal state")
	}
	var body []byte
	var err error
	if failure != nil {
		body, err = json.Marshal(failure)
		if err != nil {
			return err
		}
	}
	_, err = db.ExecContext(ctx, "UPDATE identity_host_removals SET state=?3,fault=?4 WHERE host=?1 AND request=?2", host[:], request.String(), state, body)
	return err
}

func HostRemovals(ctx context.Context, db *sql.Conn) ([]link.HostRemovalAttempt, error) {
	rows, err := db.QueryContext(ctx, "SELECT body,state,fault,active FROM identity_host_removals ORDER BY rowid")
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	values := []link.HostRemovalAttempt{}
	for rows.Next() {
		value, err := readHostRemoval(rows)
		if err != nil {
			return nil, err
		}
		values = append(values, value)
	}
	return values, rows.Err()
}
