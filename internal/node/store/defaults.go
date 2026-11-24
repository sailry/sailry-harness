package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"slices"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	exploreDescription         = "Inspect the codebase and return concise findings with exact paths"
	previousExploreDescription = "Read existing code and report findings with exact paths"
	exploreInstructions        = "Inspect only the assigned codebase scope using read-only tools. Search and read in parallel, separate confirmed facts from inferences, and return concise findings with exact file:line pointers and open gaps."
	exploreInstructionsV1      = "Explore the assigned scope using read-only tools. Read existing code and relevant project instructions, distinguish confirmed facts from inferences, and return concise findings with exact paths and unresolved questions. Do not modify resources or request user input."
	exploreInstructionsV2      = "Explore the assigned scope using read-only tools. Read files the task names or needs, distinguish confirmed facts from inferences, and return concise findings with exact paths and unresolved questions. Do not modify resources or request user input."

	researchDescription         = "Investigate public sources and return cited conclusions"
	previousResearchDescription = "Investigate public sources and return supported conclusions"
	researchInstructions        = "Investigate the assigned question using the web-search:research skill and available read-only tools. Lead with evidence-backed conclusions, inline source links, dates where relevant, and conflicting or missing evidence."
	researchInstructionsV1      = "Research the assigned question using the selected web-search:research skill and available read-only tools. Return supported conclusions with source links, uncertainty and conflicting evidence to the parent. Keep the inherited permissions and the task's budget; do not modify resources or ask the user for input."

	exploreMaxTurns  = uint32(16)
	researchMaxTurns = uint32(8)
)

func initialExplore(id protocol.RoleID) protocol.RoleProfile {
	limit := exploreMaxTurns
	return protocol.RoleProfile{
		ID: id, Revision: 1, Key: "explore", Name: "Explore",
		Appearance:  &protocol.ProjectsAppearance{Icon: "code", Color: "teal"},
		Description: exploreDescription,
		ReadOnly:    true, MaxTurns: &limit, Skills: []string{}, Instructions: exploreInstructions,
	}
}

func initialResearch(id protocol.RoleID) protocol.RoleProfile {
	limit := researchMaxTurns
	return protocol.RoleProfile{
		ID: id, Revision: 1, Key: "research", Name: "Research",
		Appearance:  &protocol.ProjectsAppearance{Icon: "globe", Color: "violet"},
		Description: researchDescription,
		ReadOnly:    true, MaxTurns: &limit, Skills: []string{"web-search:research"}, Instructions: researchInstructions,
	}
}

func upgradeDefaultRole(role protocol.RoleProfile) (protocol.RoleProfile, bool) {
	switch role.Key {
	case "explore":
		if role.Name == "Explore" && role.ReadOnly && role.Tools == nil && len(role.Skills) == 0 &&
			role.MaxTurns == nil && role.Description == previousExploreDescription &&
			(role.Instructions == exploreInstructionsV1 || role.Instructions == exploreInstructionsV2) {
			limit := exploreMaxTurns
			role.Description = exploreDescription
			role.MaxTurns = &limit
			role.Instructions = exploreInstructions
			return role, true
		}
	case "research":
		if role.Name == "Research" && role.ReadOnly && role.Tools == nil &&
			slices.Equal(role.Skills, []string{"web-search:research"}) &&
			role.MaxTurns != nil && *role.MaxTurns == researchMaxTurns &&
			role.Description == previousResearchDescription && role.Instructions == researchInstructionsV1 {
			role.Description = researchDescription
			role.Instructions = researchInstructions
			return role, true
		}
	}
	return role, false
}

func initializeRoles(ctx context.Context, connection *sql.Conn, owner protocol.User) error {
	roles := []protocol.RoleProfile{
		initialExplore(protocol.NewRoleID()),
		initialResearch(protocol.NewRoleID()),
	}
	for _, role := range roles {
		body, err := json.Marshal(role)
		if err != nil {
			return err
		}
		if _, err := connection.ExecContext(ctx, "INSERT INTO roles(id,user,key,body) VALUES(?1,?2,?3,?4)", role.ID.String(), owner.ID.String(), configurationKey(owner, role.Key), body); err != nil {
			return err
		}
	}
	return nil
}

