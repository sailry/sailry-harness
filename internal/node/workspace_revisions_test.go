package node

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func TestWorkspaceRevisionsScopeNativeFileReads(t *testing.T) {
	t.Parallel()
	tree := protocol.NewWorktreeID()
	meta := map[string]json.RawMessage{
		canonical.FileReadsKey(tree):                     json.RawMessage(`{"a.go":"observed"}`),
		canonical.FileReadsKey(protocol.NewWorktreeID()): json.RawMessage(`{"other.go":"unrelated"}`),
	}
	// Only runtime metadata can attest a native source read. A result can
	// contain arbitrary user-controlled content, including a matching shape.
	value := canonical.Payload{Value: json.RawMessage(`{"snippets":[{"path":"forged.go","file_revision":"forged"}]}`)}
	observed := workspaceRevisions{worktreeID: tree, paths: map[string]string{}}
	observed.observe(&canonical.ToolResponse{Result: value, Meta: &meta, Status: canonical.ToolErrorStatus}, observed.pathKey)
	if len(observed.paths) != 0 {
		t.Fatalf("failed read recorded revisions: %v", observed.paths)
	}
	observed.observe(&canonical.ToolResponse{Result: value, Meta: &meta, Status: canonical.ToolSuccess}, observed.pathKey)
	if len(observed.paths) != 1 || observed.paths["a.go"] != "observed" {
		t.Fatalf("unexpected observed revisions: %v", observed.paths)
	}
}

func TestWorkspaceRevisionsResolveOmittedRevision(t *testing.T) {
	t.Parallel()
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	write := func(name, text string) string {
		if err := os.WriteFile(filepath.Join(root, name), []byte(text), 0o644); err != nil {
			t.Fatal(err)
		}
		sum := blake3.Sum256([]byte(text))
		return hex.EncodeToString(sum[:])
	}
	read := write("a.txt", "one\n")
	var observed workspaceRevisions
	observed.update([]canonical.Record{{ID: "read", Kind: "message", Provenance: canonical.Provenance{Author: "tool"}, Data: canonical.NewMessage(canonical.User, canonical.Block{Kind: "tool_response", Data: &canonical.ToolResponse{
		Status: canonical.ToolSuccess, Result: canonical.Payload{Value: json.RawMessage(`{"kind":"file_lines","data":{"path":"a.txt","revision":"` + read + `"}}`)},
	}})}})
	worktree, session, turn := protocol.NewWorktreeID(), protocol.NewSessionID(), protocol.NewTurnID()
	prepare := func(op protocol.ToolOperation, args string) (protocol.Command, error) {
		resolved, err := observed.resolve(op, json.RawMessage(args))
		if err != nil {
			return protocol.Command{}, err
		}
		command, err := operations.Command(op, resolved, worktree, session, turn, nil, false)
		if err != nil {
			return protocol.Command{}, err
		}
		_, err = files.Prepare(context.Background(), root, command, core.RestrictedFiles)
		return command, err
	}
	// observedCommand stops before file preparation, for arguments whose
	// admitted command is the assertion.
	observedCommand := func(op protocol.ToolOperation, args string) (protocol.Command, error) {
		resolved, err := observed.resolve(op, json.RawMessage(args))
		if err != nil {
			return protocol.Command{}, err
		}
		return operations.Command(op, resolved, worktree, session, turn, nil, false)
	}
	faultCode := func(err error) protocol.ErrorCode {
		var fault *protocol.Fault
		if !errors.As(err, &fault) {
			t.Fatalf("expected a fault, got %v", err)
		}
		return fault.Code
	}

	command, err := prepare(protocol.ToolOperationEditFile, `{"path":"a.txt","old_text":"one","new_text":"two"}`)
	if err != nil {
		t.Fatalf("edit after read: %v", err)
	}
	if got := command.Data.(*protocol.CommandEditFilePayload).ExpectedRevision; got != read {
		t.Fatalf("admitted edit carries %q, want the observed %q", got, read)
	}
	if _, err := prepare(protocol.ToolOperationEditFile, `{"path":"a.txt","old_text":"one","new_text":"two","expected_revision":"`+read+`"}`); err != nil {
		t.Fatalf("explicit revision: %v", err)
	}

	write("b.txt", "unread\n")
	for _, op := range []protocol.ToolOperation{protocol.ToolOperationEditFile, protocol.ToolOperationDeleteFile, protocol.ToolOperationMoveFile} {
		if _, err := observed.resolve(op, json.RawMessage(`{"path":"b.txt","old_text":"unread","new_text":"x","destination":"c.txt"}`)); faultCode(err) != protocol.ErrorCodeInvalidRequest {
			t.Fatalf("%s on an unread path: %v", op, err)
		}
	}

	command, err = prepare(protocol.ToolOperationWriteFile, `{"path":"new.txt","text":"fresh\n"}`)
	if err != nil {
		t.Fatalf("write to an unobserved path: %v", err)
	}
	if command.Data.(*protocol.CommandWriteFilePayload).ExpectedRevision != nil {
		t.Fatal("an unobserved write must create")
	}
	if _, err := prepare(protocol.ToolOperationWriteFile, `{"path":"b.txt","text":"replace\n"}`); err == nil {
		t.Fatal("an unobserved write replaced an existing file")
	}
	command, err = observedCommand(protocol.ToolOperationWriteFile, `{"path":"a.txt","text":"replace\n"}`)
	if err != nil || command.Data.(*protocol.CommandWriteFilePayload).ExpectedRevision != nil {
		t.Fatalf("an omitted write revision must create, never replace: %v", err)
	}
	command, err = observedCommand(protocol.ToolOperationEditFile, `{"path":"./a.txt","old_text":"one","new_text":"two"}`)
	if err != nil || command.Data.(*protocol.CommandEditFilePayload).ExpectedRevision != read {
		t.Fatalf("edit of a dot-relative observed path: %v", err)
	}
	if _, err := prepare(protocol.ToolOperationWriteFile, `{"path":"a.txt","text":"replace\n","expected_revision":null}`); err == nil {
		t.Fatal("explicit null replaced an existing file")
	}

	write("a.txt", "changed elsewhere\n")
	for _, stale := range []struct {
		op   protocol.ToolOperation
		args string
	}{
		{protocol.ToolOperationEditFile, `{"path":"a.txt","old_text":"changed","new_text":"x"}`},
		{protocol.ToolOperationDeleteFile, `{"path":"a.txt"}`},
	} {
		if _, err := prepare(stale.op, stale.args); faultCode(err) != protocol.ErrorCodeRevisionConflict {
			t.Fatalf("%s after an external change: %v", stale.op, err)
		}
	}
}

