package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"

	"github.com/sailry/sailry-harness/internal/node/hooks"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func ReadHooks(ctx context.Context, db *sql.Conn) (protocol.HooksSettings, error) {
	value, err := readBody[protocol.HooksSettings](ctx, db, "SELECT body FROM node_hook_settings WHERE singleton=1")
	if value == nil {
		return protocol.DefaultHooksSettings(), err
	}
	return *value, err
}

func captureHooks(ctx context.Context, db *sql.Conn) ([]byte, error) {
	value, err := ReadHooks(ctx, db)
	if err != nil {
		return nil, err
	}
	return encode(value)
}

func SaveHooks(ctx context.Context, db *sql.Conn, input protocol.HooksSettings, expected uint64) (protocol.HooksSettings, error) {
	current, err := ReadHooks(ctx, db)
	if err != nil {
		return protocol.HooksSettings{}, err
	}
	if current.Revision != expected || input.Revision != expected {
		return protocol.HooksSettings{}, fault(protocol.ErrorCodeRevisionConflict, "hook settings changed; reload before saving")
	}
	if err := hooks.Validate(input); err != nil {
		return protocol.HooksSettings{}, err
	}
	input.Revision, err = nextRevision(expected)
	if err != nil {
		return protocol.HooksSettings{}, err
	}
	body, err := encode(input)
	if err != nil {
		return protocol.HooksSettings{}, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO node_hook_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body)
	return input, err
}

func FrozenHooks(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (protocol.HooksSettings, error) {
	var body []byte
	if err := db.QueryRowContext(ctx, "SELECT hooks FROM agent_runs WHERE turn=?1", turn.String()).Scan(&body); err != nil {
		return protocol.HooksSettings{}, err
	}
	var settings protocol.HooksSettings
	err := json.Unmarshal(body, &settings)
	return settings, err
}

func ClaimHook(ctx context.Context, db *sql.Conn, turn protocol.TurnID, phase protocol.HooksPhase, ordinal uint64) error {
	if ordinal > math.MaxInt64 || phase == protocol.HooksPhaseTurnEnd && ordinal != 0 {
		return invalid("invalid hook receipt")
	}
	if _, err := phase.MarshalJSON(); err != nil {
		return invalid("invalid hook phase")
	}
	var body []byte
	var status, kind string
	err := db.QueryRowContext(ctx, "SELECT r.config,json_extract(a.body,'$.status'),t.kind FROM turns t JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision JOIN agent_runs a ON a.turn=t.id WHERE t.id=?1", turn.String()).Scan(&body, &status, &kind)
	if err != nil {
		return err
	}
	var config protocol.SessionConfig
	if err := json.Unmarshal(body, &config); err != nil {
		return err
	}
	if kind != "task" || config.Mode == protocol.WorkModePlan || status != "running" {
		return fault(protocol.ErrorCodePermissionDenied, "hooks cannot execute in this turn")
	}
	settings, err := FrozenHooks(ctx, db, turn)
	if err != nil {
		return err
	}
	if len(hooks.Scripts(settings, phase)) == 0 {
		return fault(protocol.ErrorCodePermissionDenied, "hook is not enabled in the admitted turn")
	}
	result, err := db.ExecContext(ctx, "INSERT OR IGNORE INTO hook_executions(turn,phase,ordinal) VALUES(?1,?2,?3)", turn.String(), string(phase), int64(ordinal))
	if err != nil {
		return err
	}
	count, err := result.RowsAffected()
	if err != nil {
		return err
	}
	if count != 1 {
		return fault(protocol.ErrorCodeOutcomeUnknown, "hook was already admitted; side effects were not replayed")
	}
	return nil
}

type hookResult struct {
	Outcome *protocol.ProcessOutcome `json:"Ok,omitempty"`
	Fault   *protocol.Fault          `json:"Err,omitempty"`
}

func FinishHook(ctx context.Context, db *sql.Conn, turn protocol.TurnID, phase protocol.HooksPhase, ordinal uint64, outcome protocol.ProcessOutcome, resultErr error) error {
	if ordinal > math.MaxInt64 {
		return invalid("hook sequence limit exceeded")
	}
	result := hookResult{}
	if resultErr != nil {
		result.Fault = &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: resultErr.Error()}
		var value *protocol.Fault
		if errors.As(resultErr, &value) {
			result.Fault = value
		}
	} else {
		result.Outcome = &outcome
	}
	body, err := encode(result)
	if err != nil {
		return err
	}
	updated, err := db.ExecContext(ctx, "UPDATE hook_executions SET result=?4 WHERE turn=?1 AND phase=?2 AND ordinal=?3 AND result IS NULL", turn.String(), string(phase), int64(ordinal), body)
	if err != nil {
		return err
	}
	count, err := updated.RowsAffected()
	if err != nil {
		return err
	}
	if count != 1 {
		return fault(protocol.ErrorCodeConflict, "hook receipt has already completed or does not exist")
	}
	return nil
}
