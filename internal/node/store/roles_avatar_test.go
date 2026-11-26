package store

import (
	"database/sql"
	"encoding/json"
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// A role's and a team's icon may name a generated avatar by its seed; a
// project's may not, and no icon may name one by anything else.
func TestRoleAvatarAppearance(t *testing.T) {
	t.Parallel()
	role := protocol.DefaultRoleProfile()
	role.ID, role.Key, role.Name = protocol.NewRoleID(), "review", "Review"
	for icon, valid := range map[string]bool{"avatar-fox": true, "avatar-k3f9x2a1": true, "avatar-" + strings.Repeat("a", 32): true, "code": true, "avatar-" + strings.Repeat("a", 33): false, "avatar-Fox": false, "avatar-fox-1": false, "avatar-": false, "fox": false} {
		role.Appearance = &protocol.ProjectsAppearance{Icon: icon, Color: "teal"}
		if err := ValidateRole(role); (err == nil) != valid {
			t.Errorf("role icon %q: %v", icon, err)
		}
	}
	role.Appearance = &protocol.ProjectsAppearance{Icon: "avatar-fox", Color: "pink"}
	if ValidateRole(role) == nil {
		t.Error("an avatar passes with an unknown color")
	}
	if validateAppearance(protocol.ProjectsAppearance{Icon: "avatar-fox", Color: "teal"}) == nil {
		t.Error("a project takes a generated avatar")
	}
	// A team takes the same avatars.
	team := protocol.DefaultTeamProfile()
	team.ID, team.Key, team.Name = protocol.NewTeamID(), "ship", "Ship"
	for icon, valid := range map[string]bool{"avatar-k3f9x2a1": true, "code": true, "avatar-Fox": false} {
		team.Appearance = &protocol.ProjectsAppearance{Icon: icon, Color: "none"}
		if err := ValidateTeam(team); (err == nil) != valid {
			t.Errorf("team icon %q: %v", icon, err)
		}
	}
}

func TestDefaultRolesAndUpgrade(t *testing.T) {
	t.Parallel()
	explore := initialExplore(protocol.NewRoleID())
	research := initialResearch(protocol.NewRoleID())
	if !explore.ReadOnly || explore.MaxTurns == nil || *explore.MaxTurns != exploreMaxTurns || len(explore.Skills) != 0 {
		t.Fatalf("unexpected initial explore role: %+v", explore)
	}
	if !research.ReadOnly || research.MaxTurns == nil || *research.MaxTurns != researchMaxTurns || !reflect.DeepEqual(research.Skills, []string{"web-search:research"}) {
		t.Fatalf("unexpected initial research role: %+v", research)
	}
	if _, changed := upgradeDefaultRole(explore); changed {
		t.Fatal("fresh explore role reported an upgrade")
	}
	if _, changed := upgradeDefaultRole(research); changed {
		t.Fatal("fresh research role reported an upgrade")
	}

	chosenModel := &protocol.RoleModel{Provider: protocol.NewProviderID(), Model: "fast-model"}
	legacyExplore := protocol.RoleProfile{
		ID: protocol.NewRoleID(), Revision: 2, Key: "explore", Name: "Explore",
		Appearance:  &protocol.ProjectsAppearance{Icon: "code", Color: "teal"},
		Description: previousExploreDescription, ReadOnly: true, Model: chosenModel,
		Skills: []string{}, Instructions: exploreInstructionsV2,
	}
	upgradedExplore, changed := upgradeDefaultRole(legacyExplore)
	if !changed || upgradedExplore.Instructions != exploreInstructions || upgradedExplore.Description != exploreDescription ||
		upgradedExplore.MaxTurns == nil || *upgradedExplore.MaxTurns != exploreMaxTurns || !reflect.DeepEqual(upgradedExplore.Model, chosenModel) {
		t.Fatalf("legacy explore upgrade = %+v (changed=%v)", upgradedExplore, changed)
	}
	if _, again := upgradeDefaultRole(upgradedExplore); again {
		t.Fatal("upgraded explore role was not idempotent")
	}

	previousLimit := previousExploreMaxTurns
	legacyExploreV3 := protocol.RoleProfile{
		ID: protocol.NewRoleID(), Revision: 3, Key: "explore", Name: "Explore",
		Appearance:  &protocol.ProjectsAppearance{Icon: "code", Color: "teal"},
		Description: previousExploreDescriptionV2, ReadOnly: true, MaxTurns: &previousLimit, Model: chosenModel,
		Skills: []string{}, Instructions: exploreInstructionsV3,
	}
	upgradedExploreV3, changedV3 := upgradeDefaultRole(legacyExploreV3)
	if !changedV3 || upgradedExploreV3.Instructions != exploreInstructions || upgradedExploreV3.Description != exploreDescription ||
		upgradedExploreV3.MaxTurns == nil || *upgradedExploreV3.MaxTurns != exploreMaxTurns || !reflect.DeepEqual(upgradedExploreV3.Model, chosenModel) {
		t.Fatalf("legacy explore V3 upgrade = %+v (changed=%v)", upgradedExploreV3, changedV3)
	}
	if _, again := upgradeDefaultRole(upgradedExploreV3); again {
		t.Fatal("upgraded explore V3 role was not idempotent")
	}

	customExplore := legacyExplore
	customExplore.Instructions = "My custom explore rules"
	if got, changed := upgradeDefaultRole(customExplore); changed || got.Instructions != customExplore.Instructions {
		t.Fatalf("customized explore role was overwritten: %+v (changed=%v)", got, changed)
	}

	limit := researchMaxTurns
	legacyResearch := protocol.RoleProfile{
		ID: protocol.NewRoleID(), Revision: 1, Key: "research", Name: "Research",
		Appearance:  &protocol.ProjectsAppearance{Icon: "globe", Color: "violet"},
		Description: previousResearchDescription, ReadOnly: true, MaxTurns: &limit, Model: chosenModel,
		Skills: []string{"web-search:research"}, Instructions: researchInstructionsV1,
	}
	upgradedResearch, changed := upgradeDefaultRole(legacyResearch)
	if !changed || upgradedResearch.Instructions != researchInstructions || upgradedResearch.Description != researchDescription ||
		!reflect.DeepEqual(upgradedResearch.Model, chosenModel) {
		t.Fatalf("legacy research upgrade = %+v (changed=%v)", upgradedResearch, changed)
	}
	if _, again := upgradeDefaultRole(upgradedResearch); again {
		t.Fatal("upgraded research role was not idempotent")
	}

	storage, _, _, _, _ := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		pending, err := defaultsPending(ctx, db)
		if err != nil || pending {
			return err
		}
		var id string
		if err := db.QueryRowContext(ctx, "SELECT id FROM roles WHERE key='explore'").Scan(&id); err != nil {
			return err
		}
		legacyExplore.ID, err = protocol.ParseRoleID(id)
		if err != nil {
			return err
		}
		body, err := json.Marshal(legacyExplore)
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "UPDATE roles SET body=?1 WHERE id=?2", body, id); err != nil {
			return err
		}
		if pending, err := defaultsPending(ctx, db); err != nil || !pending {
			t.Fatalf("defaultsPending with legacy explore = %v, %v", pending, err)
		}
		if err := refreshAppearances(ctx, db); err != nil {
			return err
		}
		if pending, err := defaultsPending(ctx, db); err != nil || pending {
			t.Fatalf("defaultsPending after refresh = %v, %v", pending, err)
		}
		var refreshed []byte
		if err := db.QueryRowContext(ctx, "SELECT body FROM roles WHERE id=?1", id).Scan(&refreshed); err != nil {
			return err
		}
		var saved protocol.RoleProfile
		if err := json.Unmarshal(refreshed, &saved); err != nil {
			return err
		}
		if saved.Instructions != exploreInstructions || saved.MaxTurns == nil || *saved.MaxTurns != exploreMaxTurns || saved.Revision != legacyExplore.Revision+1 {
			t.Fatalf("refreshed explore role = %+v", saved)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
