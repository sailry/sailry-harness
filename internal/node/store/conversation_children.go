package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func conversationChildren(ctx context.Context, db *sql.Conn, entries []protocol.ConversationEntry) ([]protocol.ConversationChild, error) {
	calls := make([][2]string, 0, len(entries))
	for _, entry := range entries {
		calls = append(calls, [2]string{entry.Turn.String(), entry.ID})
	}
	body, err := encode(calls)
	if err != nil {
		return nil, err
	}
	rows, err := db.QueryContext(ctx, "SELECT a.body,d.body FROM agent_delegations d JOIN turns t ON t.id=d.turn JOIN agent_runs a ON a.turn=d.turn JOIN json_each(?1) e ON d.parent_turn=json_extract(e.value,'$[0]') AND d.entry=json_extract(e.value,'$[1]') ORDER BY t.rowid", string(body))
	if err != nil {
		return nil, err
	}
	selected := []protocol.ConversationChild{}
	for rows.Next() {
		var run, origin []byte
		if err = rows.Scan(&run, &origin); err != nil {
			rows.Close()
			return nil, err
		}
		value := protocol.DefaultConversationChild()
		if err = json.Unmarshal(run, &value.Run); err != nil {
			rows.Close()
			return nil, err
		}
		if err = json.Unmarshal(origin, &value.Origin); err != nil {
			rows.Close()
			return nil, err
		}
		selected = append(selected, value)
	}
	if err = errors.Join(rows.Err(), rows.Close()); err != nil {
		return nil, err
	}
	for index := range selected {
		role, err := ReadChildRole(ctx, db, selected[index].Run.Turn)
		if err != nil {
			return nil, err
		}
		if role != nil {
			selected[index].Name = &role.Name
		}
	}
	return selected, nil
}
