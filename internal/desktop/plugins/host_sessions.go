package plugins

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"sync"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type hostSDKAdd func(string, bool, HostOperation)

func (owner *Host) session(arguments []json.RawMessage, index int) (protocol.SessionID, error) {
	if index < len(arguments) && string(arguments[index]) != "null" {
		return hostValue[protocol.SessionID](arguments, index)
	}
	if session := owner.scope().Session; session != nil {
		return *session, nil
	}
	return protocol.SessionID{}, hostInvalid("plugin has no session scope")
}

func (owner *Host) addSessionSDK(add hostSDKAdd) {
	add("readProjectCatalog", true, owner.readOperation("read_project_catalog", nil))
	add("listTeams", true, owner.readOperation("list_teams", nil))
	add("listAssociatedSessions", true, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		association, err := hostValue[map[string]json.RawMessage](arguments, 0)
		if err != nil {
			return nil, err
		}
		if err := owner.association(association); err != nil {
			return nil, err
		}
		before, err := hostOptional[*protocol.SessionID](arguments, 1, nil)
		if err != nil {
			return nil, err
		}
		limit, err := hostOptional(arguments, 2, uint16(32))
		if err != nil {
			return nil, err
		}
		return owner.readData(ctx, "list_associated_sessions", map[string]any{"association": association, "before": before, "limit": limit})
	})
	add("startSession", false, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		draft, err := hostValue[map[string]json.RawMessage](arguments, 0)
		if err != nil {
			return nil, err
		}
		if body := optionalRaw(draft, "association"); string(body) != "null" {
			var association map[string]json.RawMessage
			if json.Unmarshal(body, &association) != nil {
				return nil, hostInvalid("invalid session association")
			}
			if err := owner.association(association); err != nil {
				return nil, err
			}
			draft["association"], _ = json.Marshal(association)
		}
		return owner.prepareData(ctx, "start_session", draft)
	})
	add("readSession", true, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		session, err := owner.session(arguments, 0)
		if err != nil {
			return nil, err
		}
		return owner.readData(ctx, "read_session", map[string]any{"session": session})
	})
	for _, definition := range []struct {
		name, command string
		limit         uint16
	}{
		{"readConversation", "read_conversation", 20},
		{"readConversationRuns", "read_conversation_runs", 100},
	} {
		add(definition.name, true, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			session, err := owner.session(arguments, 2)
			if err != nil {
				return nil, err
			}
			before, err := hostOptional[*protocol.TurnID](arguments, 0, nil)
			if err != nil {
				return nil, err
			}
			limit, err := hostOptional(arguments, 1, definition.limit)
			if err != nil {
				return nil, err
			}
			return owner.readData(ctx, definition.command, map[string]any{"session": session, "before": before, "limit": limit})
		})
	}
	add("readConversationRun", true, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		session, err := owner.session(arguments, 2)
		if err != nil {
			return nil, err
		}
		input, err := namedArgumentsWithDefaults(map[string]any{"expected_revision": nil}, "turn", "expected_revision")(arguments)
		if err != nil {
			return nil, err
		}
		input.(map[string]any)["session"] = session
		return owner.readData(ctx, "read_conversation_run", input)
	})
	for _, definition := range []struct{ name, command, field string }{
		{"sendMessage", "queue_turn", "message"},
		{"setSessionConfig", "set_session_config", "config"},
		{"setSessionExecution", "set_session_team", ""},
	} {
		add(definition.name, false, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			session, err := owner.session(arguments, 2)
			if err != nil {
				return nil, err
			}
			var draft map[string]any
			if definition.field == "" {
				value, err := firstArgument(arguments)
				if err != nil {
					return nil, err
				}
				var ok bool
				draft, ok = value.(map[string]any)
				if !ok {
					return nil, hostInvalid("invalid execution selection")
				}
				for field := range draft {
					if field != "execution" && field != "team" && field != "models" {
						return nil, hostInvalid("invalid execution selection")
					}
				}
				if _, exists := draft["team"]; !exists {
					draft["team"] = nil
				}
				if _, exists := draft["models"]; !exists {
					draft["models"] = map[string]any{}
				}
			} else {
				value, err := firstArgument(arguments)
				if err != nil {
					return nil, err
				}
				draft = map[string]any{definition.field: value}
			}
			revision, err := hostValue[string](arguments, 1)
			if err != nil {
				return nil, err
			}
			draft["session"], draft["expected_revision"] = session, revision
			return owner.prepareData(ctx, definition.command, draft)
		})
	}
	add("stopTurn", false, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		if _, err := owner.session(nil, 0); err != nil {
			return nil, err
		}
		return owner.prepareOperation("stop_turn", namedArguments("turn"))(ctx, arguments)
	})
	add("resolveSessionModel", true, owner.readOperation("resolve_plugin_model", namedArgumentsWithDefaults(
		map[string]any{"effort": nil, "config": nil}, "model", "effort", "config",
	)))
	add("nextConversation", true, owner.nextConversation)
}

