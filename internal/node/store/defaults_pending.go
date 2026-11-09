package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"reflect"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func tableExists(ctx context.Context, connection *sql.Conn, name string) (bool, error) {
	var exists bool
	err := connection.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1)", name).Scan(&exists)
	return exists, err
}

func defaultsPending(ctx context.Context, connection *sql.Conn) (bool, error) {
	for _, table := range []string{"telemetry_settings", "search_settings", "roles", "teams"} {
		exists, err := tableExists(ctx, connection, table)
		if err != nil {
			return false, err
		}
		if !exists {
			continue
		}
		if table == "telemetry_settings" {
			var body []byte
			err := connection.QueryRowContext(ctx, "SELECT body FROM telemetry_settings WHERE singleton=1").Scan(&body)
			if errors.Is(err, sql.ErrNoRows) {
				return true, nil
			}
			if err != nil {
				return false, err
			}
			var settings protocol.TelemetrySettings
			if err := json.Unmarshal(body, &settings); err != nil {
				return false, err
			}
			if err := validateTelemetry(settings); err != nil {
				return false, err
			}
			var fields map[string]json.RawMessage
			if err := json.Unmarshal(body, &fields); err != nil {
				return false, err
			}
			if len(fields) != 4 {
				return true, nil
			}
			continue
		}
		rows, err := connection.QueryContext(ctx, "SELECT body FROM "+table)
		if err != nil {
			return false, err
		}
		pending := false
		for rows.Next() {
			var body []byte
			if err = rows.Scan(&body); err != nil {
				break
			}
			switch table {
			case "roles":
				var role protocol.RoleProfile
				if err = json.Unmarshal(body, &role); err != nil {
					break
				}
				previous := initialExplore(role.ID)
				previous.Appearance = nil
				previous.Instructions = previousExploreInstructions
				pending = role.Appearance == nil || reflect.DeepEqual(previous, role)
			case "teams":
				var team protocol.TeamProfile
				if err = json.Unmarshal(body, &team); err != nil {
					break
				}
				pending = team.Appearance == nil
			case "search_settings":
				var saved searchStored
				if err = json.Unmarshal(body, &saved); err != nil {
					break
				}
				var original struct {
					Settings map[string]json.RawMessage `json:"settings"`
				}
				if err = json.Unmarshal(body, &original); err != nil {
					break
				}
				_, exists := original.Settings["order"]
				pending = !exists
				for driver := range protocol.DefaultSearchSettings().Providers {
					if _, exists := saved.Settings.Providers[driver]; !exists {
						pending = true
					}
				}
			}
			if err != nil || pending {
				break
			}
		}
		err = errors.Join(err, rows.Err(), rows.Close())
		if err != nil || pending {
			return pending, err
		}
	}
	return false, nil
}
