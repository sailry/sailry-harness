package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func ReadComputerSettings(ctx context.Context, db *sql.Conn) (protocol.ComputerSettings, error) {
	value, err := readPersonalSetting[protocol.ComputerSettings](ctx, db, "computer")
	if value == nil {
		return protocol.DefaultComputerSettings(), err
	}
	return *value, err
}

func SaveComputerSettings(ctx context.Context, db *sql.Conn, input protocol.ComputerSettings) (protocol.ComputerSettings, []protocol.Event, error) {
	current, err := ReadComputerSettings(ctx, db)
	if err != nil {
		return protocol.ComputerSettings{}, nil, err
	}
	if current.Revision != input.Revision {
		return protocol.ComputerSettings{}, nil, fault(protocol.ErrorCodeRevisionConflict, "computer settings changed")
	}
	next, err := nextRevision(input.Revision)
	if err != nil {
		return protocol.ComputerSettings{}, nil, err
	}
	stored := protocol.ComputerSettings{
		Revision: next,
		Enabled:  input.Enabled,
	}
	body, err := encode(stored)
	if err != nil {
		return protocol.ComputerSettings{}, nil, err
	}
	if err = writePersonalSetting(ctx, db, "computer", body); err != nil {
		return protocol.ComputerSettings{}, nil, err
	}
	return stored, []protocol.Event{{Kind: "computer_settings_changed", Data: protocol.EventComputerSettingsChangedPayload{Revision: stored.Revision}}}, nil
}

func ReadExternalBrowserSettings(ctx context.Context, db *sql.Conn) (protocol.ExternalBrowserSettings, error) {
	value, err := readPersonalSetting[protocol.ExternalBrowserSettings](ctx, db, "external_browser")
	if value == nil {
		return protocol.DefaultExternalBrowserSettings(), err
	}
	return *value, err
}

func SaveExternalBrowserSettings(ctx context.Context, db *sql.Conn, input protocol.ExternalBrowserSettings) (protocol.ExternalBrowserSettings, []protocol.Event, error) {
	current, err := ReadExternalBrowserSettings(ctx, db)
	if err != nil {
		return protocol.ExternalBrowserSettings{}, nil, err
	}
	if current.Revision != input.Revision {
		return protocol.ExternalBrowserSettings{}, nil, fault(protocol.ErrorCodeRevisionConflict, "external browser settings changed")
	}
	next, err := nextRevision(input.Revision)
	if err != nil {
		return protocol.ExternalBrowserSettings{}, nil, err
	}
	stored := protocol.ExternalBrowserSettings{
		Revision: next,
		Enabled:  input.Enabled,
	}
	body, err := encode(stored)
	if err != nil {
		return protocol.ExternalBrowserSettings{}, nil, err
	}
	if err = writePersonalSetting(ctx, db, "external_browser", body); err != nil {
		return protocol.ExternalBrowserSettings{}, nil, err
	}
	return stored, []protocol.Event{{Kind: "external_browser_settings_changed", Data: protocol.EventExternalBrowserSettingsChangedPayload{Revision: stored.Revision}}}, nil
}
