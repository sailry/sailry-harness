package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func ApplyConfigurationManifest(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, manifest protocol.ConfigurationManifest) ([]protocol.Event, error) {
	if err := validateConfigurationManifest(manifest); err != nil {
		return nil, err
	}
	user, err := configurationReceiver(ctx, db, node, caller, manifest.Source)
	if err != nil {
		return nil, err
	}
	if user.Controller != nil {
		return applyConfigurationCache(ctx, db, node, caller, manifest)
	}
	status, err := ReadConfigurationFollow(ctx, db)
	if err != nil {
		return nil, err
	}
	if status.Source == nil {
		if err := SetConfigurationSource(ctx, db, node, caller, &caller, status.Revision); err != nil {
			return nil, err
		}
	} else if *status.Source != caller {
		return nil, fault(protocol.ErrorCodePermissionDenied, "configuration belongs to a different source")
	}
	manifestDigest, err := configurationManifestDigest(manifest.Records)
	if err != nil {
		return nil, err
	}
	var revision uint64
	var previousDigest string
	err = db.QueryRowContext(ctx, "SELECT revision,digest FROM configuration_follow_versions WHERE user=?1 AND category=?2 AND source=?3", user.ID.String(), manifest.Category, caller[:]).Scan(&revision, &previousDigest)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return nil, err
	}
	if manifest.Revision < revision {
		return nil, fault(protocol.ErrorCodeRevisionConflict, "configuration manifest is older than the applied source revision")
	}
	if manifest.Revision == revision && manifestDigest != previousDigest {
		return nil, fault(protocol.ErrorCodeRevisionConflict, "source manifest reused a revision for different content")
	}
	ctx = configurationApplyContext(ctx)
	events := []protocol.Event{}
	seen := map[string]bool{}
	localIDs := map[string]string{}
	retained := map[string]bool{}
	claim := func(local, source string) error {
		if previous, exists := localIDs[local]; exists && previous != source {
			return invalid("configuration records share a target identity")
		}
		localIDs[local] = source
		return nil
	}
	for _, record := range manifest.Records {
		seen[record.ID] = true
		var digest, state, localID string
		var materialized bool
		err = db.QueryRowContext(ctx, "SELECT digest,state,materialized,CASE WHEN local_id='' THEN id ELSE local_id END FROM configuration_follow_records WHERE user=?1 AND category=?2 AND id=?3 AND source=?4", user.ID.String(), manifest.Category, record.ID, caller[:]).Scan(&digest, &state, &materialized, &localID)
		if err != nil && !errors.Is(err, sql.ErrNoRows) {
			return nil, err
		}
		if digest == record.Digest && (state == "applied" || state == "authorization") {
			if err := claim(localID, record.ID); err != nil {
				return nil, err
			}
			retained[localID] = materialized
			continue
		}
		if len(record.Body) == 0 {
			return nil, fault(protocol.ErrorCodeRevisionConflict, "configuration manifest needs missing record content")
		}
		local, err := localConfigurationRecord(ctx, db, manifest.Category, record)
		if err != nil {
			return nil, err
		}
		if err := claim(local.ID, record.ID); err != nil {
			return nil, err
		}
		// A replacement source identity may adopt the same role/team key. Keep
		// its last usable copy and lock even when the replacement cannot apply.
		if !materialized {
			origin, err := configurationOrigin(ctx, db, manifest.Category, local.ID)
			if err != nil {
				return nil, err
			}
			materialized = origin != nil && origin.Node == caller
		}
		var failure error
		if _, err := db.ExecContext(ctx, "SAVEPOINT follow_record"); err != nil {
			return nil, err
		}
		changed, missing, applyErr := applyConfigurationRecord(ctx, db, node, manifest.Category, local)
		if applyErr != nil {
			failure = applyErr
			_, err = db.ExecContext(ctx, "ROLLBACK TO follow_record")
			if err != nil {
				return nil, err
			}
		}
		if _, err := db.ExecContext(ctx, "RELEASE follow_record"); err != nil {
			return nil, err
		}
		state = "applied"
		message := ""
		if failure != nil {
			var reported *protocol.Fault
			if !errors.As(failure, &reported) {
				return nil, failure
			}
			state, message = "conflict", reported.Message
			if reported.Code == protocol.ErrorCodeNotFound || reported.Code == protocol.ErrorCodeNotConfigured || reported.Code == protocol.ErrorCodeUnavailable {
				state = "pending"
			}
		} else {
			materialized = true
			events = append(events, changed...)
			if missing {
				state, message = "authorization", "authorization is required on this host"
			}
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO configuration_follow_records(user,category,id,source,revision,digest,state,message,body,materialized,local_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) ON CONFLICT(user,category,id) DO UPDATE SET source=excluded.source,revision=excluded.revision,digest=excluded.digest,state=excluded.state,message=excluded.message,body=excluded.body,materialized=excluded.materialized,local_id=excluded.local_id", user.ID.String(), manifest.Category, record.ID, caller[:], record.Revision, record.Digest, state, message, record.Body, materialized, local.ID); err != nil {
			return nil, err
		}
		retained[local.ID] = materialized
	}
	rows, err := db.QueryContext(ctx, "SELECT id,CASE WHEN local_id='' THEN id ELSE local_id END,materialized FROM configuration_follow_records WHERE user=?1 AND category=?2 AND source=?3", user.ID.String(), manifest.Category, caller[:])
	if err != nil {
		return nil, err
	}
	type removed struct {
		id, local    string
		materialized bool
	}
	var stale []removed
	for rows.Next() {
		var item removed
		if err := rows.Scan(&item.id, &item.local, &item.materialized); err != nil {
			rows.Close()
			return nil, err
		}
		if !seen[item.id] {
			stale = append(stale, item)
		}
	}
	if err := errors.Join(rows.Err(), rows.Close()); err != nil {
		return nil, err
	}
	for _, item := range stale {
		if item.materialized && !retained[item.local] {
			changed, err := removeConfigurationCopy(ctx, db, node, manifest.Category, item.local)
			if err != nil {
				return nil, err
			}
			events = append(events, changed...)
		}
		if _, err := db.ExecContext(ctx, "DELETE FROM configuration_follow_records WHERE user=?1 AND category=?2 AND id=?3 AND source=?4", user.ID.String(), manifest.Category, item.id, caller[:]); err != nil {
			return nil, err
		}
	}
	digest := manifestDigest
	if _, err := db.ExecContext(ctx, "INSERT INTO configuration_follow_versions(user,category,source,revision,digest) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(user,category,source) DO UPDATE SET revision=excluded.revision,digest=excluded.digest", user.ID.String(), manifest.Category, caller[:], manifest.Revision, digest); err != nil {
		return nil, err
	}
	current, err := ReadConfigurationFollow(ctx, db)
	if err != nil {
		return nil, err
	}
	return append(events, protocol.Event{Kind: "configuration_follow_changed", Data: current}), nil
}

