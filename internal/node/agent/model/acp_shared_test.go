package model

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestACPProgramServesSessions(t *testing.T) {
	t.Parallel()
	fixture := acpFixtureModel(t, "meta", nil, func(capture *ACPCapture) { capture.Adapter.Acp.Concurrency = 2 })
	other := cloneACPModel(t, fixture.model)
	binding, err := Capture(other.binding.Provider, other.binding.Config, other.binding.Secret, protocol.NewSessionID())
	if err != nil {
		t.Fatal(err)
	}
	language, err := NewACP(binding, other.capture)
	if err != nil {
		t.Fatal(err)
	}
	other = language.(*acpModel)

	// Two requests of different sessions are served by one program at once,
	// each in its own slot.
	first, agent, err := fixture.model.lease(t.Context(), false)
	if err != nil {
		t.Fatal(err)
	}
	second, shared, err := other.lease(t.Context(), false)
	if err != nil {
		t.Fatal(err)
	}
	if first != second || agent != shared {
		t.Fatal("a program with room for two requests started another")
	}
	mine, theirs := fixture.model.reserve(t.Context(), agent), other.reserve(t.Context(), shared)
	if mine == theirs || mine.owner == theirs.owner {
		t.Fatal("two sessions share one slot")
	}
	agent.give(mine)
	shared.give(theirs)
	fixture.model.capture.Pool.Release(first, true)
	other.capture.Pool.Release(second, true)

	// Each session prompts its own agent session in the same program.
	for _, model := range []*acpModel{fixture.model, other} {
		run := &acpFixture{t: t, model: model, dir: fixture.dir}
		if result := run.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure != nil || result.text != "ok" {
			t.Fatalf("reply = %#v", result)
		}
	}
	prompts := fixture.received("session/prompt")
	if len(fixture.received("initialize")) != 1 || len(fixture.received("session/new")) != 2 || len(prompts) != 2 {
		t.Fatalf("programs = %d, sessions = %d, prompts = %d", len(fixture.received("initialize")), len(fixture.received("session/new")), len(prompts))
	}
	session := func(index int) any { return prompts[index]["params"].(map[string]any)["sessionId"] }
	if session(0) == session(1) {
		t.Fatal("two Sailry sessions prompted one agent session")
	}
}