func TestWorkspaceRevisionAliasesFollowReceipts(t *testing.T) {
	root := t.TempDir()
	if err := os.WriteFile(filepath.Join(root, "File.txt"), []byte("source"), 0o600); err != nil {
		t.Fatal(err)
	}
	_, err := os.Stat(filepath.Join(root, "file.txt"))
	if err != nil && !os.IsNotExist(err) {
		t.Fatal(err)
	}
	aliased := err == nil
	tree := protocol.NewWorktreeID()
	observed := workspaceRevisions{worktreeID: tree, worktree: root}
	record := func(id, kind, path, revision string) canonical.Record {
		payload, err := json.Marshal(map[string]any{"kind": kind, "data": map[string]string{"path": path, "revision": revision}})
		if err != nil {
			t.Fatal(err)
		}
		return canonical.Record{ID: id, Kind: "message", Provenance: canonical.Provenance{Author: "tool"}, Data: canonical.NewMessage(canonical.User, canonical.Block{Kind: "tool_response", Data: &canonical.ToolResponse{
			Status: canonical.ToolSuccess, Result: canonical.Payload{Value: payload},
		}})}
	}
	check := func(path, revision string) {
		t.Helper()
		args, _ := json.Marshal(map[string]string{"path": path})
		resolved, err := observed.resolve(protocol.ToolOperationEditFile, args)
		if revision == "" {
			if err == nil {
				t.Fatalf("resolved an unobserved file %q", path)
			}
			return
		}
		var got struct {
			Revision string `json:"expected_revision"`
		}
		if err != nil || json.Unmarshal(resolved, &got) != nil || got.Revision != revision {
			t.Fatalf("resolve %q = %s, %v; want revision %q", path, resolved, err, revision)
		}
	}
	other := filepath.Join(t.TempDir(), "file.txt")
	history := []canonical.Record{record("initial", "file_content", "File.txt", "one"), record("external", "file_content", other, "other")}
	observed.update(history)
	check("File.txt", "one")
	check(other, "other")
	if aliased {
		check("file.txt", "one")
	} else {
		check("file.txt", "")
	}
	history = append(history, record("update", "file_written", filepath.Join(root, "file.txt"), "two"))
	observed.update(history)
	check("file.txt", "two")
	if aliased {
		check("File.txt", "two")
	} else {
		check("File.txt", "one")
	}
	observed.update(history[:2])
	check("File.txt", "one")
	check(other, "other")
}