func configurationReceiver(ctx context.Context, db *sql.Conn, node, caller, source protocol.NodeID) (protocol.User, error) {
	if source != caller || caller == node {
		return protocol.User{}, fault(protocol.ErrorCodePermissionDenied, "configuration source does not match authenticated caller")
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return user, err
	}
	member, err := CallerUser(ctx, db, node, caller)
	if err != nil || member.ID != user.ID {
		return user, fault(protocol.ErrorCodePermissionDenied, "configuration source is not a member of this user")
	}
	if user.Local {
		return user, fault(protocol.ErrorCodePermissionDenied, "host-local configuration is not a controller replica")
	}
	if user.Controller != nil && *user.Controller != caller {
		return user, fault(protocol.ErrorCodePermissionDenied, "configuration belongs to a different controller")
	}
	return user, nil
}

func configurationCopyCollision(ctx context.Context, db *sql.Conn, category, id string) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	var exists bool
	switch category {
	case "providers", "roles", "teams", "ssh_profiles", "database_profiles":
		owner, present, err := configurationOwner(ctx, db, category, id)
		if err != nil {
			return err
		}
		if present && owner != user.ID {
			return fault(protocol.ErrorCodeConflict, "source identity belongs to another user")
		}
		exists = present
	}
	origin, err := configurationOrigin(ctx, db, category, id)
	if err != nil {
		return err
	}
	if origin != nil {
		return checkConfigurationCopySource(ctx, db, origin)
	}
	switch category {
	case "providers", "roles", "teams", "ssh_profiles", "database_profiles":
		if !user.Local {
			return nil
		}
	case "defaults":
		if !user.Local {
			return nil
		}
		value, err := ReadDefaults(ctx, db)
		if err != nil {
			return err
		}
		exists = value.Config != nil
	case "team_settings":
		if !user.Local {
			return nil
		}
		value, err := ReadTeamSettings(ctx, db)
		if err != nil {
			return err
		}
		exists = value.Revision != 0
	default:
		return nil
	}
	if exists {
		return fault(protocol.ErrorCodeConflict, "source record conflicts with local configuration")
	}
	return nil
}

