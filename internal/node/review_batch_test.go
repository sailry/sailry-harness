package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"charm.land/fantasy"
	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Each request consumes the real bundled plugin's preceding response. The
// compact receipt can succeed only if the full diff reached the model.
func TestReviewBatchRequests(t *testing.T) {
	t.Parallel()
	root, paths := reviewBatchFiles(t)
	script := &agentToolScript{}
	var scriptErr error
	var reviewID, token string
	var groups []string
	scope := operations.Alias("code-review", "review_scope")
	read := operations.Alias("code-review", "read_review_change")
	report := operations.Alias("code-review", "report_finding")
	complete := operations.Alias("code-review", "review_complete")
	script.reset(func(step int, results map[string]string) [][3]string {
		call := func(id, name string, arguments any) [][3]string {
			encoded, err := json.Marshal(arguments)
			if err != nil {
				scriptErr = err
				return nil
			}
			return [][3]string{{id, name, string(encoded)}}
		}
		decode := func(id string, value any) bool {
			if err := json.Unmarshal([]byte(results[id]), value); err != nil {
				scriptErr = fmt.Errorf("read %s result: %w", id, err)
				return false
			}
			return true
		}
		switch step {
		case 1:
			return call("discover", "tool_search", map[string]any{"query": "select:" + strings.Join([]string{scope, read, report, complete}, ",")})
		case 2:
			return call("scope", scope, map[string]any{})
		case 3:
			var snapshot struct {
				ReviewID string                        `json:"review_id"`
				Status   string                        `json:"status"`
				Next     *int                          `json:"next"`
				Files    []struct{ Path string }       `json:"files"`
				Groups   []struct{ ID string }         `json:"groups"`
				Rules    []struct{ Path, Text string } `json:"rules"`
			}
			if !decode("scope", &snapshot) {
				return nil
			}
			if snapshot.ReviewID == "" || snapshot.Status != "open" || snapshot.Next != nil ||
				len(snapshot.Files) != len(paths) || len(snapshot.Groups) != 3 || len(snapshot.Rules) != 1 ||
				snapshot.Rules[0].Path != "rules/go.md" || snapshot.Rules[0].Text == "" {
				scriptErr = fmt.Errorf("initial scope must contain the complete inventory and Go rule: %s", results["scope"])
				return nil
			}
			reviewID = snapshot.ReviewID
			selected := make([]string, 0, len(paths))
			for _, file := range snapshot.Files {
				selected = append(selected, file.Path)
			}
			for _, group := range snapshot.Groups {
				groups = append(groups, group.ID)
			}
			return call("read", read, map[string]any{"review_id": reviewID, "paths": selected})
		case 4:
			var batch struct {
				Token string `json:"delivery_token"`
				Next  *int   `json:"next"`
				Files []struct {
					Path, Revision string
					Next           *int
					Lines          []struct {
						Kind, Text string
						NewLine    int `json:"new_line"`
					}
				} `json:"files"`
			}
			if !decode("read", &batch) {
				return nil
			}
			if batch.Token == "" || batch.Next != nil || len(batch.Files) != len(paths) {
				scriptErr = fmt.Errorf("batch did not return the complete frozen diff and delivery token: %s", results["read"])
				return nil
			}
			token = batch.Token
			findings := []map[string]any{}
			for index, file := range batch.Files {
				if file.Path != paths[index] || file.Next != nil || file.Revision == "" {
					scriptErr = fmt.Errorf("invalid captured diff for %s", paths[index])
					return nil
				}
				if index != 0 && index != len(batch.Files)-1 {
					continue
				}
				for _, line := range file.Lines {
					if line.Kind == "added" && strings.Contains(line.Text, "return false") {
						findings = append(findings, map[string]any{"path": file.Path, "revision": file.Revision,
							"side": "new", "line": line.NewLine, "severity": "P1", "title": "Value disabled",
							"explanation": "The function now always rejects its previously enabled path.",
							"evidence":    "The frozen added line returns false unconditionally."})
					}
				}
			}
			if len(findings) != 2 {
				scriptErr = fmt.Errorf("captured diff has %d usable finding anchors, want 2", len(findings))
				return nil
			}
			return call("findings", report, map[string]any{"review_id": reviewID, "findings": findings})
		case 5:
			var findings struct{ Results []struct{ ID string } }
			if !decode("findings", &findings) {
				return nil
			}
			if len(findings.Results) != 2 || findings.Results[0].ID == "" || findings.Results[1].ID == "" {
				scriptErr = fmt.Errorf("batch findings were not recorded: %s", results["findings"])
				return nil
			}
			return call("complete", complete, map[string]any{"review_id": reviewID,
				"reviewed_groups": groups, "delivery_token": token, "finalize": true})
		case 6:
			var receipt struct {
				Status   string `json:"status"`
				Reviewed int    `json:"reviewed_files"`
				Findings int    `json:"findings"`
			}
			if decode("complete", &receipt) && (receipt.Status != "complete" || receipt.Reviewed != len(paths) || receipt.Findings != 2) {
				scriptErr = fmt.Errorf("unexpected final receipt: %s", results["complete"])
			}
		default:
			scriptErr = fmt.Errorf("unexpected review request %d", step)
		}
		return nil
	})
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{
		MCP: fixtureMCPOptions(), Environment: &environment,
		Process: process.Options{After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }},
		Model:   func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil },
	})
	if err != nil {
		t.Fatal(err)
	}
	defer node.Stop(context.Background())
	session := reviewSession(t, node, root, protocol.PermissionFull)
	ctx, cancel := context.WithTimeout(t.Context(), 45*time.Second)
	defer cancel()
	subscription, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
		Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Review these twelve changes"},
	}})
	for {
		var history protocol.ConversationHistory
		if err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			history, err = store.ReadConversation(ctx, db, session.ID, nil, 100)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		for _, approval := range history.Page.Approvals {
			if approval.State == protocol.ConversationApprovalStatePending {
				t.Fatalf("review unexpectedly asked for approval: %+v", approval)
			}
		}
		for _, run := range history.Page.Runs {
			switch run.Status {
			case protocol.ConversationStatusCompleted:
				script.mu.Lock()
				defer script.mu.Unlock()
				if scriptErr != nil || len(script.offered) != 6 || len(history.Page.Children) != 0 {
					t.Fatalf("review requests=%d, children=%d, error=%v", len(script.offered), len(history.Page.Children), scriptErr)
				}
				return
			case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted, protocol.ConversationStatusCancelled:
				t.Fatalf("review turn failed: %+v", run.Error)
			}
		}
		if _, err = subscription.Next(ctx); err != nil {
			t.Fatal(err)
		}
	}
}