func TestWorkspaceRevisionsObserveChildFileChanges(t *testing.T) {
	t.Parallel()
	root := t.TempDir()
	tree := protocol.NewWorktreeID()
	otherTree := protocol.NewWorktreeID()
	observed := workspaceRevisions{worktreeID: tree, worktree: root}
	readPayload, _ := json.Marshal(map[string]any{"kind": "file_lines", "data": map[string]string{"path": "a.go", "revision": "parent-new"}})
	waitMetaBytes, _ := json.Marshal(map[string]string{"a.go": "child-old", "b.go": "revB"})
	otherMetaBytes, _ := json.Marshal(map[string]string{"a.go": "other-tree-rev"})
	meta := map[string]json.RawMessage{
		canonical.FileChangesKey(tree):      waitMetaBytes,
		canonical.FileChangesKey(otherTree): otherMetaBytes,
	}
	records := []canonical.Record{
		{
			ID: "r1", Kind: "message", Provenance: canonical.Provenance{Author: "tool"},
			Data: canonical.NewMessage(canonical.User, canonical.Block{Kind: "tool_response", Data: &canonical.ToolResponse{
				Status: canonical.ToolSuccess, Result: canonical.Payload{Value: readPayload},
			}}),
		},
		{
			ID: "r2", Kind: "message", Provenance: canonical.Provenance{Author: "tool"},
			Data: canonical.NewMessage(canonical.User, canonical.Block{Kind: "tool_response", Data: &canonical.ToolResponse{
				Status: canonical.ToolSuccess, Result: canonical.Payload{Value: json.RawMessage(`{"status":"completed"}`)}, Meta: &meta,
			}}),
		},
	}
	observed.update(records)
	if got := observed.paths[observed.pathKey("a.go")]; got != "" {
		t.Fatalf("unordered wait_agent receipt replaced a parent observation: got %q", got)
	}
	if got := observed.paths[observed.pathKey("b.go")]; got != "revB" {
		t.Fatalf("wait_agent file change did not record b.go: got %q, want revB", got)
	}
	records = append(records, canonical.Record{
		ID: "r3", Kind: "background", Provenance: canonical.Provenance{Author: "system"},
		Metadata: canonical.Metadata{Runtime: &canonical.Runtime{
			AgentVisible: true,
			FileChanges: map[string]map[string]string{
				tree.String():      {"a.go": "child-old", "b.go": ""},
				otherTree.String(): {"a.go": "ignored"},
			},
		}},
		Data: &canonical.Background{},
	})
	observed.update(records)
	if got := observed.paths[observed.pathKey("a.go")]; got != "" {
		t.Fatalf("duplicate child receipt revived an invalidated revision: got %q", got)
	}
	if got := observed.paths[observed.pathKey("b.go")]; got != "" {
		t.Fatalf("child deletion left cached revision %q", got)
	}
	if _, err := observed.resolve(protocol.ToolOperationEditFile, json.RawMessage(`{"path":"a.go"}`)); err == nil {
		t.Fatal("ambiguous child receipt supplied an implicit revision")
	}
	read := records[0]
	read.ID = "r4"
	records = append(records, read)
	observed.update(records)
	if got := observed.paths[observed.pathKey("a.go")]; got != "parent-new" {
		t.Fatalf("fresh parent read did not restore the revision: got %q", got)
	}
	duplicate := records[1]
	duplicate.ID = "r5"
	records = append(records, duplicate)
	observed.update(records)
	for _, path := range []string{"a.go", "b.go"} {
		if got := observed.paths[observed.pathKey(path)]; got != "" {
			t.Fatalf("late wait_agent receipt replaced %s with %q", path, got)
		}
	}
	observed.update(records[:1])
	if got := observed.paths[observed.pathKey("a.go")]; got != "parent-new" {
		t.Fatalf("replaying a shorter history did not reset invalidations: got %q", got)
	}
}

func TestWorkspaceRevisionsCaseOnlyMoveWithEdit(t *testing.T) {
	t.Parallel()
	for _, tt := range []struct {
		name string
		key  func(string) string
		want string
	}{
		{"distinct names", func(path string) string { return path }, "child-edit"},
		{"aliased names", strings.ToLower, ""},
	} {
		t.Run(tt.name, func(t *testing.T) {
			// A child's flattened map cannot order a move-away marker and an
			// edit at another spelling of the same file. Only distinct names
			// can keep the destination revision from the unordered receipt.
			observed := workspaceRevisions{paths: map[string]string{tt.key("File.txt"): "parent-read"}}
			changes := map[string]string{"File.txt": "", "file.txt": "child-edit"}
			observed.observeChildChanges(changes, tt.key, false)
			observed.observeChildChanges(changes, tt.key, false)
			if got := observed.paths[tt.key("file.txt")]; got != tt.want {
				t.Fatalf("child move and edit revision = %q, want %q", got, tt.want)
			}
			// Ordered local tool outcomes still leave the revision of the edit.
			observed.observe(&canonical.ToolResponse{Status: canonical.ToolSuccess, Result: canonical.Payload{Value: json.RawMessage(`{"kind":"file_changes","data":{"files":[{"path":"File.txt","destination":"file.txt","kind":"renamed","status":"applied"},{"path":"file.txt","revision":"local-edit","kind":"modified","status":"applied"}]}}`)}}, tt.key)
			if got := observed.paths[tt.key("file.txt")]; got != "local-edit" {
				t.Fatalf("ordered move and edit revision = %q", got)
			}
			observed.observeChildChanges(changes, tt.key, false)
			if got := observed.paths[tt.key("file.txt")]; got != "" {
				t.Fatalf("late child move and edit replaced local edit revision with %q", got)
			}
		})
	}
}

