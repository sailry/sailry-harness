package model

import (
	"encoding/json"
	"slices"
	"testing"

	"charm.land/fantasy"
)

func TestChatSchemaCollapsesNullableRoleEnum(t *testing.T) {
	t.Parallel()
	raw := json.RawMessage(`{"type":"object","additionalProperties":false,"required":["task"],"properties":{"role":{"anyOf":[{"type":"string","enum":["explore","reviewer"]},{"type":"null"}],"description":"Exact roster role key; omit or pass null to use this turn's model"},"task":{"type":"string"}}}`)
	adapted, err := declarationSchema(fantasy.Call{}, "spawn_agent", raw, true)
	if err != nil {
		t.Fatal(err)
	}
	var doc struct {
		Properties map[string]struct {
			Type  string   `json:"type"`
			Enum  []string `json:"enum"`
			AnyOf any      `json:"anyOf"`
		} `json:"properties"`
	}
	if err := json.Unmarshal(adapted, &doc); err != nil {
		t.Fatal(err)
	}
	role, ok := doc.Properties["role"]
	if !ok {
		t.Fatalf("role property missing after chat adaptation: %s", adapted)
	}
	if role.Type != "string" || role.AnyOf != nil || !slices.Equal(role.Enum, []string{"explore", "reviewer"}) {
		t.Fatalf("unexpected adapted role schema: %+v (raw=%s)", role, adapted)
	}
}
