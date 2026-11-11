package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"reflect"
	"strings"
	"unicode"

	"github.com/MoeForge/moego/base64"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func ListSsh(ctx context.Context, db *sql.Conn) ([]protocol.SshProfile, error) {
	return readBodies[protocol.SshProfile](ctx, db, "SELECT body FROM ssh_profiles ORDER BY id")
}

func ReadSsh(ctx context.Context, db *sql.Conn, id protocol.SshID, revision uint64) (protocol.SshProfile, protocol.SshCredential, error) {
	var body []byte
	var credential string
	if err := db.QueryRowContext(ctx, "SELECT body,credential FROM ssh_profiles WHERE id=?1", id.String()).Scan(&body, &credential); err != nil {
		if err == sql.ErrNoRows {
			err = fault(protocol.ErrorCodeNotFound, "SSH profile does not exist on this Node")
		}
		return protocol.SshProfile{}, protocol.SshCredential{}, err
	}
	var profile protocol.SshProfile
	var secret protocol.SshCredential
	if err := json.Unmarshal(body, &profile); err != nil {
		return profile, secret, err
	}
	if err := checkRevision(profile.Revision, revision); err != nil {
		return profile, secret, err
	}
	err := json.Unmarshal([]byte(credential), &secret)
	return profile, secret, err
}

func ExecuteSshCommand(ctx context.Context, db *sql.Conn, command protocol.Command) (protocol.Output, []protocol.Event, error) {
	switch input := command.Data.(type) {
	case *protocol.CommandSaveSshPayload:
		previous, err := readBody[protocol.SshProfile](ctx, db, "SELECT body FROM ssh_profiles WHERE id=?1", input.Profile.ID.String())
		if err != nil {
			return protocol.Output{}, nil, err
		}
		current := uint64(0)
		if previous != nil {
			current = previous.Revision
		}
		if err := checkRevision(current, input.ExpectedRevision); err != nil {
			return protocol.Output{}, nil, err
		}
		if err := validateSshSharing(ctx, db, input.Profile.Sharing); err != nil {
			return protocol.Output{}, nil, err
		}
		profile := input.Profile
		profile.Name = strings.TrimSpace(profile.Name)
		profile.Host = strings.TrimSpace(profile.Host)
		profile.Username = strings.TrimSpace(profile.Username)
		if profile.Name == "" || len(profile.Name) > 256 || profile.Host == "" || len(profile.Host) > 253 ||
			strings.ContainsFunc(profile.Host, func(char rune) bool { return unicode.IsSpace(char) || unicode.IsControl(char) }) ||
			profile.Port == 0 || profile.Username == "" || len(profile.Username) > 256 || strings.ContainsFunc(profile.Username, unicode.IsControl) {
			return protocol.Output{}, nil, invalid("invalid SSH profile")
		}
		var trusted *protocol.SshHostKey
		if previous != nil {
			trusted = previous.HostKey
		}
		if !reflect.DeepEqual(profile.HostKey, trusted) {
			return protocol.Output{}, nil, invalid("host keys require explicit confirmation")
		}
		if previous != nil && (previous.Host != profile.Host || previous.Port != profile.Port) {
			profile.HostKey = nil
		}
		var encodedCredential any
		if input.Credential != nil {
			if input.Credential.Kind != string(profile.Authentication) {
				return protocol.Output{}, nil, invalid("SSH credential authentication does not match")
			}
			body, err := json.Marshal(input.Credential)
			if err != nil {
				return protocol.Output{}, nil, err
			}
			if len(body) > 65536 {
				return protocol.Output{}, nil, invalid("SSH credential exceeds the size limit")
			}
			encodedCredential = string(body)
		} else if previous == nil || previous.Authentication != profile.Authentication {
			return protocol.Output{}, nil, invalid("SSH credential is required")
		}
		profile.Revision, err = nextRevision(input.ExpectedRevision)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		body, err := encode(profile)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if previous == nil {
			_, err = db.ExecContext(ctx, "INSERT INTO ssh_profiles(id,body,credential) VALUES(?1,?2,?3)", profile.ID.String(), body, encodedCredential)
		} else {
			_, err = db.ExecContext(ctx, "UPDATE ssh_profiles SET body=?2,credential=COALESCE(?3,credential) WHERE id=?1", profile.ID.String(), body, encodedCredential)
		}
		return protocol.Output{Kind: "ssh_profile", Data: profile}, []protocol.Event{{Kind: "ssh_changed", Data: profile}}, err
	case *protocol.CommandTrustSshPayload:
		profile, _, err := ReadSsh(ctx, db, input.Profile, input.ExpectedRevision)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		digest, err := base64.RawStdEncoding.DecodeString(strings.TrimPrefix(input.Key.Fingerprint, "SHA256:"))
		if input.Key.Algorithm == "" || len(input.Key.Algorithm) > 128 || !strings.HasPrefix(input.Key.Fingerprint, "SHA256:") || err != nil || len(digest) != 32 {
			return protocol.Output{}, nil, invalid("invalid SSH host key")
		}
		profile.Revision, err = nextRevision(input.ExpectedRevision)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		key := input.Key
		profile.HostKey = &key
		body, err := encode(profile)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		_, err = db.ExecContext(ctx, "UPDATE ssh_profiles SET body=?2 WHERE id=?1", profile.ID.String(), body)
		return protocol.Output{Kind: "ssh_profile", Data: profile}, []protocol.Event{{Kind: "ssh_changed", Data: profile}}, err
	case *protocol.CommandRemoveSshPayload:
		if _, _, err := ReadSsh(ctx, db, input.Profile, input.ExpectedRevision); err != nil {
			return protocol.Output{}, nil, err
		}
		if _, err := db.ExecContext(ctx, "DELETE FROM ssh_profiles WHERE id=?1", input.Profile.String()); err != nil {
			return protocol.Output{}, nil, err
		}
		profiles, err := ListSsh(ctx, db)
		return protocol.Output{Kind: "ssh_profiles", Data: profiles}, []protocol.Event{{Kind: "ssh_removed", Data: protocol.EventSshRemovedPayload{ID: input.Profile}}}, err
	}
	return protocol.Output{}, nil, invalid("SSH configuration command expected")
}

func validateSshSharing(ctx context.Context, db *sql.Conn, sharing *protocol.ConnectionSharing) error {
	if sharing == nil || sharing.Kind != "projects" {
		return nil
	}
	var projects []protocol.ProjectID
	body, err := json.Marshal(sharing.Data)
	if err != nil || json.Unmarshal(body, &projects) != nil || len(projects) == 0 {
		return invalid("select distinct projects for sharing")
	}
	seen := map[protocol.ProjectID]bool{}
	for _, project := range projects {
		if seen[project] {
			return invalid("select distinct projects for sharing")
		}
		seen[project] = true
		var exists bool
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1 AND id NOT IN (SELECT project FROM removed_projects))", project.String()).Scan(&exists); err != nil {
			return err
		}
		if !exists {
			return fault(protocol.ErrorCodeNotFound, "shared project is unavailable")
		}
	}
	return nil
}
