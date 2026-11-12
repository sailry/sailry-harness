package protocol

import (
	"encoding"
	"encoding/json"
	"reflect"
	"strings"
	"testing"
)

// future returns value's JSON with the object at path changed as a build
// that declares more would have written it. A path element is a member name
// or an array index.
func future(t *testing.T, value any, change func(object map[string]any), path ...any) []byte {
	t.Helper()
	encoded, err := json.Marshal(value)
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	root := fixtureJSON(t, encoded)
	current := root
	for _, step := range path {
		switch step := step.(type) {
		case string:
			current = current.(map[string]any)[step]
		case int:
			current = current.([]any)[step]
		}
	}
	change(current.(map[string]any))
	encoded, err = json.Marshal(root)
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	return encoded
}

func set(name string, value any) func(map[string]any) {
	return func(object map[string]any) { object[name] = value }
}

// TestPeerTolerance decodes what a build of the same protocol version that
// declares more may send. A peer's report decodes; the same bytes as authored
// input are refused, as they were before peers were told apart.
func TestPeerTolerance(t *testing.T) {
	t.Parallel()
	extension := DefaultPluginExtension()
	extension.ApiVersion = "v1"
	tool := DefaultToolDeclaration()
	tool.Name, tool.Handler = "inspect", &ToolHandler{Name: "run", Parameters: json.RawMessage(`{"type":"object"}`)}
	extension.Tools = []ToolDeclaration{tool}
	task := DefaultConversationBackgroundTask()
	task.ID, task.Title, task.State = "task", "Build", ConversationBackgroundStateRunning
	task.Session, task.Turn = SessionID{1}, TurnID{2}
	task.Kind = ConversationBackgroundKind{Kind: "agent", Data: &ConversationBackgroundKindAgentPayload{Session: SessionID{3}, Turn: TurnID{4}}}
	page := DefaultConversationPage()
	page.Session = task.Session
	page.Background = []ConversationBackgroundTask{task}
	page.Queue.Paused = true
	backup := DefaultBackupSettings()
	record := BackupRecord{ID: "backup", Sha256: strings.Repeat("0", 64)}
	listing := DefaultFileBrowserListing()
	listing.Separator = "/"
	listing.Locations = []FileBrowserLocation{{Kind: FileBrowserLocationKindHome, Name: "Home", Path: "/home"}}
	fault := Fault{Code: ErrorCodeConflict, Message: "continuation refused", Details: &FaultDetails{Kind: "storage_conflict"}}

	paused := DefaultConversationCompactionFailure()
	paused.Source, paused.Stage, paused.Reason = ConversationCompactionSourceAutomatic, ConversationCompactionStageRequest, ConversationCompactionReasonRequestFailed
	paused.Paused = true

	cases := []struct {
		name string
		data []byte
		// target returns the value to decode into; check inspects it after a
		// tolerant decode and returns the JSON it must encode to, or nil when
		// only the decode is specified.
		target func() any
		check  func(t *testing.T, value any)
		// open marks input the authored decoder accepted before: an unknown
		// member of an object that was never closed.
		open bool
	}{
		{
			name:   "extension with an unknown member",
			data:   future(t, extension, set("future_feature", map[string]any{"enabled": true})),
			target: func() any { return new(PluginExtension) },
			check: func(t *testing.T, value any) {
				if got := value.(*PluginExtension); got.ApiVersion != "v1" || len(got.Tools) != 1 {
					t.Fatalf("extension lost its declared members: %+v", got)
				}
			},
		},
		{
			name:   "tool handler with an unknown member",
			data:   future(t, extension, set("future_gate", true), "tools", 0, "handler"),
			target: func() any { return new(PluginExtension) },
			check: func(t *testing.T, value any) {
				if got := value.(*PluginExtension).Tools[0].Handler; got == nil || got.Name != "run" {
					t.Fatalf("handler lost its declared members: %+v", got)
				}
			},
		},
		{
			name:   "background task with an unknown hold",
			data:   future(t, page, set("hold", "future_hold"), "background", 0),
			target: func() any { return new(ConversationPage) },
			check: func(t *testing.T, value any) {
				hold := value.(*ConversationPage).Background[0].Hold
				if hold == nil || *hold != "future_hold" {
					t.Fatalf("hold = %v, want the peer's value", hold)
				}
			},
		},
		{
			name:   "background task with an unknown member",
			data:   future(t, page, set("future_progress", 3), "background", 0),
			target: func() any { return new(ConversationPage) },
			check: func(t *testing.T, value any) {
				if got := value.(*ConversationPage).Background[0]; got.ID != "task" || got.Hold != nil {
					t.Fatalf("task = %+v", got)
				}
			},
		},
		{
			name:   "queue with an unknown pause reason",
			data:   future(t, page, set("pause_reason", "future_reason"), "queue"),
			target: func() any { return new(ConversationPage) },
			check: func(t *testing.T, value any) {
				reason := value.(*ConversationPage).Queue.PauseReason
				if reason == nil || *reason != "future_reason" {
					t.Fatalf("pause reason = %v, want the peer's value", reason)
				}
			},
		},
		{
			name: "backup settings with unknown members",
			data: future(t, backup, func(object map[string]any) {
				object["future_retention"], object["future_last"] = 7, map[string]any{"at": 1}
			}),
			target: func() any { return new(BackupSettings) },
			check: func(t *testing.T, value any) {
				if got := value.(*BackupSettings); got.Revision != backup.Revision || got.Last != nil {
					t.Fatalf("settings = %+v", got)
				}
			},
		},
		{
			name:   "backup record with an unknown member",
			data:   future(t, record, set("future_verified", true)),
			target: func() any { return new(BackupRecord) },
			check: func(t *testing.T, value any) {
				if got := value.(*BackupRecord); got.ID != "backup" {
					t.Fatalf("record = %+v", got)
				}
			},
		},
		{
			name:   "fault with an unknown details kind",
			data:   future(t, fault, func(object map[string]any) { object["kind"], object["retry_ms"] = "future_details", 500 }, "details"),
			target: func() any { return new(Fault) },
			check: func(t *testing.T, value any) {
				got := value.(*Fault)
				if got.Code != ErrorCodeConflict || got.Message != "continuation refused" || got.Details == nil || got.Details.Kind != "future_details" {
					t.Fatalf("fault = %+v", got)
				}
				content, ok := got.Details.Data.(UnknownVariant)
				if !ok || !reflect.DeepEqual(fixtureJSON(t, content), map[string]any{"kind": "future_details", "retry_ms": json.Number("500")}) {
					t.Fatalf("details content = %#v", got.Details.Data)
				}
			},
		},
		{
			name:   "location with an unknown kind",
			data:   future(t, listing, set("kind", "future_place"), "locations", 0),
			target: func() any { return new(FileBrowserListing) },
			check: func(t *testing.T, value any) {
				if got := value.(*FileBrowserListing).Locations[0]; got.Kind != "future_place" || got.Path != "/home" {
					t.Fatalf("location = %+v", got)
				}
			},
		},
		{
			name:   "location with an unknown medium and member",
			data:   future(t, listing, func(object map[string]any) { object["medium"], object["future_capacity"] = "future_medium", 1 }, "locations", 0),
			target: func() any { return new(FileBrowserListing) },
			check: func(t *testing.T, value any) {
				medium := value.(*FileBrowserListing).Locations[0].Medium
				if medium == nil || *medium != "future_medium" {
					t.Fatalf("medium = %v", medium)
				}
			},
		},
		{
			name:   "decision question with an unknown member",
			data:   []byte(`{"state":"Page","questions":{"visible":{"type":"noul","instructions":"Is Save visible?","future_weight":2}}}`),
			target: func() any { return new(DecisionRequest) },
			check: func(t *testing.T, value any) {
				if got := value.(*DecisionRequest).Questions["visible"]; got.Kind != "noul" {
					t.Fatalf("question = %+v", got)
				}
			},
		},
		{
			name:   "conversation part of an unknown resource type",
			data:   []byte(`{"kind":"resource","data":{"type":"future_resource","limit":3}}`),
			target: func() any { return new(ConversationPart) },
			check: func(t *testing.T, value any) {
				got := value.(*ConversationPart)
				if raw, ok := got.Data.(*json.RawMessage); got.Kind != "resource" || !ok || !strings.Contains(string(*raw), "future_resource") {
					t.Fatalf("part = %+v", got)
				}
			},
			open: true,
		},
		{
			name:   "conversation part of an unknown kind",
			data:   []byte(`{"kind":"future_part","data":{"text":"later"}}`),
			target: func() any { return new(ConversationPart) },
			check: func(t *testing.T, value any) {
				got := value.(*ConversationPart)
				if _, unknown := got.Data.(UnknownVariant); got.Kind != "future_part" || !unknown {
					t.Fatalf("part = %+v", got)
				}
			},
		},
		{
			name:   "update of an unknown kind",
			data:   []byte(`{"kind":"future_update","data":{"session":"s"}}`),
			target: func() any { return new(Update) },
			check: func(t *testing.T, value any) {
				got := value.(*Update)
				if _, unknown := got.Data.(UnknownVariant); got.Kind != "future_update" || !unknown {
					t.Fatalf("update = %+v", got)
				}
			},
		},
		{
			name:   "output wrapping a tolerated value",
			data:   future(t, Output{Kind: "backup", Data: &record}, set("future_verified", true), "data"),
			target: func() any { return new(Output) },
			check: func(t *testing.T, value any) {
				if got, ok := value.(*Output).Data.(*BackupRecord); !ok || got.ID != "backup" {
					t.Fatalf("output = %+v", value)
				}
			},
		},
		{
			name: "values inside a struct without a decoder",
			data: []byte(`{"error":{"code":"conflict","message":"refused","details":{"kind":"future_details"}},"connected":true,"future_state":1}`),
			target: func() any {
				return new(struct {
					Error     *Fault `json:"error"`
					Connected bool   `json:"connected"`
				})
			},
			check: func(*testing.T, any) {},
		},
		{
			name:   "compaction failure that paused, with an unknown member",
			data:   future(t, paused, set("future_resume_ms", 5)),
			target: func() any { return new(ConversationCompactionFailure) },
			check: func(t *testing.T, value any) {
				if got := value.(*ConversationCompactionFailure); !got.Paused || got.Reason != ConversationCompactionReasonRequestFailed {
					t.Fatalf("failure = %+v", got)
				}
			},
		},
		{
			// A build that does not declare the member writes none: as before.
			name:   "compaction failure without its paused member",
			data:   []byte(`{"source":"automatic","stage":"request","reason":"request_failed","continued":false,"status":null}`),
			target: func() any { return new(ConversationCompactionFailure) },
			check: func(t *testing.T, value any) {
				got := value.(*ConversationCompactionFailure)
				encoded, err := json.Marshal(got)
				if err != nil || got.Paused || strings.Contains(string(encoded), "paused") {
					t.Fatalf("failure = %+v, encoded %s: %v", got, encoded, err)
				}
			},
			open: true,
		},
		{
			name:   "unknown member of an open object",
			data:   future(t, listing, set("future_capacity", 1), "locations", 0),
			target: func() any { return new(FileBrowserListing) },
			check:  func(*testing.T, any) {},
			open:   true,
		},
	}
	for _, item := range cases {
		t.Run(item.name, func(t *testing.T) {
			t.Parallel()
			value := item.target()
			if err := DecodeAs(PeerData, item.data, value); err != nil {
				t.Fatalf("a peer's report was refused: %v\n%s", err, item.data)
			}
			item.check(t, value)
			encoded, err := json.Marshal(value)
			if err != nil {
				t.Fatalf("the decoded report does not encode: %v", err)
			}
			again := item.target()
			if err := DecodeAs(PeerData, encoded, again); err != nil || !reflect.DeepEqual(value, again) {
				t.Fatalf("the report changed when it was passed on: %v\n%s", err, encoded)
			}
			if err := json.Unmarshal(item.data, item.target()); (err == nil) != item.open {
				t.Fatalf("authored decode: %v, accepted = %v, want %v", err, err == nil, item.open)
			}
		})
	}
}

