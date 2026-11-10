package hostctl

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestHostTaskOptions(t *testing.T) {
	t.Parallel()
	session := "00000000-0000-0000-0000-000000000001"
	worktree := "00000000-0000-0000-0000-000000000002"
	request := "00000000-0000-0000-0000-000000000003"
	team := "00000000-0000-0000-0000-000000000004"
	for _, row := range []struct {
		name string
		args []string
		ok   bool
	}{
		{"existing", []string{"submit", "--session", session, "--prompt", "Task", "--request-id", request}, true},
		{"new", []string{"submit", "--worktree", worktree, "--stdin", "--no-wait", "--execution", "team", "--team", team, "--team-revision", "7"}, true},
		{"wait", []string{"wait", "--session", session, "--turn", request}, true},
		{"two_targets", []string{"submit", "--session", session, "--worktree", worktree, "--prompt", "Task"}, false},
		{"two_prompts", []string{"submit", "--session", session, "--stdin", "--prompt", "Task"}, false},
		{"session_config", []string{"submit", "--session", session, "--prompt", "Task", "--title", "Title"}, false},
		{"worktree_revision", []string{"submit", "--worktree", worktree, "--prompt", "Task", "--expected-revision", "1"}, false},
		{"team_revision_without_team", []string{"submit", "--worktree", worktree, "--prompt", "Task", "--team-revision", "1"}, false},
		{"noncanonical_team_revision", []string{"submit", "--worktree", worktree, "--prompt", "Task", "--team", team, "--team-revision", "01"}, false},
		{"model_with_team", []string{"submit", "--worktree", worktree, "--prompt", "Task", "--execution", "model", "--team", team}, false},
		{"blank", []string{"submit", "--session", session, "--prompt", " \n "}, false},
		{"zero_timeout", []string{"wait", "--session", session, "--turn", request, "--timeout", "0"}, false},
		{"relative_profile", []string{"wait", "--session", session, "--turn", request, "--data-dir", "relative"}, false},
	} {
		t.Run(row.name, func(t *testing.T) {
			options, help, err := ParseTask(row.args, strings.NewReader("Input from stdin"))
			if (err == nil) != row.ok || help {
				t.Fatalf("parse=%v help=%v", err, help)
			}
			if err != nil {
				return
			}
			body, err := json.Marshal(options.Task)
			if err != nil {
				t.Fatal(err)
			}
			var decoded Task
			if json.Unmarshal(body, &decoded) != nil || decoded.Version != 1 || decoded.Timeout != 600 || decoded.Command != options.Task.Command {
				t.Fatalf("invalid source task envelope: %s", body)
			}
			if row.name == "existing" && decoded.Request.String() != request {
				t.Fatal("original durable request identity changed")
			}
			if row.name == "new" && (decoded.Wait || decoded.Target.Team == nil || decoded.Target.Team.Revision == nil || *decoded.Target.Team.Revision != 7) {
				t.Fatal("new task lost its frozen team selection")
			}
			if row.name == "existing" {
				known := options.Task.Result()
				incoming := known
				node := protocol.NodeID{1}
				incoming.Node, incoming.State = &node, "uncertain"
				if !matchesResponse(known, incoming) || incoming.ExitCode() != 4 {
					t.Fatal("uncertain result lost the submission identity")
				}
				other := protocol.RequestID{9}
				incoming.Request = &other
				if matchesResponse(known, incoming) {
					t.Fatal("accepted another request's outcome")
				}
			}
			if row.name == "wait" {
				result := options.Task.Result()
				node := protocol.NodeID{1}
				result.Node = &node
				body, err := json.Marshal(result)
				if err != nil {
					t.Fatal(err)
				}
				var restored TaskResult
				if err := json.Unmarshal(body, &restored); err != nil || restored.Request != nil || restored.Error != nil {
					t.Fatalf("nullable source result fields changed: %v", err)
				}
			}
		})
	}
	for _, row := range []struct {
		args []string
		ok   bool
		help bool
	}{
		{nil, true, false},
		{[]string{"--help"}, true, true},
		{[]string{"--help", "--internet"}, false, false},
		{[]string{"--bind", "127.0.0.1:0", "--internet"}, false, false},
		{[]string{"--internet", "--relay", "https://relay.example"}, false, false},
		{[]string{"--bootstrap", "--pairing-service", "https://pair.example"}, false, false},
		{[]string{"--relay", "https://one.example", "--relay", "https://two.example"}, true, false},
		{[]string{"--pairing-service", "http://127.0.0.1:8080", "--telemetry", "off"}, true, false},
		{[]string{"--pairing-service", "http://pair.example"}, false, false},
		{[]string{"--pairing-service", "https://pair.example/path"}, false, false},
		{[]string{"--telemetry", "false"}, false, false},
	} {
		options, help, err := ParseService(row.args)
		if (err == nil) != row.ok || help != row.help {
			t.Fatalf("service args=%v err=%v help=%v", row.args, err, help)
		}
		if err == nil && !help && options.Profile == "" {
			t.Fatal("service lost the shared default profile")
		}
	}
}
