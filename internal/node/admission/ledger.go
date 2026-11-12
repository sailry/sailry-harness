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
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

const credentialPrefix = "credential-v1:"

// Retention is how long a settled request answers a replay or an inspection.
// A retry later than this is a new request.
const Retention = 7 * 24 * time.Hour

// PruneInterval is how often a running Node removes expired requests.
const PruneInterval = 24 * time.Hour

// ErrNotAdmitted reports a result for a request that is not running.
var ErrNotAdmitted = errors.New("durable admission missing")

var settleRetry = time.Second

// A request is in one of three states. "admitted": it is running, or its
// result is being stored. "completed": it ended and its success or fault is
// stored. "unknown": the Node stopped while it ran, so nobody can tell what it
// did. The last two are final; nothing changes such a row except Prune, and
// Resolve for a request that another Node holds too.

// CredentialBody is the ledger identity of a request that carries a secret:
// its digest, so replay is exact and the secret is never retained.
func CredentialBody(body []byte) []byte {
	hash := blake3.Sum256(body)
	return []byte(credentialPrefix + hex.EncodeToString(hash[:]))
}

// DigestOnly reports whether the ledger keeps only the digest of a command's
// request. It names every durable command whose payload can hold a secret: a
// protocol.Secret, directly or inside a command carried by a schedule
// callback, and the server definitions of save_plugin_mcp, whose environment
// and headers are free-form.
func DigestOnly(kind string) bool {
	switch kind {
	case "put_credential", "save_provider", "complete_provider_login", "import_session",
		"save_ssh", "save_database", "save_search_settings",
		"save_plugin_settings", "save_plugin_mcp", "install_mcp",
		"start_connector_login", "submit_connector_input", "update_connection",
		"save_backup_settings", "restore_backup", "restore_backup_file",
		"dispatch", "plugin_transaction":
		return true
	default:
		return false
	}
}

// Body is what the ledger stores for a serialized request.
func Body(command protocol.Command, encoded []byte) []byte {
	if DigestOnly(command.Kind) {
		return CredentialBody(encoded)
	}
	return encoded
}

// Admit records the serialized request, or answers from its earlier admission.
func Admit(ctx context.Context, db *sql.Conn, caller protocol.NodeID, request protocol.Request, encoded []byte) (*link.Response, error) {
	previous, err := Existing(ctx, db, caller, request, encoded)
	if err != nil || previous != nil {
		return previous, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status,at) VALUES(?1,?2,?3,'admitted',?4)", caller[:], request.ID.String(), Body(request.Command, encoded), time.Now().Unix())
	return nil, err
}

// Existing answers a request that was admitted before: its stored result, or
// OutcomeUnknown while it runs and after a restart interrupted it. It never
// runs the request again; InspectRequest tells the two unknown cases apart.

func Existing(ctx context.Context, db *sql.Conn, caller protocol.NodeID, request protocol.Request, encoded []byte) (*link.Response, error) {
	body := Body(request.Command, encoded)
	var previous []byte
	var status string
	var result sql.NullString
	err := db.QueryRowContext(ctx, "SELECT body,status,result FROM requests WHERE caller=?1 AND id=?2", caller[:], request.ID.String()).Scan(&previous, &status, &result)
	if err == nil {
		if !bytes.Equal(previous, body) {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "request identifier was already used for different content"}
		}
		if status == "completed" {
			return decode(result)
		}
		if status == "unknown" {
			return &link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "request was interrupted by a Node restart; its outcome is unknown and will not change"}}, nil
		}
		return &link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "request is still running; inspect it using the same request identifier"}}, nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return nil, err
	}
	return nil, nil
}

// Complete stores the result of a running request inside the caller's
// transaction. A caller whose transaction then fails uses Settle.
func Complete(ctx context.Context, db *sql.Conn, caller protocol.NodeID, id protocol.RequestID, response link.Response) error {
	return record(ctx, db, caller, id, response, "status='admitted'")
}

// unresolved selects a request whose stored outcome is that it is not known.
const unresolved = "(status='unknown' OR status='completed' AND json_extract(result,'$.Err.code')='outcome_unknown')"