// TestPeerCommandTolerance decodes what a peer asks this build to do: an
// option this build does not have is ignored, a value it cannot act on is
// refused.
func TestPeerCommandTolerance(t *testing.T) {
	t.Parallel()
	update := DefaultBackupUpdate()
	keep := PluginSettingsSecretUpdate{Kind: "keep"}
	update.Identity, update.Secret, update.SessionToken, update.Passphrase = keep, keep, keep, keep
	save := Command{Kind: "save_backup_settings", Data: &update}
	restore := Command{Kind: "restore_backup", Data: &CommandRestoreBackupPayload{ID: "backup", Location: BackupLocationLocal}}
	for _, item := range []struct {
		name     string
		data     []byte
		accepted bool
	}{
		{"a declared command", future(t, save, func(map[string]any) {}), true},
		{"an unknown member", future(t, save, set("future_retention", 7), "data"), true},
		{"an unknown enum value", future(t, restore, set("location", "future_location"), "data"), false},
		{"an unknown command", []byte(`{"kind":"future_command","data":{}}`), false},
		{"a missing required member", future(t, restore, func(object map[string]any) { delete(object, "id") }, "data"), false},
	} {
		t.Run(item.name, func(t *testing.T) {
			t.Parallel()
			var command Command
			if err := DecodeAs(PeerCommand, item.data, &command); (err == nil) != item.accepted {
				t.Fatalf("decode: %v, want accepted = %v\n%s", err, item.accepted, item.data)
			}
		})
	}
}