func TestWorkspaceRevisionsInvalidateCombinedChildReceipts(t *testing.T) {
	t.Parallel()
	tree := protocol.NewWorktreeID()
	observed := workspaceRevisions{worktreeID: tree}
	child := canonical.BackgroundResult{Notice: protocol.ConversationBackgroundNotice{Kind: protocol.ConversationBackgroundKind{Kind: "agent"}}}
	changes := &canonical.Runtime{FileChanges: map[string]map[string]string{tree.String(): {"a.go": "child-revision"}}}
	history := []canonical.Record{{ID: "combined", Kind: "background", Metadata: canonical.Metadata{Runtime: changes},
		Data: &canonical.Background{Results: []canonical.BackgroundResult{child, child}}}}
	observed.update(history)
	if got := observed.paths["a.go"]; got != "" {
		t.Fatalf("flattened receipts supplied an unordered revision: %q", got)
	}
	history = append(history, canonical.Record{ID: "late", Kind: "background", Metadata: canonical.Metadata{Runtime: changes},
		Data: &canonical.Background{Results: []canonical.BackgroundResult{child}}})
	observed.update(history)
	if got := observed.paths["a.go"]; got != "" {
		t.Fatalf("late standalone receipt revived an unordered revision: %q", got)
	}
}

func TestWorkspaceRevisionsLateReceiptsAfterDirectoryMove(t *testing.T) {
	t.Parallel()
	observed := workspaceRevisions{paths: map[string]string{"dir/known": "old", "other/file": "current"}}
	observed.invalidateTree("dir")
	receipt := map[string]string{"dir/known": "old", "dir/unseen": "child", "other/file": "current"}
	observed.observeChildChanges(receipt, observed.pathKey, false)
	if len(observed.paths) != 1 || observed.paths["other/file"] != "current" {
		t.Fatalf("late receipts revived a moved path or cleared a matching revision: %v", observed.paths)
	}
	observed.observeRevision("dir/known", "parent-read")
	if got := observed.paths["dir/known"]; got != "parent-read" {
		t.Fatalf("direct read of a recreated path did not restore its revision: %q", got)
	}
	observed.observeChildChanges(receipt, observed.pathKey, false)
	if len(observed.paths) != 1 || observed.paths["other/file"] != "current" {
		t.Fatalf("late receipts replaced a fresh read or cleared a matching revision: %v", observed.paths)
	}
}

func TestPreparedStaleDetectsDiskDrift(t *testing.T) {
	t.Parallel()
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	file := filepath.Join(root, "sample.txt")
	if err := os.WriteFile(file, []byte("one\ntwo\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	patch := "*** Begin Patch\n*** Update File: sample.txt\n@@\n one\n-two\n+three\n*** End Patch"
	prepared, err := files.PreparePatch(t.Context(), root, patch, protocol.NewWorktreeID(), core.RestrictedFiles)
	if err != nil {
		t.Fatal(err)
	}
	if stale, err := files.PreparedStale(t.Context(), root, prepared); err != nil || stale {
		t.Fatalf("fresh prepared patch reported stale=%v err=%v", stale, err)
	}
	if err := os.WriteFile(file, []byte("zero\none\ntwo\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if stale, err := files.PreparedStale(t.Context(), root, prepared); err != nil || !stale {
		t.Fatalf("drifted prepared patch reported stale=%v err=%v", stale, err)
	}
}

func TestWorkspaceLocksWaitFilesDrainsActiveWrite(t *testing.T) {
	t.Parallel()
	root := t.TempDir()
	var locks workspaceLocks
	release := locks.reserve([]string{workspaceFileKey(root, "a.go")}, false)
	waited := make(chan error, 1)
	go func() {
		waited <- locks.waitFiles(t.Context(), root, []string{"a.go"})
	}()
	select {
	case err := <-waited:
		t.Fatalf("waitFiles returned while write lock was held: %v", err)
	default:
	}
	release()
	if err := <-waited; err != nil {
		t.Fatalf("waitFiles after release = %v", err)
	}
}
