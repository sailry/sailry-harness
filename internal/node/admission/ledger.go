// Package admission owns the caller-scoped durable request ledger.
package admission

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"strings"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func Admit(ctx context.Context, db *sql.Conn, caller protocol.NodeID, request protocol.Request, body []byte) (*link.Response, error) {
	previous, err := Existing(ctx, db, caller, request.ID, body)
	if err != nil || previous != nil {
		return previous, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", caller[:], request.ID.String(), body)
	return nil, err
}

func Existing(ctx context.Context, db *sql.Conn, caller protocol.NodeID, id protocol.RequestID, body []byte) (*link.Response, error) {
	var previous []byte
	var status string
	var result sql.NullString
	err := db.QueryRowContext(ctx, "SELECT body,status,result FROM requests WHERE caller=?1 AND id=?2", caller[:], id.String()).Scan(&previous, &status, &result)
	if err == nil {
		if !bytes.Equal(previous, body) {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "request identifier was already used for different content"}
		}
		if status == "completed" {
			return decode(result)
		}
		return &link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "request outcome requires reconciliation; automatic replay is disabled"}}, nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return nil, err
	}
	return nil, nil
}

func Complete(ctx context.Context, db *sql.Conn, caller protocol.NodeID, id protocol.RequestID, response link.Response) error {
	value := protocol.Result[protocol.Output, protocol.Fault]{Err: response.Fault}
	if response.Fault == nil {
		value.Ok = &response.Output
	}
	body, err := json.Marshal(value)
	if err != nil {
		return err
	}
	changed, err := db.ExecContext(ctx, "UPDATE requests SET status='completed',result=?3 WHERE caller=?1 AND id=?2 AND status='admitted'", caller[:], id.String(), string(body))
	if err != nil {
		return err
	}
	count, err := changed.RowsAffected()
	if err == nil && count != 1 {
		return errors.New("durable admission missing")
	}
	return err
}

func Recover(ctx context.Context, db *sql.Conn) error {
	_, err := db.ExecContext(ctx, "UPDATE requests SET status='unknown' WHERE status='admitted'")
	return err
}

func Inspect(ctx context.Context, db *sql.Conn, caller protocol.NodeID, id protocol.RequestID, digest string) (protocol.RequestOutcome, error) {
	if len(digest) != 64 || strings.Trim(digest, "0123456789abcdef") != "" {
		return protocol.RequestOutcome{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid request digest"}
	}
	var body []byte
	var status string
	var result sql.NullString
	err := db.QueryRowContext(ctx, "SELECT body,status,result FROM requests WHERE caller=?1 AND id=?2", caller[:], id.String()).Scan(&body, &status, &result)
	if errors.Is(err, sql.ErrNoRows) {
		return protocol.RequestOutcome{Kind: "not_admitted"}, nil
	}
	if err != nil {
		return protocol.RequestOutcome{}, err
	}
	hash := blake3.Sum256(body)
	if hex.EncodeToString(hash[:]) != digest {
		return protocol.RequestOutcome{}, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "request identifier was already used for different content"}
	}
	switch status {
	case "admitted", "unknown":
		return protocol.RequestOutcome{Kind: status}, nil
	case "completed":
		response, err := decode(result)
		if err != nil {
			return protocol.RequestOutcome{}, err
		}
		value := protocol.Result[protocol.Output, protocol.Fault]{Err: response.Fault}
		if value.Err == nil {
			value.Ok = &response.Output
		}
		return protocol.RequestOutcome{Kind: "completed", Data: value}, nil
	default:
		return protocol.RequestOutcome{}, errors.New("invalid admission status")
	}
}

func decode(body sql.NullString) (*link.Response, error) {
	if !body.Valid {
		return nil, errors.New("missing request result")
	}
	var value protocol.Result[protocol.Output, protocol.Fault]
	if err := json.Unmarshal([]byte(body.String), &value); err != nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "stored request outcome is invalid"}
	}
	response := &link.Response{Fault: value.Err}
	if value.Ok != nil {
		response.Output = *value.Ok
	}
	return response, nil
}