// TestToleranceReachesEveryObject adds an undeclared member to the objects of
// each fixture. A peer's report and a peer's command must still decode to the
// value the fixture alone decodes to, wherever the member was not kept as an
// entry of a map or part of a raw value.
func TestToleranceReachesEveryObject(t *testing.T) {
	t.Parallel()
	var widen func(value any) any
	widen = func(value any) any {
		switch value := value.(type) {
		case map[string]any:
			widened := map[string]any{}
			// An object with one member may be an externally tagged union,
			// whose form admits no second member. The added value is a
			// string so that a map of strings takes it as an entry.
			if len(value) != 1 {
				widened["future_member"] = "future"
			}
			for name, member := range value {
				widened[name] = widen(member)
			}
			return widened
		case []any:
			widened := make([]any, len(value))
			for index, item := range value {
				widened[index] = widen(item)
			}
			return widened
		}
		return value
	}
	eachFixture(t, func(t *testing.T, name string, original []byte, target func() any) {
		if name == "decision_request/questions" {
			// Its questions are a map of objects, which takes the added
			// string as a question. TestPeerTolerance covers a question.
			t.Skip("an added member would be a map entry of the wrong type")
		}
		widened, err := json.Marshal(widen(fixtureJSON(t, original)))
		if err != nil {
			t.Fatal(err)
		}
		for name, tolerance := range map[string]Tolerance{"report": PeerData, "command": PeerCommand} {
			value := target()
			if err := DecodeAs(tolerance, widened, value); err != nil {
				t.Errorf("%s with undeclared members was refused: %v", name, err)
				continue
			}
			plain := target()
			if err := DecodeAs(tolerance, original, plain); err != nil {
				t.Fatalf("%s fixture was refused: %v", name, err)
			}
			encoded, err := json.Marshal(value)
			if err != nil {
				t.Fatalf("encode: %v", err)
			}
			if strings.Contains(string(encoded), "future_member") {
				continue // kept as a map entry or inside a raw value
			}
			if !reflect.DeepEqual(value, plain) {
				t.Errorf("%s decoded to a different value with undeclared members", name)
			}
		}
	})
}

