package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"
	"net/url"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type searchAuth struct {
	Key   *protocol.Secret `json:"key"`
	Basic *protocol.Secret `json:"basic"`
}

func (value *searchAuth) UnmarshalJSON(data []byte) error {
	type plain searchAuth
	var decoded plain
	if err := protocol.DecodeObject(data, &decoded, nil, true); err != nil {
		return err
	}
	*value = searchAuth(decoded)
	return nil
}

type searchStored struct {
	Settings    protocol.SearchSettings              `json:"settings"`
	Credentials map[protocol.SearchDriver]searchAuth `json:"credentials"`
}

func (value *searchStored) UnmarshalJSON(data []byte) error {
	type plain searchStored
	var decoded plain
	if err := protocol.DecodeObject(data, &decoded, []string{"settings", "credentials"}, true); err != nil {
		return err
	}
	*value = searchStored(decoded)
	return nil
}

func (value searchStored) MarshalJSON() ([]byte, error) {
	type plain searchStored
	if value.Credentials == nil {
		value.Credentials = make(map[protocol.SearchDriver]searchAuth)
	}
	return json.Marshal(plain(value))
}

func describeSearch(settings *protocol.SearchSettings) {
	defaults := protocol.DefaultSearchSettings()
	settings.Capabilities = make(map[protocol.SearchDriver]protocol.SearchCapability, len(settings.Providers))
	for driver, config := range settings.Providers {
		capability := defaults.Capabilities[driver]
		if driver == protocol.SearchDriverExa && config.KeyConfigured {
			capability.Quota = protocol.SearchQuotaFreeQuota
		}
		settings.Capabilities[driver] = capability
	}
}

func applySearchDefaults(ctx context.Context, connection *sql.Conn) error {
	var body []byte
	if err := connection.QueryRowContext(ctx, "SELECT body FROM search_settings WHERE singleton=1").Scan(&body); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return nil
		}
		return err
	}
	var saved searchStored
	if err := json.Unmarshal(body, &saved); err != nil {
		return err
	}
	var original struct {
		Settings map[string]json.RawMessage `json:"settings"`
	}
	if err := json.Unmarshal(body, &original); err != nil {
		return err
	}
	_, hasOrder := original.Settings["order"]
	changed := !hasOrder
	for driver, config := range protocol.DefaultSearchSettings().Providers {
		if _, exists := saved.Settings.Providers[driver]; !exists {
			saved.Settings.Providers[driver] = config
			changed = true
		}
	}
	if !changed {
		return nil
	}
	describeSearch(&saved.Settings)
	if err := validateSearch(saved); err != nil {
		return err
	}
	body, err := json.Marshal(saved)
	if err != nil {
		return err
	}
	_, err = connection.ExecContext(ctx, "UPDATE search_settings SET body=?1 WHERE singleton=1", body)
	return err
}

func validateSearch(saved searchStored) error {
	settings := saved.Settings
	drivers := protocol.DefaultSearchSettings().Order
	if len(settings.Providers) != len(drivers) || settings.Revision > math.MaxInt64 {
		return errors.New("invalid search settings")
	}
	seen := make(map[protocol.SearchDriver]bool)
	for _, driver := range settings.Order {
		seen[driver] = true
	}
	if len(settings.Order) != len(drivers) || len(seen) != len(drivers) {
		return errors.New("search order must contain every driver once")
	}
	for _, driver := range drivers {
		config, exists := settings.Providers[driver]
		if !exists || !seen[driver] {
			return errors.New("search driver configuration is missing")
		}
		auth := saved.Credentials[driver]
		if driver != protocol.SearchDriverModel && config.Model != nil {
			return errors.New("model binding requires the model search driver")
		}
		if model := config.Model; model != nil && (strings.TrimSpace(model.Model) == "" || len(model.Model) > 256 || strings.ContainsFunc(model.Model, unicode.IsControl)) {
			return errors.New("invalid search model")
		}
		if driver == protocol.SearchDriverModel && auth.Key != nil {
			return errors.New("model search uses provider credentials")
		}
		if config.KeyConfigured != (auth.Key != nil) || config.BasicConfigured != (auth.Basic != nil) {
			return errors.New("search credentials do not match their configuration")
		}
		if driver != protocol.SearchDriverSearxng && (config.Endpoint != nil || auth.Basic != nil) {
			return errors.New("custom search endpoints and Basic authentication require SearXNG")
		}
		if endpoint := config.Endpoint; endpoint != nil {
			parsed, err := url.Parse(*endpoint)
			if err != nil || len(*endpoint) > 4096 || parsed.Hostname() == "" ||
				(parsed.Scheme != "http" && parsed.Scheme != "https") || parsed.RawQuery != "" || parsed.ForceQuery || strings.Contains(*endpoint, "#") {
				return errors.New("invalid search endpoint")
			}
			if parsed.User != nil {
				_, password := parsed.User.Password()
				if parsed.User.Username() != "" || password {
					return errors.New("invalid search endpoint")
				}
			}
		}
		for _, secret := range []*protocol.Secret{auth.Key, auth.Basic} {
			if secret == nil {
				continue
			}
			value := secret.Expose()
			if strings.TrimSpace(value) == "" || len(value) > 4096 || strings.ContainsFunc(value, unicode.IsControl) {
				return errors.New("invalid search credential")
			}
		}
		if auth.Basic != nil {
			user, _, found := strings.Cut(auth.Basic.Expose(), ":")
			if !found || user == "" {
				return errors.New("Basic authentication requires username:password")
			}
		}
	}
	return nil
}