func reviewBatchFiles(t *testing.T) (string, []string) {
	t.Helper()
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	repository, err := gogit.PlainInitWithOptions(root, &gogit.PlainInitOptions{InitOptions: gogit.InitOptions{DefaultBranch: plumbing.NewBranchReferenceName("main")}})
	if err != nil {
		t.Fatal(err)
	}
	worktree, err := repository.Worktree()
	if err != nil {
		t.Fatal(err)
	}
	paths := []string{}
	write := func(path string, index int, enabled bool) {
		t.Helper()
		body := fmt.Sprintf("package sample\n\nfunc value%d() bool { return %t }\n", index, enabled)
		if err = os.WriteFile(filepath.Join(root, path), []byte(body), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	for _, group := range []string{"alpha", "beta", "gamma"} {
		if err = os.Mkdir(filepath.Join(root, group), 0o700); err != nil {
			t.Fatal(err)
		}
		for index := range 4 {
			path := fmt.Sprintf("%s/value%d.go", group, index)
			write(path, index, true)
			if _, err = worktree.Add(path); err != nil {
				t.Fatal(err)
			}
			paths = append(paths, path)
		}
	}
	if _, err = worktree.Commit("Initial review fixture", &gogit.CommitOptions{Author: &object.Signature{Name: "Fixture", Email: "fixture@example.test", When: time.Unix(100, 0)}}); err != nil {
		t.Fatal(err)
	}
	for index, path := range paths {
		write(path, index%4, false)
	}
	return root, paths
}