// TestUndeclaredResultVariantIsAnError keeps a peer's result that is neither
// Ok nor Err from ending the process that decodes it.
func TestUndeclaredResultVariantIsAnError(t *testing.T) {
	var outcome RequestOutcome
	if err := DecodeAs(PeerData, []byte(`{"kind":"completed","data":{"Later":{}}}`), &outcome); err == nil {
		t.Fatalf("an undeclared result variant decoded to %#v", outcome)
	}
	if err := DecodeAs(PeerData, []byte(`{"kind":"completed","data":{"Err":{"code":"cancelled","message":"stopped"}}}`), &outcome); err != nil {
		t.Fatal(err)
	}
	if result := outcome.Data.(*Result[Output, Fault]); result.Ok != nil || result.Err == nil || result.Err.Code != ErrorCodeCancelled {
		t.Fatalf("declared result: %#v", result)
	}
}

// TestLeaseEndsWithItsDecode keeps a tolerant decode from loosening a later
// authored decode of the same bytes.
func TestLeaseEndsWithItsDecode(t *testing.T) {
	data := []byte(`{"kind":"future_details"}`)
	var details FaultDetails
	if err := DecodeAs(PeerData, data, &details); err != nil {
		t.Fatal(err)
	}
	if leased.Load() != 0 {
		t.Fatalf("%d slices stayed leased", leased.Load())
	}
	if err := json.Unmarshal(data, &details); err == nil {
		t.Fatal("an authored decode kept the tolerance of an earlier decode")
	}
	if _, err := json.Marshal(FaultDetails{Kind: "future_details"}); err == nil {
		t.Fatal("an undeclared variant without a peer's content was encoded")
	}
	if _, err := json.Marshal(ConversationBackgroundTask{}); err == nil {
		t.Fatal("an unset enum was encoded")
	}
}

