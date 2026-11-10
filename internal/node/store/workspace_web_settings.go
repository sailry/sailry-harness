package store

import (
	"context"
	"database/sql"
	"maps"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/web"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func readWebSearchStored(ctx context.Context, db *sql.Conn) (searchStored, error) {
	value, err := readBody[searchStored](ctx, db, "SELECT body FROM search_settings WHERE singleton=1")
	if err != nil {
		return searchStored{}, err
	}
	if value == nil {
		value = &searchStored{Settings: protocol.DefaultSearchSettings(), Credentials: map[protocol.SearchDriver]searchAuth{}}
	}
	describeSearch(&value.Settings)
	if err := validateSearch(*value); err != nil {
		return searchStored{}, invalid(err.Error())
	}
	return *value, nil
}

func ReadSearchSettings(ctx context.Context, db *sql.Conn) (protocol.SearchSettings, error) {
	saved, err := readWebSearchStored(ctx, db)
	return saved.Settings, err
}

// ReadWebSearchPlan copies private credentials into a Node-owned execution capture.
// The caller binds the frozen model callback before leaving its admission owner.
func ReadWebSearchPlan(ctx context.Context, db *sql.Conn) (web.SearchPlan, error) {
	saved, err := readWebSearchStored(ctx, db)
	if err != nil {
		return web.SearchPlan{}, err
	}
	plan := web.SearchPlan{Settings: saved.Settings, Credentials: make(map[protocol.SearchDriver]web.Auth, len(saved.Credentials))}
	for driver, auth := range saved.Credentials {
		plan.Credentials[driver] = web.Auth{Key: auth.Key, Basic: auth.Basic}
	}
	return plan, nil
}

func SearchSettingsController(request protocol.Request) bool {
	context := request.Plugin
	return context == nil || context.Surface.Kind == "settings" && context.Invocation == nil && context.Turn == nil && context.Session == nil && context.Worktree == nil
}

func SaveSearchSettings(ctx context.Context, db *sql.Conn, request protocol.Request, update protocol.SearchUpdate) (protocol.Output, []protocol.Event, error) {
	if !SearchSettingsController(request) {
		return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "search settings require a controller")
	}
	saved, err := readWebSearchStored(ctx, db)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if saved.Settings.Revision != update.Settings.Revision {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeRevisionConflict, "search settings changed")
	}
	saved.Settings = update.Settings
	saved.Settings.Providers = maps.Clone(update.Settings.Providers)
	saved.Settings.Order = slices.Clone(update.Settings.Order)
	if saved.Credentials == nil {
		saved.Credentials = map[protocol.SearchDriver]searchAuth{}
	}
	saved.Settings.Revision, err = nextRevision(update.Settings.Revision)
	if err != nil {
		return protocol.Output{}, nil, invalid("search revision is exhausted")
	}
	for driver, changes := range update.Credentials {
		auth := saved.Credentials[driver]
		auth.Key, err = replaceSearchSecret(auth.Key, changes.Key)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		auth.Basic, err = replaceSearchSecret(auth.Basic, changes.Basic)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		saved.Credentials[driver] = auth
	}
	for driver, config := range saved.Settings.Providers {
		auth := saved.Credentials[driver]
		config.KeyConfigured, config.BasicConfigured = auth.Key != nil, auth.Basic != nil
		saved.Settings.Providers[driver] = config
	}
	describeSearch(&saved.Settings)
	if err := validateSearch(saved); err != nil {
		return protocol.Output{}, nil, invalid(err.Error())
	}
	body, err := encode(saved)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO search_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body); err != nil {
		return protocol.Output{}, nil, err
	}
	return protocol.Output{Kind: "search_settings", Data: saved.Settings}, []protocol.Event{{Kind: "search_settings_changed", Data: protocol.EventSearchSettingsChangedPayload{Revision: saved.Settings.Revision}}}, nil
}

func replaceSearchSecret(current *protocol.Secret, update protocol.PluginSettingsSecretUpdate) (*protocol.Secret, error) {
	switch update.Kind {
	case "keep":
		return current, nil
	case "clear":
		return nil, nil
	case "replace":
		switch value := update.Data.(type) {
		case protocol.Secret:
			return &value, nil
		case *protocol.Secret:
			if value != nil {
				copy := *value
				return &copy, nil
			}
		}
	}
	return nil, invalid("invalid search credential update")
}
