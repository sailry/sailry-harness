package files

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCheckpointRestoreSettledVersions(t *testing.T) {
	t.Parallel()
	version := func(text string) *protocol.ConversationCheckpointVersion {
		return &protocol.ConversationCheckpointVersion{Revision: revision([]byte(text)), Size: uint64(len(text))}
	}
	// One turn changed the file from "first" to "second" and then to "third";
	// this is the later change.
	second := "second\n"
	later := protocol.ConversationCheckpointContent{File: protocol.ConversationCheckpointFile{Path: "note.txt", Before: version(second), After: *version("third\n")}, Before: &second, After: "third\n"}
	settled := []*protocol.ConversationCheckpointVersion{version("first\n"), version(second)}
	created := protocol.ConversationCheckpointContent{File: protocol.ConversationCheckpointFile{Path: "note.txt", After: *version("first\n")}, After: "first\n"}
	for _, test := range []struct {
		name, current string
		absent        bool
		content       protocol.ConversationCheckpointContent
		settled       []*protocol.ConversationCheckpointVersion
		want          string
		refused       bool
	}{
		{name: "the checkpoint's result is restored", current: "third\n", content: later, settled: settled, want: second},
		{name: "an already restored file is left", current: second, content: later, settled: settled, want: second},
		{name: "a file restored past this change is left", current: "first\n", content: later, settled: settled, want: "first\n"},
		{name: "another change is not overwritten", current: "independent\n", content: later, settled: settled, want: "independent\n", refused: true},
		{name: "a created file is deleted", current: "first\n", content: created, settled: []*protocol.ConversationCheckpointVersion{nil}, absent: false, want: ""},
		{name: "a deleted file stays deleted", absent: true, content: created, settled: []*protocol.ConversationCheckpointVersion{nil}, want: ""},
		{name: "an absent file is not a restored edit", absent: true, content: later, settled: settled, want: "", refused: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			root, err := filepath.EvalSymlinks(t.TempDir())
			if err != nil {
				t.Fatal(err)
			}
			path := filepath.Join(root, "note.txt")
			if !test.absent {
				if err := os.WriteFile(path, []byte(test.current), 0o600); err != nil {
					t.Fatal(err)
				}
			}
			restored, err := RestoreCheckpoint(context.Background(), root, test.content, test.settled)
			if (err != nil) != test.refused {
				t.Fatalf("restore: %+v, %v", restored, err)
			}
			data, err := os.ReadFile(path)
			if test.want == "" {
				if !os.IsNotExist(err) {
					t.Fatalf("file remained: %q, %v", data, err)
				}
				if !test.refused && restored.Revision != nil {
					t.Fatalf("deleted file has a revision: %+v", restored)
				}
				return
			}
			if err != nil || string(data) != test.want {
				t.Fatalf("content: %q, %v", data, err)
			}
			if !test.refused && (restored.Revision == nil || *restored.Revision != revision(data)) {
				t.Fatalf("restored revision: %+v", restored)
			}
		})
	}
}