func record(ctx context.Context, db *sql.Conn, caller protocol.NodeID, id protocol.RequestID, response link.Response, state string) error {
	value := protocol.Result[protocol.Output, protocol.Fault]{Err: response.Fault}
	if response.Fault == nil {
		value.Ok = &response.Output
	}
	body, err := json.Marshal(value)
	if err != nil {
		return err
	}
	changed, err := db.ExecContext(ctx, "UPDATE requests SET status='completed',result=?3 WHERE caller=?1 AND id=?2 AND "+state, caller[:], id.String(), string(body))
	if err != nil {
		return err
	}
	count, err := changed.RowsAffected()
	if err == nil && count != 1 {
		return ErrNotAdmitted
	}
	return err
}

// Settle stores the result of a running request in a transaction of its own
// and repeats a failed write every second, so a request never stays "admitted"
// after it ended. write is the store's Write. Settle gives up when ctx ends
// (the Node is stopping, and Recover makes the request unknown at the next
// start), when the store can no longer confirm a commit, and when the request
// is not running.
func Settle(ctx context.Context, write func(context.Context, func(*sql.Conn) error) error, caller protocol.NodeID, id protocol.RequestID, response link.Response) error {
	return settle(ctx, write, caller, id, response, "status='admitted'")
}

// Resolve replaces an outcome that was stored as not known with the answer
// obtained later from the one party that knows it: the Node a request was
// forwarded to, which holds the request under the same identifier. It writes
// as Settle does. Nothing else changes a final row, and a row that holds a
// success or any other fault is left alone (ErrNotAdmitted).
func Resolve(ctx context.Context, write func(context.Context, func(*sql.Conn) error) error, caller protocol.NodeID, id protocol.RequestID, response link.Response) error {
	return settle(ctx, write, caller, id, response, unresolved)
}

func settle(ctx context.Context, write func(context.Context, func(*sql.Conn) error) error, caller protocol.NodeID, id protocol.RequestID, response link.Response, state string) error {
	recording := context.WithoutCancel(ctx)
	for {
		err := write(recording, func(db *sql.Conn) error { return record(recording, db, caller, id, response, state) })
		if err == nil || errors.Is(err, ErrNotAdmitted) {
			return err
		}
		var failed *protocol.Fault
		if errors.As(err, &failed) && failed.Code == protocol.ErrorCodeOutcomeUnknown {
			return err
		}
		select {
		case <-ctx.Done():
			return err
		case <-time.After(settleRetry):
		}
	}
}

// Recover runs once at startup, before any command. A request still admitted
// was interrupted by the previous stop: it becomes unknown, for good.
func Recover(ctx context.Context, db *sql.Conn) error {
	_, err := db.ExecContext(ctx, "UPDATE requests SET status='unknown' WHERE status='admitted'")
	return err
}

// Prune deletes completed and unknown requests older than Retention and
// reports how many. It keeps every request that a turn, a queued turn, an
// approval or a dispatch job refers to, because those read their input and
// result from this table; such a row goes once its owner is removed. A row
// written without a time is given the current one, so it lasts a full
// Retention from the first pass that sees it.
func Prune(ctx context.Context, db *sql.Conn, now time.Time) (int64, error) {
	if _, err := db.ExecContext(ctx, "UPDATE requests SET at=?1 WHERE at=0", now.Unix()); err != nil {
		return 0, err
	}
	removed, err := db.ExecContext(ctx, `DELETE FROM requests WHERE status IN ('completed','unknown') AND at<?1
		AND id NOT IN (SELECT request FROM turns)
		AND id NOT IN (SELECT request FROM agent_pending)
		AND id NOT IN (SELECT request FROM agent_approvals)
		AND id NOT IN (SELECT json_extract(body,'$.request') FROM dispatch_jobs WHERE json_extract(body,'$.request') IS NOT NULL)`, now.Add(-Retention).Unix())
	if err != nil {
		return 0, err
	}
	return removed.RowsAffected()
}

// Digest is the request digest InspectRequest expects: the lowercase BLAKE3
// of the serialized request. Any controller that kept a request can therefore
// ask its target what became of it.
func Digest(encoded []byte) string {
	hash := blake3.Sum256(encoded)
	return hex.EncodeToString(hash[:])
}

// Inspect reports what became of a request without running it: not_admitted
// (it never arrived), admitted (running), completed (with the stored success
// or fault) or unknown (final).

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
	stored, retained := bytes.CutPrefix(body, []byte(credentialPrefix))
	if !retained {
		stored = []byte(Digest(body))
	}
	if string(stored) != digest {
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