// The enums this build added have the text form their siblings have, with
// the same exactness: an undeclared name is refused.
func TestAddedEnumText(t *testing.T) {
	t.Parallel()
	for name, value := range map[string]interface {
		encoding.TextMarshaler
	}{
		"interrupted": ConversationQueuePauseInterrupted,
		"limit":       ConversationBackgroundHoldLimit,
		"stale":       ConversationContinuationRefusalStale,
	} {
		if text, err := value.MarshalText(); err != nil || string(text) != name {
			t.Errorf("%T text = %q, %v", value, text, err)
		}
	}
	for _, target := range []encoding.TextUnmarshaler{new(ConversationQueuePause), new(ConversationBackgroundHold), new(ConversationContinuationRefusal)} {
		if err := target.UnmarshalText([]byte("future")); err == nil {
			t.Errorf("%T accepted an undeclared name", target)
		}
	}
	var hold ConversationBackgroundHold
	if err := hold.UnmarshalText([]byte("failed")); err != nil || hold != ConversationBackgroundHoldFailed {
		t.Errorf("hold from text = %q, %v", hold, err)
	}
}

// A history entry's parents are optional: a Node that sends none leaves them
// unknown (nil), and a root commit's empty list is written and read back as a
// list, so a graph can tell the two apart.
func TestGitLogEntryParents(t *testing.T) {
	t.Parallel()
	entry := DefaultGitLogEntry()
	entry.ID, entry.Message = "a1", "Root"
	for _, item := range []struct {
		name    string
		parents []string
		member  string
	}{{"unknown", nil, ""}, {"root", []string{}, `"parents":[]`}, {"merge", []string{"b2", "c3"}, `"parents":["b2","c3"]`}} {
		t.Run(item.name, func(t *testing.T) {
			entry.Parents = item.parents
			encoded, err := json.Marshal(entry)
			if err != nil {
				t.Fatal(err)
			}
			if strings.Contains(string(encoded), `"parents"`) != (item.member != "") || !strings.Contains(string(encoded), item.member) {
				t.Fatalf("encoded %s", encoded)
			}
			var decoded GitLogEntry
			if err := json.Unmarshal(encoded, &decoded); err != nil {
				t.Fatal(err)
			}
			if (decoded.Parents == nil) != (item.parents == nil) || !reflect.DeepEqual(append([]string{}, decoded.Parents...), append([]string{}, item.parents...)) {
				t.Fatalf("decoded %#v, want %#v", decoded.Parents, item.parents)
			}
		})
	}
}