func applyConfigurationRecord(ctx context.Context, db *sql.Conn, node protocol.NodeID, category string, record protocol.ConfigurationFollowRecord) ([]protocol.Event, bool, error) {
	if err := configurationCopyCollision(ctx, db, category, record.ID); err != nil {
		return nil, false, err
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return nil, false, err
	}
	localRevision, err := configurationCopyRevision(ctx, db, category, record.ID)
	if err != nil {
		return nil, false, err
	}
	origin := &protocol.ConfigurationOrigin{Node: protocol.NodeID{}, Revision: record.Revision}
	status, err := ReadConfigurationFollow(ctx, db)
	if err != nil {
		return nil, false, err
	}
	if status.Source != nil {
		origin.Node = *status.Source
	}
	switch category {
	case "providers":
		var provider protocol.ConversationProvider
		if err := decodeConfigurationRecord(record, &provider); err != nil {
			return nil, false, err
		}
		if provider.ID.String() != record.ID {
			return nil, false, invalid("provider identity does not match manifest")
		}
		previous, err := readProviderConfiguration(ctx, db, provider.ID)
		if err != nil {
			return nil, false, err
		}
		saved := providerConfiguration{ID: provider.ID, Listed: true, Authorizations: map[protocol.CredentialID]providerAuthorization{}}
		if previous != nil {
			saved = *previous
		}
		var retained *protocol.CredentialRef
		if previous != nil && previous.Provider != nil {
			retained = previous.Provider.Credential
		}
		provider.Revision = localRevision
		provider.Owner, provider.Shared, provider.Source = &user.ID, false, origin
		provider.Credential = nil
		missing := provider.Authentication != protocol.AuthenticationApiKey
		if record.Secret != nil {
			auth, err := importedAuthorization(provider.Authentication, *record.Secret, record.ExpiresAtMs)
			if err != nil {
				return nil, false, err
			}
			id := protocol.NewCredentialID()
			if saved.Authorizations == nil {
				saved.Authorizations = map[protocol.CredentialID]providerAuthorization{}
			}
			saved.Authorizations[id] = auth
			provider.Credential = &protocol.CredentialRef{Node: node, ID: id}
			missing = false
		} else if retained != nil && previous.Provider.Authentication == provider.Authentication && previous.Provider.Endpoint == provider.Endpoint {
			provider.Credential = retained
			missing = false
		}
		if provider.Credential == nil {
			missing = true
		}
		if provider.Preset != nil && provider.Preset.Source.Kind == "package" {
			provider.Binding = nil
		}
		if err := bindProviderPreset(ctx, db, &provider, nil, false); err != nil {
			return nil, false, err
		}
		if err := providers.ValidateConfiguration(provider); err != nil {
			return nil, false, err
		}
		saved.Provider = &provider
		saved.Listed = true
		if err := saveProviderConfiguration(ctx, db, saved); err != nil {
			return nil, false, err
		}
		return []protocol.Event{{Kind: "provider_changed", Data: provider}}, missing, nil
	case "roles":
		var role protocol.RoleProfile
		if err := decodeConfigurationRecord(record, &role); err != nil {
			return nil, false, err
		}
		if role.ID.String() != record.ID {
			return nil, false, invalid("role identity does not match manifest")
		}
		role.Source, role.Revision = origin, localRevision
		var conflict bool
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM roles WHERE key=?1 AND id<>?2)", configurationKey(user, role.Key), record.ID).Scan(&conflict); err != nil {
			return nil, false, err
		}
		if conflict {
			return nil, false, fault(protocol.ErrorCodeConflict, "role key conflicts with local configuration")
		}
		if err := ValidateRole(role); err != nil {
			return nil, false, err
		}
		if role.Model != nil {
			provider, err := listedRoleProvider(ctx, db, role.Model.Provider)
			if err != nil {
				return nil, false, err
			}
			if err := validateRoleModel(provider, *role.Model); err != nil {
				return nil, false, err
			}
		}
		body, err := encode(role)
		if err != nil {
			return nil, false, err
		}
		_, err = db.ExecContext(ctx, "INSERT INTO roles(id,user,key,body) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET key=excluded.key,body=excluded.body", record.ID, user.ID.String(), configurationKey(user, role.Key), body)
		return []protocol.Event{{Kind: "role_changed", Data: role}}, false, err
	case "teams":
		var team protocol.TeamProfile
		if err := decodeConfigurationRecord(record, &team); err != nil {
			return nil, false, err
		}
		if team.ID.String() != record.ID {
			return nil, false, invalid("team identity does not match manifest")
		}
		team.Source, team.Revision = origin, localRevision
		var conflict bool
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM teams WHERE key=?1 AND id<>?2)", configurationKey(user, team.Key), record.ID).Scan(&conflict); err != nil {
			return nil, false, err
		}
		if conflict {
			return nil, false, fault(protocol.ErrorCodeConflict, "team key conflicts with local configuration")
		}
		if err := ValidateTeam(team); err != nil {
			return nil, false, err
		}
		for _, id := range team.Members {
			role, err := readUserConfiguration[protocol.RoleProfile](ctx, db, "roles", id.String())
			if err != nil {
				return nil, false, err
			}
			if role == nil {
				return nil, false, fault(protocol.ErrorCodeNotConfigured, "source role is pending")
			}
		}
		if team.Model != nil {
			provider, err := listedRoleProvider(ctx, db, team.Model.Provider)
			if err != nil {
				return nil, false, err
			}
			if err := validateRoleModel(provider, *team.Model); err != nil {
				return nil, false, err
			}
		}
		err := writeTeam(ctx, db, team)
		return []protocol.Event{{Kind: "team_changed", Data: team}}, false, err
	case "ssh_profiles":
		var profile protocol.SshProfile
		if err := decodeConfigurationRecord(record, &profile); err != nil {
			return nil, false, err
		}
		if profile.ID.String() != record.ID || profile.Authentication == protocol.SshAuthenticationKeyPath {
			return nil, false, invalid("SSH source identity or authentication is not transferable")
		}
		previous, err := readUserConfiguration[protocol.SshProfile](ctx, db, category, record.ID)
		if err != nil {
			return nil, false, err
		}
		var trusted *protocol.SshHostKey
		if previous != nil && previous.Host == profile.Host && previous.Port == profile.Port {
			trusted = previous.HostKey
		}
		profile.HostKey = trusted
		profile.Source, profile.Revision = origin, localRevision
		var credential protocol.SshCredential
		if record.Secret != nil {
			if err := json.Unmarshal([]byte(record.Secret.Expose()), &credential); err != nil {
				return nil, false, err
			}
			if credential.Kind != string(profile.Authentication) || credential.Kind == "key_path" {
				return nil, false, invalid("SSH credential is not transferable")
			}
		} else if previous != nil && previous.Authentication == profile.Authentication && previous.Host == profile.Host && previous.Port == profile.Port && previous.Username == profile.Username {
			_, credential, err = ReadSsh(ctx, db, previous.ID, previous.Revision)
			if err != nil {
				return nil, false, err
			}
		} else {
			switch profile.Authentication {
			case protocol.SshAuthenticationPassword:
				credential = protocol.SshCredential{Kind: "password", Data: &protocol.SshCredentialPasswordPayload{Password: protocol.NewSecret("")}}
			case protocol.SshAuthenticationAgent:
				credential = protocol.SshCredential{Kind: "agent"}
			case protocol.SshAuthenticationPrivateKey:
				credential = protocol.SshCredential{Kind: "private_key", Data: &protocol.SshCredentialPrivateKeyPayload{Key: protocol.NewSecret("")}}
			default:
				return nil, false, invalid("SSH authentication is not transferable")
			}
		}
		body, err := encode(profile)
		if err != nil {
			return nil, false, err
		}
		secret, err := encode(credential)
		if err != nil {
			return nil, false, err
		}
		_, err = db.ExecContext(ctx, "INSERT INTO ssh_profiles(id,user,body,credential) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET body=excluded.body,credential=excluded.credential", record.ID, user.ID.String(), body, string(secret))
		return []protocol.Event{{Kind: "ssh_changed", Data: profile}}, record.Secret == nil && previous == nil, err
	case "database_profiles":
		var profile protocol.DatabaseProfile
		if err := decodeConfigurationRecord(record, &profile); err != nil {
			return nil, false, err
		}
		if profile.ID.String() != record.ID {
			return nil, false, invalid("database identity does not match manifest")
		}
		profile.Source, profile.Revision = origin, localRevision
		if err := validateDatabaseConnection(profile.Connection, nil); err != nil {
			return nil, false, err
		}
		if _, err := databaseTunnelProfile(ctx, db, profile.Connection); err != nil {
			return nil, false, err
		}
		previous, err := ReadDatabase(ctx, db, profile.ID)
		if err != nil {
			return nil, false, err
		}
		password := ""
		if record.Secret != nil {
			password = record.Secret.Expose()
		} else if previous != nil && checkDatabaseCredential(ctx, db, profile.ID, profile.Connection) == nil {
			if err := db.QueryRowContext(ctx, "SELECT password FROM database_profiles WHERE id=?1", record.ID).Scan(&password); err != nil {
				return nil, false, err
			}
		}
		target, err := captureDatabaseCredential(ctx, db, profile.Connection)
		if err != nil {
			return nil, false, err
		}
		body, err := encode(profile)
		if err != nil {
			return nil, false, err
		}
		_, err = db.ExecContext(ctx, "INSERT INTO database_profiles(id,user,body,password,password_target) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET body=excluded.body,password=excluded.password,password_target=excluded.password_target", record.ID, user.ID.String(), body, password, target)
		return []protocol.Event{{Kind: "database_changed", Data: profile}}, password == "" && profile.Connection.Kind != "sqlite", err
	case "defaults":
		var value protocol.Defaults
		if err := decodeConfigurationRecord(record, &value); err != nil {
			return nil, false, err
		}
		if record.ID != "default" || value.Config == nil {
			return nil, false, invalid("source defaults are invalid")
		}
		value.Source, value.Revision = origin, localRevision
		provider, err := readProviderConfiguration(ctx, db, value.Config.Provider)
		if err != nil {
			return nil, false, err
		}
		if provider == nil || provider.Provider == nil {
			return nil, false, fault(protocol.ErrorCodeNotConfigured, "source provider is pending")
		}
		value.Config.Credential = provider.Provider.Credential
		if err := validateConfig(ctx, db, node, *value.Config); err != nil {
			return nil, false, err
		}
		body, err := encode(value.Config)
		if err != nil {
			return nil, false, err
		}
		if user.Local {
			_, err = db.ExecContext(ctx, "INSERT INTO defaults(singleton,revision,config) VALUES(1,?1,?2) ON CONFLICT(singleton) DO UPDATE SET revision=excluded.revision,config=excluded.config", value.Revision, body)
		} else {
			_, err = db.ExecContext(ctx, "INSERT INTO user_defaults(user,revision,config) VALUES(?1,?2,?3) ON CONFLICT(user) DO UPDATE SET revision=excluded.revision,config=excluded.config", user.ID.String(), value.Revision, body)
		}
		return nil, false, err
	case "team_settings":
		var value protocol.TeamSettings
		if err := decodeConfigurationRecord(record, &value); err != nil {
			return nil, false, err
		}
		if record.ID != "default" || value.ChildConcurrency == 0 {
			return nil, false, invalid("source team settings are invalid")
		}
		value.Source, value.Revision = origin, localRevision
		body, err := encode(value)
		if err != nil {
			return nil, false, err
		}
		if user.Local {
			_, err = db.ExecContext(ctx, "INSERT INTO team_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body)
		} else {
			_, err = db.ExecContext(ctx, "INSERT INTO user_team_settings(user,body) VALUES(?1,?2) ON CONFLICT(user) DO UPDATE SET body=excluded.body", user.ID.String(), body)
		}
		return []protocol.Event{{Kind: "team_settings_changed", Data: protocol.EventTeamSettingsChangedPayload{Revision: value.Revision}}}, false, err
	case "plugins", "plugin_settings", "connections":
		return applyPluginFollowRecord(ctx, db, node, category, record)
	}
	return nil, false, invalid("unsupported configuration category")
}

