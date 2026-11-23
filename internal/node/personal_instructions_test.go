package node

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/agent/prompt"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPersonalInstructionsCommands(t *testing.T) {
	t.Parallel()
	node := nodeFixture(t)
	root := node.profile.Ownership.Path
	value := func(data any) protocol.PersonalInstructions {
		t.Helper()
		v, ok := dispatchPayload[protocol.PersonalInstructions](data)
		if !ok {
			t.Fatalf("unexpected personal instructions payload %T", data)
		}
		return v
	}
	read := func() protocol.PersonalInstructions {
		t.Helper()
		return value(commandFixture(t, node, protocol.Command{Kind: "read_personal_instructions"}).Data)
	}
	save := func(text string, expected *string) protocol.Request {
		return protocol.NewRequest(node.ID(), protocol.Command{Kind: "save_personal_instructions", Data: protocol.CommandSavePersonalInstructionsPayload{Text: text, ExpectedRevision: expected}})
	}
	if got := read(); got.Text != "" || got.Revision != nil {
		t.Fatalf("unconfigured profile = %+v", got)
	}
	if initial := dispatchFixture(t, node, node.ID(), save("  \n", nil)); initial.Fault != nil {
		t.Fatal(initial.Fault)
	}
	if conflict := dispatchFixture(t, node, node.ID(), save("new", nil)); conflict.Fault == nil || conflict.Fault.Code != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("overwrite without revision: %+v", conflict)
	}
	// Existing files bigger than the ordinary file-preview limit are editable in full.
	old := strings.Repeat("word ", protocol.MAX_FILE_BYTES/5+1) + "\n"
	path := filepath.Join(root, "instructions.md")
	if err := os.WriteFile(path, []byte(old), 0600); err != nil {
		t.Fatal(err)
	}
	initial := read()
	if initial.Text != old || initial.Revision == nil {
		t.Fatalf("existing profile returned %d bytes", len(initial.Text))
	}
	proposals := []protocol.Request{save("First preference.\n", initial.Revision), save("Second preference.\n", initial.Revision)}
	type attempt struct {
		request protocol.Request
		result  link.Response
		err     error
	}
	results := make(chan attempt, len(proposals))
	for _, r := range proposals {
		go func() {
			admission, err := node.Dispatch(t.Context(), node.ID(), r)
			attempt := attempt{request: r, err: err}
			if err == nil {
				attempt.result = <-admission.Completion
			}
			results <- attempt
		}()
	}
	winners, conflicts := 0, 0
	var accepted protocol.Request
	var state protocol.PersonalInstructions
	for range proposals {
		a := <-results
		if a.err != nil {
			t.Fatal(a.err)
		}
		if a.result.Fault == nil {
			winners++
			accepted = a.request
			state = value(a.result.Output.Data)
		} else if a.result.Fault.Code == protocol.ErrorCodeRevisionConflict {
			conflicts++
		} else {
			t.Fatal(a.result.Fault)
		}
	}
	if winners != 1 || conflicts != 1 || state.Revision == nil {
		t.Fatalf("winners=%d conflicts=%d", winners, conflicts)
	}
	captured, err := prompt.CapturePersonal(t.Context(), root)
	if err != nil || !strings.HasSuffix(captured, state.Text) {
		t.Fatalf("prompt did not see winning preference: %q, %v", captured, err)
	}
	if cleared := dispatchFixture(t, node, node.ID(), save("", state.Revision)); cleared.Fault != nil {
		t.Fatal(cleared.Fault)
	}
	replay := dispatchFixture(t, node, node.ID(), accepted)
	if replay.Fault != nil || value(replay.Output.Data).Text != state.Text || read().Text != "" {
		t.Fatalf("durable replay changed the cleared preference: %+v", replay)
	}
	if captured, err = prompt.CapturePersonal(t.Context(), root); err != nil || captured != "" {
		t.Fatalf("cleared prompt = %q, %v", captured, err)
	}
	cleared := read()
	if err = os.Remove(path); err != nil {
		t.Fatal(err)
	}
	if got := dispatchFixture(t, node, node.ID(), save("stale", cleared.Revision)); got.Fault == nil || got.Fault.Code != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("externally deleted file accepted stale save: %+v", got)
	}
}
