package node

import (
	"context"
	"database/sql"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) renewCommandProvider(ctx context.Context, request protocol.Request) error {
	if request.Command.Kind == "plugin_transaction" {
		var providers []protocol.ProviderID
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			providers, err = store.PluginTransactionProviders(ctx, db, request)
			return err
		})
		if err != nil {
			return err
		}
		for _, provider := range providers {
			if err := node.renewProvider(ctx, provider); err != nil {
				return err
			}
		}
		return nil
	}
	switch request.Command.Kind {
	case "start_session", "submit_turn", "queue_turn", "submit_task", "continue_turn", "compact_context", "start_queued_turn", "send_queued_turn", "replace_turn", "validate_provider", "discover_models":
	default:
		return nil
	}
	var input struct {
		Session  *protocol.SessionID  `json:"session"`
		Turn     *protocol.TurnID     `json:"turn"`
		Provider *protocol.ProviderID `json:"provider"`
	}
	body, err := json.Marshal(request.Command.Data)
	if err != nil {
		return err
	}
	if request.Command.Kind == "discover_models" {
		source, ok := dispatchPayload[protocol.ConversationDiscoverySource](request.Command.Data)
		if !ok || source.Kind != "saved" {
			return nil
		}
		body, err = json.Marshal(source.Data)
		if err != nil {
			return err
		}
	}
	if json.Unmarshal(body, &input) != nil {
		return nil
	}
	var provider protocol.ProviderID
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		if request.Command.Kind == "start_session" {
			var draft protocol.ConversationStart
			if json.Unmarshal(body, &draft) != nil {
				return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "session start is invalid"}
			}
			var err error
			provider, err = store.StartSessionProvider(ctx, db, draft)
			return err
		}
		if input.Provider != nil {
			provider = *input.Provider
			return nil
		}
		if input.Session != nil {
			session, err := store.ReadSession(ctx, db, *input.Session)
			if err != nil {
				return err
			}
			provider = session.Config.Provider
			return nil
		}
		if input.Turn != nil {
			var config []byte
			err := db.QueryRowContext(ctx, "SELECT r.config FROM turns t JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision WHERE t.id=?1", input.Turn.String()).Scan(&config)
			if err != nil {
				return err
			}
			var captured protocol.SessionConfig
			if err := json.Unmarshal(config, &captured); err != nil {
				return err
			}
			provider = captured.Provider
		}
		return nil
	})
	if err != nil || provider == (protocol.ProviderID{}) {
		return err
	}
	return node.renewProvider(ctx, provider)
}