func removeConfigurationCopy(ctx context.Context, db *sql.Conn, node protocol.NodeID, category, id string) ([]protocol.Event, error) {
	switch category {
	case "providers", "roles", "teams", "ssh_profiles", "database_profiles":
		user, err := CurrentUser(ctx, db)
		if err != nil {
			return nil, err
		}
		owner, present, err := configurationOwner(ctx, db, category, id)
		if err != nil || !present || owner != user.ID {
			return nil, err
		}
	}
	switch category {
	case "providers":
		provider, err := protocol.ParseProviderID(id)
		if err != nil {
			return nil, err
		}
		saved, err := readProviderConfiguration(ctx, db, provider)
		if err != nil || saved == nil {
			return nil, err
		}
		saved.Listed = false
		if err := saveProviderConfiguration(ctx, db, *saved); err != nil {
			return nil, err
		}
		return []protocol.Event{{Kind: "provider_removed", Data: protocol.EventProviderRemovedPayload{ID: provider}}}, nil
	case "roles":
		role, err := protocol.ParseRoleID(id)
		if err != nil {
			return nil, err
		}
		_, err = db.ExecContext(ctx, "DELETE FROM roles WHERE id=?1", id)
		return []protocol.Event{{Kind: "role_removed", Data: protocol.EventRoleRemovedPayload{ID: role, Teams: []protocol.TeamProfile{}}}}, err
	case "teams":
		team, err := protocol.ParseTeamID(id)
		if err != nil {
			return nil, err
		}
		_, err = db.ExecContext(ctx, "DELETE FROM teams WHERE id=?1", id)
		return []protocol.Event{{Kind: "team_removed", Data: protocol.EventTeamRemovedPayload{ID: team}}}, err
	case "ssh_profiles":
		ssh, err := protocol.ParseSshID(id)
		if err != nil {
			return nil, err
		}
		_, err = db.ExecContext(ctx, "DELETE FROM ssh_profiles WHERE id=?1", id)
		return []protocol.Event{{Kind: "ssh_removed", Data: protocol.EventSshRemovedPayload{ID: ssh}}}, err
	case "database_profiles":
		database, err := protocol.ParseDatabaseID(id)
		if err != nil {
			return nil, err
		}
		_, err = db.ExecContext(ctx, "DELETE FROM database_profiles WHERE id=?1", id)
		return []protocol.Event{{Kind: "database_removed", Data: protocol.EventDatabaseRemovedPayload{ID: database}}}, err
	case "defaults", "team_settings":
		user, err := CurrentUser(ctx, db)
		if err != nil {
			return nil, err
		}
		if user.Local {
			_, err = db.ExecContext(ctx, "DELETE FROM "+category+" WHERE singleton=1")
		} else {
			_, err = db.ExecContext(ctx, "DELETE FROM user_"+category+" WHERE user=?1", user.ID.String())
		}
		return nil, err
	case "plugins", "plugin_settings", "connections":
		return removePluginFollowCopy(ctx, db, node, category, id)
	}
	return nil, fmt.Errorf("unsupported configuration category %q", category)
}

func configurationCopyRevision(ctx context.Context, db *sql.Conn, category, id string) (uint64, error) {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return 0, err
	}
	var revision uint64
	switch category {
	case "providers":
		err = db.QueryRowContext(ctx, "SELECT COALESCE(json_extract(body,'$.provider.revision'),0) FROM providers WHERE id=?1 AND user=?2", id, user.ID.String()).Scan(&revision)
	case "roles", "teams", "ssh_profiles", "database_profiles":
		err = db.QueryRowContext(ctx, "SELECT COALESCE(json_extract(body,'$.revision'),0) FROM "+category+" WHERE id=?1 AND user=?2", id, user.ID.String()).Scan(&revision)
	case "defaults":
		value, readErr := ReadDefaults(ctx, db)
		revision, err = value.Revision, readErr
	case "team_settings":
		value, readErr := ReadTeamSettings(ctx, db)
		revision, err = value.Revision, readErr
	default:
		return 0, nil
	}
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return 0, err
	}
	return nextRevision(revision)
}