func initializeLocalModels(ctx context.Context, connection *sql.Conn) error {
	body, err := json.Marshal(protocol.DefaultModelsLocalSettings())
	if err != nil {
		return err
	}
	_, err = connection.ExecContext(ctx, "INSERT INTO local_model_settings(singleton,body) VALUES(1,?1)", body)
	return err
}

func initializeDefaults(ctx context.Context, connection *sql.Conn) error {
	var body []byte
	err := connection.QueryRowContext(ctx, "SELECT body FROM telemetry_settings WHERE singleton=1").Scan(&body)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return err
	}
	settings := protocol.DefaultTelemetrySettings()
	if err == nil {
		if err := json.Unmarshal(body, &settings); err != nil {
			return err
		}
		if err := validateTelemetry(settings); err != nil {
			return err
		}
	}
	var fields map[string]json.RawMessage
	if len(body) != 0 {
		if err := json.Unmarshal(body, &fields); err != nil {
			return err
		}
	}
	if len(fields) < 4 {
		body, err = json.Marshal(settings)
		if err != nil {
			return err
		}
		if _, err := connection.ExecContext(ctx, "INSERT INTO telemetry_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body); err != nil {
			return err
		}
	}
	if err := applySearchDefaults(ctx, connection); err != nil {
		return err
	}
	return refreshAppearances(ctx, connection)
}

func refreshAppearances(ctx context.Context, connection *sql.Conn) error {
	for _, table := range []string{"roles", "teams"} {
		rows, err := connection.QueryContext(ctx, "SELECT body FROM "+table+" ORDER BY key")
		if err != nil {
			return err
		}
		var bodies [][]byte
		for rows.Next() {
			var body []byte
			if err := rows.Scan(&body); err != nil {
				rows.Close()
				return err
			}
			bodies = append(bodies, body)
		}
		err = rows.Err()
		rows.Close()
		if err != nil {
			return err
		}
		for _, body := range bodies {
			var id string
			var changed bool
			var value any
			if table == "roles" {
				var role protocol.RoleProfile
				if err := json.Unmarshal(body, &role); err != nil {
					return err
				}
				role, changed = upgradeDefaultRole(role)
				if role.Appearance == nil {
					role.Appearance = appearanceFor(role.Key)
					changed = true
				}
				if changed {
					if role.Revision >= math.MaxInt64 {
						return errors.New("role revision exhausted")
					}
					role.Revision++
				}
				id, value = role.ID.String(), role
			} else {
				var team protocol.TeamProfile
				if err := json.Unmarshal(body, &team); err != nil {
					return err
				}
				if team.Appearance == nil {
					if team.Revision >= math.MaxInt64 {
						return errors.New("team revision exhausted")
					}
					team.Appearance = appearanceFor(team.Key)
					team.Revision++
					changed = true
				}
				id, value = team.ID.String(), team
			}
			if changed {
				body, err = json.Marshal(value)
				if err != nil {
					return err
				}
				if _, err := connection.ExecContext(ctx, "UPDATE "+table+" SET body=?1 WHERE id=?2", body, id); err != nil {
					return err
				}
			}
		}
	}
	return nil
}

func appearanceFor(key string) *protocol.ProjectsAppearance {
	switch key {
	case "explore":
		return &protocol.ProjectsAppearance{Icon: "code", Color: "teal"}
	case "research":
		return &protocol.ProjectsAppearance{Icon: "globe", Color: "violet"}
	}
	icons := []string{"folder", "code", "terminal", "globe", "server", "database", "package", "game", "book", "finance", "device", "education", "writing", "tags", "music", "media", "design", "health", "nature", "business", "analytics", "profile", "fitness", "law", "audio", "travel", "tools", "science", "ai", "favorite"}
	colors := []string{"blue", "indigo", "violet", "magenta", "red", "orange", "amber", "green", "teal", "cyan"}
	seed := uuid.New()
	return &protocol.ProjectsAppearance{Icon: icons[int(seed[0])%len(icons)], Color: colors[int(seed[1])%len(colors)]}
}

func validateTelemetry(settings protocol.TelemetrySettings) error {
	if settings.Revision > math.MaxInt64 || settings.InstallationID == uuid.Nil {
		return errors.New("telemetry settings are invalid")
	}
	return nil
}

func storedError(table string, err error) error {
	return fmt.Errorf("incompatible stored %s: %w", table, err)
}