func (owner *Host) association(value map[string]json.RawMessage) error {
	namespace := nativeProp(value, "namespace", "")
	if namespace != "" && namespace != owner.scope().Package.Name {
		return hostInvalid("session association namespace belongs to another package")
	}
	value["namespace"], _ = json.Marshal(owner.scope().Package.Name)
	return nil
}

type hostFeed struct {
	mu       sync.Mutex
	latest   json.RawMessage
	err      error
	changed  chan struct{}
	observer *client.Observer
	query    *protocol.UsageQuery
	all      bool
	revision uint64
}

func (owner *Host) feed(key, kind string, arguments json.RawMessage) (*hostFeed, error) {
	owner.mu.Lock()
	defer owner.mu.Unlock()
	if existing := owner.feeds[key]; existing != nil {
		return existing, nil
	}
	if owner.binding.Client == nil {
		return nil, hostUnavailable("plugin Client is unavailable")
	}
	observer, err := owner.binding.Client.Watch(owner.ctx, kind, arguments)
	if err != nil {
		return nil, err
	}
	feed := &hostFeed{changed: make(chan struct{}), observer: observer}
	owner.observers[key], owner.feeds[key] = observer, feed
	go func() {
		for {
			value, err := observer.Next(owner.ctx)
			feed.mu.Lock()
			feed.latest, feed.err = value, err
			close(feed.changed)
			feed.changed = make(chan struct{})
			feed.mu.Unlock()
			if err != nil {
				return
			}
		}
	}()
	return feed, nil
}

func (owner *Host) nextConversation(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	session, err := owner.session(nil, 0)
	if err != nil {
		return nil, err
	}
	seen, err := hostOptional(arguments, 0, "")
	if err != nil {
		return nil, err
	}
	runs := false
	if len(arguments) > 1 {
		projection, err := hostValue[string](arguments, 1)
		if err != nil || projection != "runs" {
			return nil, hostInvalid("invalid conversation projection")
		}
		runs = true
	}
	check := func() error {
		_, err := owner.readData(ctx, "read_session", map[string]any{"session": session})
		return err
	}
	if err := check(); err != nil {
		return nil, err
	}
	body, _ := hostJSON(session)
	feed, err := owner.feed("conversation", "watch_conversation", body)
	if err != nil {
		return nil, err
	}
	waited := false
	for {
		if err := owner.check(ctx); err != nil {
			return nil, err
		}
		feed.mu.Lock()
		body, failure, changed := append(json.RawMessage{}, feed.latest...), feed.err, feed.changed
		feed.mu.Unlock()
		if failure != nil {
			return nil, failure
		}
		if len(body) != 0 {
			value, err := decodeHostJSON(body)
			if err != nil {
				return nil, err
			}
			if runs {
				selected := map[string]any{"connected": hostAt(value, "connected"), "error": hostAt(value, "error"), "generation": hostAt(value, "generation"), "snapshot": nil}
				if snapshot := hostAt(value, "snapshot"); snapshot != nil {
					entries := []any{}
					hostRows(hostAt(snapshot, "page", "runs"), func(run any) {
						entries = append(entries, map[string]any{"turn": hostAt(run, "turn"), "status": hostAt(run, "status"), "error": hostAt(run, "error")})
					})
					selected["snapshot"] = map[string]any{"page": map[string]any{"session": hostAt(snapshot, "page", "session"), "runs": entries}}
				}
				value = selected
			}
			body, err = hostJSON(value)
			if err != nil {
				return nil, err
			}
			digest := blake3.Sum256(body)
			cursor := hex.EncodeToString(digest[:])
			if cursor != seen {
				if waited {
					if err := check(); err != nil {
						return nil, err
					}
				}
				return hostJSON(map[string]any{"cursor": cursor, "view": value})
			}
		}
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-owner.ctx.Done():
			return nil, owner.ctx.Err()
		case <-changed:
			waited = true
		}
	}
}
