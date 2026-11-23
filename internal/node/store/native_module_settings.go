package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// ReadComputerSettings returns the machine-wide Computer Use setting stored
// under the local Node owner row in user_personal_settings (avoiding a new
// singleton table while sharing one setting across all machine administrators
// and agent turns).
func ReadComputerSettings(ctx context.Context, db *sql.Conn) (protocol.ComputerSettings, error) {
	local, err := LocalUser(ctx, db)
	if err != nil {
		return protocol.DefaultComputerSettings(), err
	}
	value, err := readBody[protocol.ComputerSettings](ctx, db, "SELECT body FROM user_personal_settings WHERE user=?1 AND kind='computer'", local.ID.String())
	if value == nil {
		return protocol.DefaultComputerSettings(), err
	}
	return *value, err
}

func SaveComputerSettings(ctx context.Context, db *sql.Conn, input protocol.ComputerSettings) (protocol.ComputerSettings, []protocol.Event, error) {
	local, err := LocalUser(ctx, db)
	if err != nil {
		return protocol.ComputerSettings{}, nil, err
	}
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
	if _, err = db.ExecContext(ctx, "INSERT INTO user_personal_settings(user,kind,body) VALUES(?1,'computer',?2) ON CONFLICT(user,kind) DO UPDATE SET body=excluded.body", local.ID.String(), body); err != nil {
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
