package protocol

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestNilIdentifierRejected(t *testing.T) {
	t.Parallel()
	for _, identifier := range []struct {
		name   string
		target any
	}{
		{"request", new(RequestID)}, {"project", new(ProjectID)},
		{"worktree", new(WorktreeID)}, {"session", new(SessionID)},
		{"turn", new(TurnID)}, {"approval", new(ApprovalID)},
		{"checkpoint", new(CheckpointID)}, {"question", new(QuestionID)},
		{"terminal", new(TerminalID)}, {"stream", new(StreamID)},
		{"attachment", new(AttachmentID)}, {"artifact", new(ArtifactID)},
		{"credential", new(CredentialID)}, {"role", new(RoleID)},
		{"team", new(TeamID)}, {"ssh", new(SshID)},
		{"database", new(DatabaseID)}, {"provider", new(ProviderID)},
		{"event", new(EventID)}, {"job", new(JobID)},
		{"knowledge", new(KnowledgeID)}, {"model", new(ModelID)},
		{"notification", new(NotificationID)}, {"schedule", new(ScheduleID)},
	} {
		t.Run(identifier.name, func(t *testing.T) {
			if err := json.Unmarshal([]byte(`"00000000-0000-0000-0000-000000000000"`), identifier.target); err == nil {
				t.Fatal("nil identifier accepted")
			}
			encoded := []byte(`"01234567-89ab-4def-8123-456789abcdef"`)
			if err := json.Unmarshal(encoded, identifier.target); err != nil {
				t.Fatal(err)
			}
			actual, err := json.Marshal(identifier.target)
			if err != nil || string(actual) != string(encoded) {
				t.Fatalf("identifier round trip: %s, %v", actual, err)
			}
		})
	}
	for _, encoded := range []string{
		`"invalid"`, `null`, `42`,
		`"x11111111-1111-4111-8111-111111111111y"`,
		`"URN:UUID:11111111-1111-4111-8111-111111111111"`,
	} {
		var identifier RequestID
		if err := json.Unmarshal([]byte(encoded), &identifier); err == nil {
			t.Fatalf("invalid identifier accepted: %s", encoded)
		}
	}
}

func TestNodeIdArray(t *testing.T) {
	t.Parallel()
	var identity NodeID
	for index := range identity {
		identity[index] = byte(index)
	}
	const expected = `[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31]`
	encoded, err := json.Marshal(identity)
	if err != nil || string(encoded) != expected {
		t.Fatalf("node identity wire: %s, %v", encoded, err)
	}
	if actual := identity.String(); actual != "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f" {
		t.Fatalf("node identity display: %s", actual)
	}
	var decoded NodeID
	if err := json.Unmarshal(encoded, &decoded); err != nil || decoded != identity {
		t.Fatalf("node identity round trip: %s, %v", decoded, err)
	}
	for _, invalid := range []string{
		`null`, `[]`, `"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8="`,
		strings.TrimSuffix(expected, "]") + ",32]",
		strings.Replace(expected, ",31]", "]", 1),
		strings.Replace(expected, "[0,", "[null,", 1),
		strings.Replace(expected, "[0,", "[256,", 1),
		strings.Replace(expected, "[0,", "[-1,", 1),
		strings.Replace(expected, "[0,", "[0.5,", 1),
	} {
		if err := json.Unmarshal([]byte(invalid), &decoded); err == nil {
			t.Fatalf("invalid node identity accepted: %s", invalid)
		}
	}
}
