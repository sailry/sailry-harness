package live

import (
	"fmt"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"time"

	htmltomarkdown "github.com/JohannesKaufmann/html-to-markdown/v2"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

var workspaceHunkHeader = regexp.MustCompile(`^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@`)

func workspaceHunks(patch string) []viewmodel.Hunk {
	result := []viewmodel.Hunk{}
	old, new := 0, 0
	for _, line := range strings.Split(patch, "\n") {
		if match := workspaceHunkHeader.FindStringSubmatch(line); match != nil {
			old, _ = strconv.Atoi(match[1])
			new, _ = strconv.Atoi(match[2])
			result = append(result, viewmodel.Hunk{Header: line})
			continue
		}
		if len(result) == 0 || line == "" {
			continue
		}
		kind := viewmodel.Context
		entry := viewmodel.DiffLine{Text: line[1:]}
		switch line[0] {
		case '+':
			kind = viewmodel.Added
			entry.New = new
			new++
		case '-':
			kind = viewmodel.Removed
			entry.Old = old
			old++
		case ' ':
			entry.Old, entry.New = old, new
			old++
			new++
		default:
			continue
		}
		entry.Kind = kind
		index := len(result) - 1
		result[index].Lines = append(result[index].Lines, entry)
	}
	return result
}
func (p *workspaceProject) readDiff(relative, scope string) {
	if relative == "" || relative == ":changes" {
		return
	}
	if p.turnView != nil {
		p.readFile(relative)
		p.readTurnHunks(relative)
		return
	}
	key := scope + "/" + relative
	if p.diffs[key] {
		p.projectDiff(relative, p.diffContents[key])
		return
	}
	value := protocol.GitDiffScope(scope)
	if value == "" {
		value = protocol.GitDiffScopeAll
	}
	capturedStatus := p.status
	p.read("diff/"+key, protocol.Command{Kind: "read_git_diff", Data: protocol.CommandReadGitDiffPayload{Worktree: p.owner.ID, Path: relative, Scope: value}}, func(c *ui.Context, out protocol.Output) {
		diff, ok := detail[protocol.GitDiff](out.Data)
		if !ok {
			return
		}
		p.diffs[key] = true
		if p.manualDiffs == nil {
			p.manualDiffs = map[string]workspaceGitDiff{}
		}
		p.manualDiffs[key] = workspaceGitDiff{diff: diff, status: capturedStatus}
		p.panel.SetGitDiffState(relative, scope, panel.GitDiffState{Binary: diff.Binary, Partial: diff.Truncated, Size: diff.Size})
		if p.diffContents == nil {
			p.diffContents = map[string][]viewmodel.Hunk{}
		}
		p.diffContents[key] = workspaceHunks(diff.Text)
		p.projectDiff(relative, p.diffContents[key])
		p.readFile(relative)
		p.readTurnHunks(relative)
	})
}
func (p *workspaceProject) projectDiff(relative string, hunks []viewmodel.Hunk) {
	for i := range p.data.Changes {
		if p.data.Changes[i].Path == relative {
			p.data.Changes[i].Hunks = hunks
		}
	}
}
func (p *workspaceProject) showDocument(id, title, documentPath, text string) {
	for i := range p.data.Documents {
		if p.data.Documents[i].ID == id {
			p.data.Documents[i] = viewmodel.Document{ID: id, Title: title, Path: documentPath, Text: text}
			p.runtime.App.Panel.Show(panel.Tab{Kind: panel.Documents, ID: id})
			return
		}
	}
	p.data.Documents = append(p.data.Documents, viewmodel.Document{ID: id, Title: title, Path: documentPath, Text: text})
	p.runtime.App.PanelOpen = true
	p.runtime.App.Panel.Show(panel.Tab{Kind: panel.Documents, ID: id})
}

// readSessionResources reloads every session resource after a session change
// or a refresh.
func (p *workspaceProject) readSessionResources() {
	if p.session == (protocol.SessionID{}) {
		return
	}
	p.resourceRuns, p.resourceOlder = map[protocol.TurnID]protocol.ConversationStatus{}, false
	p.readArtifacts(p.session, nil, nil)
	if state, ok := p.runtime.controller.Conversation(p.session); ok {
		p.readPageResources(state.Snapshot.Page)
	}
}

// readPageResources reads what the page shows for the first time: each
// finished run once, and the history before the page once per reload.
func (p *workspaceProject) readPageResources(page protocol.ConversationPage) {
	for _, run := range page.Runs {
		p.readRunResources(p.session, run)
	}
	if page.NextBefore != nil && !p.resourceOlder {
		p.resourceOlder = true
		p.readOlderResources(p.session, page.NextBefore)
	}
}

// resourcesLoading reports whether the session's checkpoints or documents
// are being read, so their tab shows a spinner instead of its empty state.
func (p *workspaceProject) resourcesLoading(kind panel.Kind) bool {
	prefixes := []string{"checkpoints/", "older-resources/"}
	if kind == panel.Documents {
		prefixes = []string{"artifacts/"}
	}
	for key := range p.loading {
		for _, prefix := range prefixes {
			if strings.HasPrefix(key, prefix) {
				return true
			}
		}
	}
	return false
}
func (p *workspaceProject) sessionResourcesFrame() {
	if p.session == (protocol.SessionID{}) {
		return
	}
	state, ok := p.runtime.conversationState(p.session)
	if !ok {
		return
	}
	var signature strings.Builder
	for _, run := range state.Snapshot.Page.Runs {
		signature.WriteString(run.Turn.String())
		signature.WriteString(string(run.Status))
	}
	for _, artifact := range state.Snapshot.Artifacts.Items {
		fmt.Fprintf(&signature, "%s/%d", artifact.ID.String(), artifact.Revision)
	}
	value := signature.String()
	if value != p.resourceSignature {
		p.resourceSignature = value
		p.readArtifacts(p.session, nil, nil)
		p.readPageResources(state.Snapshot.Page)
	}
}
func (p *workspaceProject) readOlderResources(session protocol.SessionID, before *protocol.TurnID) {
	if before == nil {
		return
	}
	p.read("older-resources/"+session.String()+"/"+before.String(), protocol.Command{Kind: "read_conversation", Data: protocol.CommandReadConversationPayload{Session: session, Before: before, Limit: 100}}, func(c *ui.Context, out protocol.Output) {
		history, ok := detail[protocol.ConversationHistory](out.Data)
		if !ok || session != p.session {
			return
		}
		page := history.Page
		for _, run := range page.Runs {
			p.readRunResources(session, run)
		}
		if page.NextBefore != nil {
			p.readOlderResources(session, page.NextBefore)
		}
	})
}
func (p *workspaceProject) readRunResources(session protocol.SessionID, run protocol.ConversationRun) {
	if run.Worktree != p.owner.ID {
		return
	}
	if run.Status == protocol.ConversationStatusQueued || run.Status == protocol.ConversationStatusRunning || run.Status == protocol.ConversationStatusStopping {
		return
	}
	if p.resourceRuns[run.Turn] == run.Status {
		return
	}
	if p.resourceRuns == nil {
		p.resourceRuns = map[protocol.TurnID]protocol.ConversationStatus{}
	}
	p.resourceRuns[run.Turn] = run.Status
	title := run.Turn.String()
	if run.Input != nil && strings.TrimSpace(run.Input.Message.Text) != "" {
		title = strings.SplitN(run.Input.Message.Text, "\n", 2)[0]
	}
	started := time.Time{}
	if run.StartedMs != nil {
		started = time.UnixMilli(*run.StartedMs)
	}
	p.readCheckpointPage(session, run.Turn, title, started, nil, nil)
}
func (p *workspaceProject) readArtifacts(session protocol.SessionID, after *protocol.ArtifactID, accumulated []protocol.ArtifactSummary) {
	key := "artifacts/" + session.String()
	if after != nil {
		key += "/" + after.String()
	}
	p.read(key, protocol.Command{Kind: "list_artifacts", Data: protocol.CommandListArtifactsPayload{Session: session, After: after, Limit: 64}}, func(c *ui.Context, out protocol.Output) {
		page, ok := detail[protocol.ArtifactPage](out.Data)
		if !ok || p.session != session {
			return
		}
		accumulated = append(accumulated, page.Items...)
		if page.NextAfter != nil {
			p.readArtifacts(session, page.NextAfter, accumulated)
			return
		}
		documents := []viewmodel.Document{}
		loaded := map[string]viewmodel.Document{}
		for _, old := range p.data.Documents {
			loaded[old.ID] = old
			if _, artifact := p.artifacts[old.ID]; !artifact {
				documents = append(documents, old)
			}
		}
		previous := p.artifacts
		p.artifacts = map[string]protocol.ArtifactSummary{}
		for _, artifact := range accumulated {
			if artifact.Kind != protocol.ArtifactKindDocument {
				continue
			}
			id := artifact.ID.String()
			p.artifacts[id] = artifact
			document := viewmodel.Document{ID: id, Title: artifact.Title, Text: artifact.Summary}
			if old, ok := previous[id]; ok && old.Revision == artifact.Revision {
				if value, exists := loaded[id]; exists {
					document.Text = value.Text
				}
			} else {
				delete(p.documents, id)
			}
			documents = append(documents, document)
		}
		p.data.Documents = documents
	})
}
func (p *workspaceProject) readDocument(id string) {
	artifact, exists := p.artifacts[id]
	if !exists {
		return
	}
	session := p.session
	revision := artifact.Revision
	if cached, ok := p.documents[id]; ok && cached == revision {
		return
	}
	p.documents[id] = revision
	p.read("artifact/"+id, protocol.Command{Kind: "read_artifact", Data: protocol.CommandReadArtifactPayload{ID: artifact.ID, Revision: &revision, Session: &session}}, func(c *ui.Context, out protocol.Output) {
		value, ok := detail[protocol.ArtifactContent](out.Data)
		if !ok {
			return
		}
		p.runtime.storeDocument(id, value)
		p.runtime.async(func() {
			text, err := htmltomarkdown.ConvertString(value.Html)
			p.runtime.enqueue(func(c *ui.Context) {
				if err != nil {
					p.runtime.reportKeys(c, err, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
					return
				}
				if p.session != session {
					return
				}
				for i := range p.data.Documents {
					if p.data.Documents[i].ID == id {
						p.data.Documents[i].Text = text
					}
				}
			})
		})
	})
}
func (p *workspaceProject) readCheckpointPage(session protocol.SessionID, turn protocol.TurnID, title string, started time.Time, before *protocol.CheckpointID, accumulated []protocol.ConversationCheckpointFile) {
	key := "checkpoints/" + turn.String()
	if before != nil {
		key += "/" + before.String()
	}
	p.read(key, protocol.Command{Kind: "list_file_checkpoints", Data: protocol.CommandListFileCheckpointsPayload{Session: session, Turn: turn, Before: before, Limit: 100}}, func(c *ui.Context, out protocol.Output) {
		value, ok := detail[protocol.ConversationCheckpointPage](out.Data)
		if !ok || p.session != session {
			return
		}
		accumulated = append(accumulated, value.Files...)
		if value.Next != nil {
			p.readCheckpointPage(session, turn, title, started, value.Next, accumulated)
			return
		}
		p.checkpointOrder[turn.String()] = append([]protocol.ConversationCheckpointFile(nil), accumulated...)
		files := []string{}
		for _, file := range accumulated {
			if file.Worktree != p.owner.ID {
				continue
			}
			p.checkpoints[file.ID.String()] = file
			files = append(files, file.Path)
		}
		if len(files) == 0 {
			return
		}
		checkpoint := viewmodel.Checkpoint{ID: turn.String(), Turn: title, Time: started, Files: files}
		found := false
		for i := range p.data.Checkpoints {
			if p.data.Checkpoints[i].ID == checkpoint.ID {
				p.data.Checkpoints[i] = checkpoint
				found = true
			}
		}
		if !found {
			p.data.Checkpoints = append(p.data.Checkpoints, checkpoint)
		}
		p.readCheckpointFigures(session, turn)
		sort.SliceStable(p.data.Checkpoints, func(i, j int) bool { return p.data.Checkpoints[i].Time.After(p.data.Checkpoints[j].Time) })
		if relative := p.panel.Git.Change(); relative != "" && relative != ":changes" {
			p.readTurnHunks(relative)
		}
	})
}
func (p *workspaceProject) restore(_ *ui.Context, id string) {
	p.restoreTurn(p.session, id, "", func(bool) {})
}
func (p *workspaceProject) readTurnHunks(relative string) {
	session := p.session
	var turn protocol.TurnID
	found := false
	if selected, exists := p.reviewTurns[relative]; exists {
		turn, found = selected, true
	}
	for _, checkpoint := range p.data.Checkpoints {
		if found {
			break
		}
		for _, file := range p.checkpointOrder[checkpoint.ID] {
			if file.Path == relative && file.Worktree == p.owner.ID {
				turn = file.Turn
				found = true
				break
			}
		}
		if found {
			break
		}
	}
	if !found {
		return
	}
	p.read("hunks/"+relative, protocol.Command{Kind: "read_turn_hunks", Data: protocol.CommandReadTurnHunksPayload{Session: session, Turn: turn, Path: relative}}, func(c *ui.Context, out protocol.Output) {
		if value, ok := detail[protocol.ConversationCheckpointHunkReview](out.Data); ok && p.session == session && value.Session == session && value.Worktree == p.owner.ID {
			p.hunks[relative] = value
		}
	})
}
func (p *workspaceProject) hunk(c *ui.Context, relative string, index int, action string) {
	if action == "keep" {
		return
	}
	review, exists := p.hunks[relative]
	source, loaded := p.files[relative]
	if !loaded {
		p.readFile(relative)
	}
	var displayed *viewmodel.Hunk
	for _, change := range p.data.Changes {
		if change.Path == relative && index >= 0 && index < len(change.Hunks) {
			displayed = &change.Hunks[index]
		}
	}
	matched := -1
	if displayed != nil {
		matched = workspaceHunkMatch(*displayed, review.Hunks)
	}
	if !exists || review.Session != p.session || review.Worktree != p.owner.ID || matched < 0 || !loaded || source.Revision == nil || source.Truncated {
		if p.turnView == nil && displayed != nil && (!exists || review.Session != p.session || review.Worktree != p.owner.ID || matched < 0) {
			p.manualGitHunk(c, relative, *displayed, action)
			return
		}
		p.runtime.reportKeys(c, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "hunk source is not loaded"}, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
		return
	}
	operation := protocol.ConversationCheckpointHunkOperationReject
	if action == "stage" {
		operation = protocol.ConversationCheckpointHunkOperationStage
	}
	selection := protocol.ConversationCheckpointHunkSelection{Session: p.session, Turn: review.Turn, Worktree: p.owner.ID, Path: relative, Hunk: review.Hunks[matched].ID, Operation: operation, ExpectedRevision: *source.Revision}
	if operation == protocol.ConversationCheckpointHunkOperationStage {
		selection.ExpectedIndex = p.status.IndexRevision
		selection.ExpectedHead = p.status.Head
	}
	p.runtime.executeScoped("hunk/"+relative, protocol.Command{Kind: "apply_turn_hunk", Data: selection}, func(c *ui.Context, out protocol.Output, err error) {
		if err != nil {
			p.runtime.reportKeys(c, err, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
			return
		}
		p.refresh()
		kit.Toast(c, kit.Done, i18n.T(c, "workspace_completed"))
	})
}

func workspaceHunkFingerprint(hunk viewmodel.Hunk) string {
	var result strings.Builder
	for _, line := range hunk.Lines {
		if line.Kind == viewmodel.Added || line.Kind == viewmodel.Removed {
			fmt.Fprintf(&result, "%d/%d/%d:%s\n", line.Kind, line.Old, line.New, line.Text)
		}
	}
	return result.String()
}
func workspaceHunkMatch(displayed viewmodel.Hunk, reviews []protocol.ConversationCheckpointHunk) int {
	wanted := workspaceHunkFingerprint(displayed)
	if wanted == "" {
		return -1
	}
	matched := -1
	for index, review := range reviews {
		hunks := workspaceHunks(review.Text)
		if len(hunks) == 1 && workspaceHunkFingerprint(hunks[0]) == wanted {
			if matched >= 0 {
				return -1
			}
			matched = index
		}
	}
	return matched
}
func (p *workspaceProject) readCheckpointFigures(session protocol.SessionID, turn protocol.TurnID) {
	key := "checkpoint-figures/" + session.String() + "/" + turn.String()
	if p.loading[key] {
		return
	}
	p.loading[key] = true
	p.runtime.executeScoped(key, protocol.Command{Kind: "read_turn_diff", Data: protocol.CommandReadTurnDiffPayload{Session: session, Turn: turn}}, func(c *ui.Context, out protocol.Output, err error) {
		delete(p.loading, key)
		if err != nil {
			if faultCode(err) != protocol.ErrorCodeBusy {
				p.runtime.reportKeys(c, err, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
			}
			return
		}
		value, ok := detail[protocol.ConversationCheckpointTurnDiff](out.Data)
		if !ok || p.session != session {
			return
		}
		p.turnChanges[turn.String()] = workspaceTurnFiles(value)
		added, removed := 0, 0
		for _, file := range value.Files {
			for _, hunk := range workspaceHunks(file.Text) {
				for _, line := range hunk.Lines {
					if line.Kind == viewmodel.Added {
						added++
					}
					if line.Kind == viewmodel.Removed {
						removed++
					}
				}
			}
		}
		for i := range p.data.Checkpoints {
			if p.data.Checkpoints[i].ID == turn.String() {
				p.data.Checkpoints[i].Added, p.data.Checkpoints[i].Removed = added, removed
			}
		}
	})
}
