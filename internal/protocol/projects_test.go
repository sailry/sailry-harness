package protocol

import (
	"encoding/json"
	"testing"
)

func TestProjectCreationVisibility(t *testing.T) {
	t.Parallel()
	if DefaultProjectsDraft().Visibility != ProjectVisibilityShared || DefaultCommandRegisterProjectPayload().Visibility != ProjectVisibilityShared {
		t.Fatal("new project defaults are not shared")
	}
	for _, kind := range []string{"create_project", "register_project"} {
		t.Run(kind, func(t *testing.T) {
			for _, visibility := range []ProjectVisibility{"", ProjectVisibilityPrivate, ProjectVisibilityShared} {
				var payload any = CommandRegisterProjectPayload{Name: "New", Path: "/new", Visibility: visibility}
				if kind == "create_project" {
					payload = ProjectsDraft{Name: "New", Path: "/new", Appearance: DefaultProjectsAppearance(), Source: ProjectsSource{Kind: "local"}, Visibility: visibility}
				}
				body, err := json.Marshal(Command{Kind: kind, Data: payload})
				if err != nil {
					t.Fatal(err)
				}
				var wire struct {
					Data map[string]json.RawMessage `json:"data"`
				}
				if err := json.Unmarshal(body, &wire); err != nil {
					t.Fatal(err)
				}
				want := visibility
				if want == "" {
					want = ProjectVisibilityShared
				}
				// Explicit values preserve intent with peers using either creation default.
				if string(wire.Data["visibility"]) != `"`+string(want)+`"` {
					t.Fatalf("%s encoded visibility %q as %s", kind, visibility, body)
				}
				check := func(body []byte, want ProjectVisibility) {
					t.Helper()
					var decoded Command
					if err := json.Unmarshal(body, &decoded); err != nil {
						t.Fatal(err)
					}
					var got ProjectVisibility
					switch value := decoded.Data.(type) {
					case *ProjectsDraft:
						got = value.Visibility
					case *CommandRegisterProjectPayload:
						got = value.Visibility
					}
					if got != want {
						t.Fatalf("%s visibility = %q, want %q", kind, got, want)
					}
				}
				check(body, want)
				delete(wire.Data, "visibility")
				legacy := map[string]any{"kind": kind, "data": wire.Data}
				body, err = json.Marshal(legacy)
				if err != nil {
					t.Fatal(err)
				}
				check(body, ProjectVisibilityShared)
			}
		})
	}
}

func TestStoredProjectKeepsLegacyPrivateVisibility(t *testing.T) {
	t.Parallel()
	if DefaultProject().Visibility != ProjectVisibilityPrivate {
		t.Fatal("stored project default changed")
	}
	body := []byte(`{"id":"00000000-0000-0000-0000-000000000001","name":"Existing","path":"/existing","appearance":{"icon":"folder","color":"none"}}`)
	for _, tolerance := range []Tolerance{Authored, PeerCommand, PeerData} {
		var project Project
		if err := DecodeAs(tolerance, body, &project); err != nil {
			t.Fatal(err)
		}
		if project.Visibility != ProjectVisibilityPrivate {
			t.Fatalf("stored project became %q", project.Visibility)
		}
		encoded, err := json.Marshal(project)
		if err != nil {
			t.Fatal(err)
		}
		var fields map[string]json.RawMessage
		if err := json.Unmarshal(encoded, &fields); err != nil {
			t.Fatal(err)
		}
		if _, exists := fields["visibility"]; exists {
			t.Fatal("legacy private project serialization changed")
		}
	}
}
