package preferences

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

func TestPrefsRoundTrip(t *testing.T) {
	t.Parallel()
	fixture, err := os.ReadFile("testdata/preferences-rust.json")
	if err != nil {
		t.Fatal(err)
	}
	decode := func(t *testing.T, b []byte) Data {
		t.Helper()
		var d Data
		if err := json.Unmarshal(b, &d); err != nil {
			t.Fatal(err)
		}
		return d
	}
	jsonValue := func(t *testing.T, b []byte) any {
		t.Helper()
		decoder := json.NewDecoder(bytes.NewReader(b))
		decoder.UseNumber()
		var v any
		if err := decoder.Decode(&v); err != nil {
			t.Fatal(err)
		}
		return v
	}
	blur := float32(25)
	for _, test := range []struct {
		name string
		blur *float32
	}{{name: "Rust Data"}, {name: "Go blur", blur: &blur}} {
		t.Run(test.name, func(t *testing.T) {
			d := decode(t, fixture)
			if test.blur != nil {
				d.Surfaces.Blur = test.blur
			}
			encoded, err := json.Marshal(d)
			if err != nil {
				t.Fatal(err)
			}
			want := jsonValue(t, fixture).(map[string]any)
			if test.blur != nil {
				want["surfaces"].(map[string]any)["blur"] = json.Number("25")
			}
			if !reflect.DeepEqual(jsonValue(t, encoded), want) {
				t.Fatal("controller save lost or changed a Rust Data field")
			}
			if test.blur == nil {
				pretty, err := json.MarshalIndent(d, "", "  ")
				if err != nil {
					t.Fatal(err)
				}
				if !bytes.Equal(pretty, bytes.TrimSpace(fixture)) {
					t.Fatal("Go bytes differ from the Rust-written controller fixture")
				}
			}
			if test.blur == nil && strings.Contains(string(encoded), `"blur"`) {
				t.Fatal("unset Go extension was written")
			}
			materials := d.Materials()
			if materials.Main != float32(.835) || materials.Sidebar != float32(.45) || test.blur != nil && materials.Blur != float32(.25) {
				t.Fatalf("percent conversion: %+v", materials)
			}
			s := Open(filepath.Join(t.TempDir(), "desktop", "preferences.json"))
			s.Data = d
			if err := s.Save(); err != nil {
				t.Fatal(err)
			}
			first, err := os.ReadFile(s.Path)
			if err != nil {
				t.Fatal(err)
			}
			if err := s.Save(); err != nil {
				t.Fatal(err)
			}
			second, err := os.ReadFile(s.Path)
			if err != nil {
				t.Fatal(err)
			}
			if !bytes.Equal(first, second) || !reflect.DeepEqual(Open(s.Path).Data, d) {
				t.Fatal("preferences were not deterministic across relaunch")
			}
		})
	}
	t.Run("legacy and missing optional fields", func(t *testing.T) {
		var fields map[string]json.RawMessage
		if err := json.Unmarshal(fixture, &fields); err != nil {
			t.Fatal(err)
		}
		fields["sessions"] = fields["pins"]
		delete(fields, "pins")
		delete(fields, "sidebar_metrics")
		delete(fields, "theme_effects")
		encoded, err := json.Marshal(fields)
		if err != nil {
			t.Fatal(err)
		}
		d := decode(t, encoded)
		if d.Pins == nil || len(*d.Pins) != 2 || d.SidebarMetrics || d.ThemeEffects != nil {
			t.Fatal("legacy alias or omitted defaults changed")
		}
	})
	t.Run("serde required duplicate and null rules", func(t *testing.T) {
		var original map[string]json.RawMessage
		if err := json.Unmarshal(fixture, &original); err != nil {
			t.Fatal(err)
		}
		for _, row := range []struct{ name, field, value string }{
			{"terminal required", "terminal", `{"font_size":14}`},
			{"device required", "devices", `{"node":{"name":null,"platform":null}}`},
			{"file picker required", "file_picker", `{"grid":true}`},
			{"theme required", "appearance", `{"package":"builtin"}`},
			{"composer required", "composer", `{"model":null,"mode":"code"}`},
			{"dictation required", "dictation", `{"microphone":null}`},
			{"pin required", "pins", `{"node/session":{"order":null}}`},
			{"directory required", "plugin_directories", `{"node/plugin":{"path":"/fixture"}}`},
			{"artifact unknown", "artifact_drafts", `{"node/document/1/form":{"form":"form","fields":[],"extra":true}}`},
			{"artifact field required", "artifact_drafts", `{"node/document/1/form":{"form":"form","fields":[{"name":"choice"}]}}`},
			{"required null", "sidebar_metrics", `null`},
		} {
			t.Run(row.name, func(t *testing.T) {
				fields := map[string]json.RawMessage{}
				for k, v := range original {
					fields[k] = v
				}
				fields[row.field] = json.RawMessage(row.value)
				b, err := json.Marshal(fields)
				if err != nil {
					t.Fatal(err)
				}
				var d Data
				if json.Unmarshal(b, &d) == nil {
					t.Fatal("accepted a value rejected by the serde controller format")
				}
			})
		}
		for _, addition := range []string{`,"version":1`, `,"sessions":null`, `,"sessions":null,"sessions":null`} {
			base := bytes.TrimSpace(fixture)
			b := append(append([]byte(nil), base[:len(base)-1]...), []byte(addition+"}")...)
			var d Data
			if json.Unmarshal(b, &d) == nil {
				t.Fatal("duplicate controller field or pin alias was accepted")
			}
		}
	})
	t.Run("unreadable and failed save preserve data", func(t *testing.T) {
		path := filepath.Join(t.TempDir(), "preferences.json")
		bad := bytes.Replace(fixture, []byte(`"message_text_size": 16`), []byte(`"message_text_size": 15`), 1)
		if err := os.WriteFile(path, bad, 0o600); err != nil {
			t.Fatal(err)
		}
		s := Open(path)
		if s.Readable || s.Save() == nil {
			t.Fatal("unreadable preferences became writable")
		}
		got, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		if !bytes.Equal(got, bad) {
			t.Fatal("invalid preferences were overwritten")
		}
		parent := filepath.Join(t.TempDir(), "not-a-directory")
		if err := os.WriteFile(parent, []byte("kept"), 0o600); err != nil {
			t.Fatal(err)
		}
		s = Open(filepath.Join(parent, "preferences.json"))
		s.Readable = true
		s.Data = decode(t, fixture)
		if s.Save() == nil || s.Data.ToastSeconds != 8 {
			t.Fatal("failed save discarded the preference draft")
		}
	})
	t.Run("startup and shared window write order", func(t *testing.T) {
		profile := t.TempDir()
		s := ForStartup(profile)
		if s.Data.WelcomeSeen == nil || *s.Data.WelcomeSeen {
			t.Fatal("first launch did not preserve onboarding")
		}
		if err := os.MkdirAll(filepath.Join(profile, "storage"), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(profile, "storage", "node.sqlite"), nil, 0o600); err != nil {
			t.Fatal(err)
		}
		if previous := ForStartup(profile); previous.Data.WelcomeSeen == nil || !*previous.Data.WelcomeSeen {
			t.Fatal("existing fallback storage was treated as a new profile")
		}
		s.Data = decode(t, fixture)
		old, err := json.Marshal(s.Data)
		if err != nil {
			t.Fatal(err)
		}
		oldVersion := s.Changed()
		s.Data.ToastSeconds = 10
		latest, err := json.Marshal(s.Data)
		if err != nil {
			t.Fatal(err)
		}
		latestVersion := s.Changed()
		if err := s.WriteSnapshot(latestVersion, latest); err != nil {
			t.Fatal(err)
		}
		if err := s.WriteSnapshot(oldVersion, old); err != nil {
			t.Fatal(err)
		}
		got, err := os.ReadFile(s.Path)
		if err != nil {
			t.Fatal(err)
		}
		if !bytes.Equal(got, latest) {
			t.Fatal("an older window overwrote a newer controller edit")
		}
	})
}
