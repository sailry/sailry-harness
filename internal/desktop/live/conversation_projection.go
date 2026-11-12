package live

import (
	"cmp"
	"encoding/json"
	"slices"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Conversation projects canonical and live data without changing Client state.
// Thinking is retained for provider replay but intentionally has no UI row.
func Conversation(state client.ConversationState, locales ...string) ([]conversation.Turn, conversation.Session, composer.Interactions, []composer.Queued) {
	language := ""
	if len(locales) > 0 {
		language = locales[0]
	}
	snapshot := state.Snapshot
	page := snapshot.Page
	session := conversation.Session{Loading: state.Loading, LoadingOlder: state.LoadingOlder, OlderFailed: state.OlderErr != nil}
	if page.NextBefore != nil || len(snapshot.Missing) > 0 {
		session.Older = 20
	}
	// The Client reconnects quietly and reports only an interruption that
	// lasts: an observation without an error is being opened, not recovered.
	// While the Client keeps trying, its fault text is not user copy; a busy
	// host and a fault that ended the observation are told in detail.
	switch faultCode(state.Err) {
	case protocol.ErrorCodeUnavailable:
		if !state.Connected {
			session.Connection = conversation.Reconnecting
		}
	case protocol.ErrorCodeBusy:
		session.Diagnostic = &conversation.Diagnostic{Title: "live_read_busy", Detail: state.Err.Error()}
	default:
		if state.Err != nil {
			if !state.Connected {
				session.Connection = conversation.Offline
			}
			session.Diagnostic = &conversation.Diagnostic{Title: "chat_sync_failed", Detail: state.Err.Error()}
		}
	}
	if statistics := snapshot.Statistics; statistics != nil {
		u := &conversation.Usage{Turns: int(statistics.Turns)}
		if statistics.Usage != nil {
			u.Tokens = int(statistics.Usage.Input + statistics.Usage.Output)
			u.Cached = int(statistics.Usage.CachedInput)
		}
		if statistics.Cost != nil {
			u.Cost = float64(statistics.Cost.UsdMicros) / 1_000_000
		}
		if statistics.Generation != nil && statistics.Generation.ElapsedUs > 0 {
			u.Speed = float64(statistics.Generation.OutputTokens) * 1_000_000 / float64(statistics.Generation.ElapsedUs)
		}
		context, limit := statistics.ContextTokens, statistics.ContextLimit
		if context == nil {
			context = statistics.ContextEstimate
			if statistics.Budget != nil {
				limit = &statistics.Budget.ContextLimit
			}
		}
		if context != nil && limit != nil && *limit > 0 {
			u.Context = float64(*context) / float64(*limit)
		}
		session.Usage = u
	}
	runs := slices.Clone(page.Runs)
	slices.SortStableFunc(runs, func(a, b protocol.ConversationRun) int {
		x, y := a.StartedMs, b.StartedMs
		if x == nil {
			x = a.FinishedMs
		}
		if y == nil {
			y = b.FinishedMs
		}
		if x == nil && y != nil {
			return 1
		}
		if x != nil && y == nil {
			return -1
		}
		if x != nil && y != nil {
			if n := cmp.Compare(*x, *y); n != 0 {
				return n
			}
		}
		return cmp.Compare(a.Sequence, b.Sequence)
	})
	owners := map[protocol.TurnID]protocol.TurnID{}
	pending := map[protocol.TurnID]bool{}
	for _, p := range page.Queue.Items {
		pending[p.Turn] = true
	}
	for _, r := range runs {
		owners[r.Turn] = r.Turn
		if r.Joined != nil && slices.ContainsFunc(runs, func(v protocol.ConversationRun) bool { return v.Turn == *r.Joined }) {
			owners[r.Turn] = *r.Joined
		}
	}
	var turns []conversation.Turn
	positions := map[protocol.TurnID]int{}
	for _, r := range runs {
		if owners[r.Turn] != r.Turn || r.Status == "queued" && pending[r.Turn] {
			continue
		}
		if r.Status == "continued" && r.Continuation != nil && !slices.ContainsFunc(page.Entries, func(e protocol.ConversationEntry) bool { return e.Turn == r.Turn && e.Author != "user" }) {
			continue
		}
		t := conversation.Turn{ID: r.Turn.String(), Reply: conversation.Reply{Status: runStatus(r.Status), Notice: r.Notice}}
		if r.StartedMs != nil {
			t.Sent = time.UnixMilli(*r.StartedMs)
		}
		if r.FinishedMs != nil {
			t.Reply.Finished = time.UnixMilli(*r.FinishedMs)
			if r.StartedMs != nil {
				t.Reply.Elapsed = time.Duration(*r.FinishedMs-*r.StartedMs) * time.Millisecond
			}
		}
		if t.Reply.Status.Active() {
			if r.StartedMs != nil {
				t.Reply.Elapsed = max(0, time.Since(time.UnixMilli(*r.StartedMs)))
			}
			t.Reply.Phase = conversation.AwaitingResponse
			if !state.Connected {
				t.Reply.Status = conversation.Unsynced
			}
		}
		// A turn a package continued shows the package's marker, never its
		// prompt, which is input for the model.
		if r.Continuation != nil {
			t.Marker = continuationMarker(r.Continuation)
		} else if r.Input != nil {
			t.User = r.Input.Message.Text
		}
		for _, notice := range r.Notices {
			t.Notices = append(t.Notices, conversation.Notice{Title: notice.Title, Agent: notice.Kind.Kind == "agent", Outcome: noticeOutcome(notice.Outcome)})
		}
		if r.Kind == "compaction" {
			t.Compaction = &conversation.Compaction{State: conversation.Compacted}
			switch r.Status {
			case "queued":
				t.Compaction.State = conversation.CompactionQueued
			case "running", "stopping":
				t.Compaction.State = conversation.CompactionRunning
			case "failed":
				t.Compaction.State = conversation.CompactionFailed
			case "interrupted":
				t.Compaction.State = conversation.CompactionInterrupted
			case "cancelled":
				t.Compaction.State = conversation.CompactionCancelled
			}
			if r.Error != nil {
				t.Compaction.Cause = r.Error.Message
			}
			t.Compaction.Elapsed = t.Reply.Elapsed
		}
		positions[r.Turn] = len(turns)
		turns = append(turns, t)
	}
	associations := map[string][]toolAssociation{}
	// started is the background task each call started, by the call's row.
	started := map[string]string{}
	// User entries replace the captured pending input once canonical history arrives.
	canonicalUsers := map[protocol.TurnID]bool{}
	summarized := map[int]bool{}
	for _, entry := range page.Entries {
		owner := owners[entry.Turn]
		position, ok := positions[owner]
		if !ok {
			continue
		}
		t := &turns[position]
		if entry.Author == "user" {
			// The continued turn's own input is the package's prompt; guidance
			// joined into it is the user's and shows as usual.
			if t.Marker != nil && entry.Turn == owner {
				continue
			}
			if !canonicalUsers[owner] {
				t.User = ""
				canonicalUsers[owner] = true
			}
			var parts []string
			for _, part := range entry.Parts {
				if part.Kind == "text" {
					if text, ok := detail[string](part.Data); ok {
						parts = append(parts, text)
					}
				}
				switch part.Kind {
				case "image":
					if image, ok := detail[protocol.ConversationImage](part.Data); ok {
						t.Images = append(t.Images, conversation.Image{ID: image.Attachment.ID.String(), Name: image.Attachment.Spec.Name})
					}
				case "attachment":
					if file, ok := detail[protocol.Attachment](part.Data); ok {
						t.Files = append(t.Files, conversation.File{ID: file.ID.String(), Path: file.Spec.Name, Mime: file.Spec.MediaType, Size: int64(file.Spec.Size)})
					}
				}
			}
			if text := strings.Join(parts, ""); text != "" {
				if t.User != "" {
					t.User += "\n\n"
				}
				t.User += text
			}
			if t.Sent.IsZero() {
				t.Sent = time.UnixMilli(entry.TimestampMs)
			}
			continue
		}
		textOffset := 0
		for _, block := range t.Reply.Blocks {
			if block.Kind == conversation.TextBlock {
				textOffset += utf8.RuneCountInString(block.Text)
			}
		}
		var entryText strings.Builder
		for _, part := range entry.Parts {
			if part.Kind == "text" {
				if value, ok := detail[string](part.Data); ok {
					entryText.WriteString(value)
				}
			}
		}
		if entry.SearchSuggestions != nil {
			sources := make([]conversation.Citation, 0, len(entry.Citations))
			for _, citation := range entry.Citations {
				value := conversation.Citation{URI: citation.Uri}
				if citation.Title != nil {
					value.Title = *citation.Title
				}
				sources = append(sources, value)
			}
			// Search is work in the accepted layout, so it precedes the answer.
			t.Reply.Blocks = append(t.Reply.Blocks, conversation.Block{Kind: conversation.SearchBlock, Text: entryText.String(), Sources: sources, Suggestions: searchSuggestions(*entry.SearchSuggestions)})
		}
		projectParts(&t.Reply, entry.Parts, entry.ID, entry.Turn, entry.Branch, associations, started, language)
		if t.Compaction != nil {
			for _, part := range entry.Parts {
				if part.Kind == "compaction" {
					summarized[position] = true
					if text, ok := detail[string](part.Data); ok {
						t.Compaction.Summary += text
					}
				}
			}
		}
		for _, citation := range entry.Citations {
			v := conversation.Citation{URI: citation.Uri, End: textOffset + utf8.RuneCountInString(entryText.String())}
			if citation.Title != nil {
				v.Title = *citation.Title
			}
			if citation.End != nil {
				v.End = textOffset + int(*citation.End)
				if *citation.End == 0 {
					v.End = -(textOffset + 1)
				}
			}
			t.Reply.Citations = append(t.Reply.Citations, v)
		}
	}
	// A completed compaction whose loaded turn holds no summary compacted nothing.
	for _, r := range runs {
		if position, ok := positions[r.Turn]; ok && r.Kind == "compaction" && r.Status == "completed" && !summarized[position] && !slices.Contains(snapshot.Missing, r.Turn) {
			if k := turns[position].Compaction; k != nil {
				k.State = conversation.CompactionUnneeded
			}
		}
	}
	for _, draft := range snapshot.Drafts {
		if position, ok := positions[owners[draft.Turn]]; ok {
			t := &turns[position]
			projectParts(&t.Reply, draft.Parts, draft.ID, draft.Turn, draft.Branch, associations, started, language)
			if phase := draftPhase(draft.Parts); phase != conversation.PhaseNone {
				t.Reply.Phase = phase
			}
		}
	}
	for _, r := range runs {
		if position, ok := positions[owners[r.Turn]]; ok && r.Error != nil {
			t := &turns[position]
			// The recovery that stopped says why on its last retry row.
			if reason, stopped := recoveryStop(r.Error); stopped {
				for index := len(t.Reply.Blocks) - 1; index >= 0; index-- {
					if retry := t.Reply.Blocks[index].Retry; retry != nil {
						retry.Stopped, retry.Reason = true, stopKey(reason)
						retry.Exhausted = reason == protocol.ConversationRecoveryStopAttemptsExhausted
						break
					}
				}
			}
			title := failureKey(*r.Error)
			// A compaction that failed has said so, with this cause, on its own block.
			if !slices.ContainsFunc(t.Reply.Blocks, func(block conversation.Block) bool {
				return block.Kind == conversation.ErrorBlock && block.Label == "chat_compact_failed" && block.Text == r.Error.Message
			}) {
				t.Reply.Blocks = append(t.Reply.Blocks, conversation.Block{Kind: conversation.ErrorBlock, Label: title, Text: r.Error.Message})
			}
			if position == len(turns)-1 && session.Diagnostic == nil {
				session.Diagnostic = &conversation.Diagnostic{Title: title, Fields: [][2]string{{"code", string(r.Error.Code)}}, Detail: r.Error.Message}
			}
		}
	}
	calls := map[string]*conversation.Call{}
	for ti := range turns {
		firstProgress := -1
		blocks := turns[ti].Reply.Blocks[:0]
		for _, block := range turns[ti].Reply.Blocks {
			if block.Kind == conversation.ProgressBlock {
				if firstProgress >= 0 {
					blocks[firstProgress].Steps = block.Steps
					continue
				}
				firstProgress = len(blocks)
			}
			blocks = append(blocks, block)
		}
		turns[ti].Reply.Blocks = blocks
		for bi := range turns[ti].Reply.Blocks {
			for ci := range turns[ti].Reply.Blocks[bi].Calls {
				call := &turns[ti].Reply.Blocks[bi].Calls[ci]
				if call.ID != "" {
					calls[call.ID] = call
				}
			}
		}
	}
	// A command sent to the background says where it is on the row of the
	// call that started it, so a result no turn has taken is still in the
	// transcript. A task the Node no longer lists leaves the row as it was.
	for id, task := range started {
		index := slices.IndexFunc(page.Background, func(t protocol.ConversationBackgroundTask) bool { return t.ID == task })
		if call := calls[id]; call != nil && index >= 0 {
			call.Background = projectBackground(page.Background[index])
		}
	}
	var interactions composer.Interactions
	for _, a := range page.Approvals {
		if projected := calls[toolCallKey(a.Entry, a.Index)]; projected != nil {
			switch a.State {
			case "pending":
				projected.Pending = true
				projected.State = conversation.Pending
				projected.Running = false
			case "denied":
				projected.Failed = false
				projected.Warning = true
				projected.Pending = false
				projected.Running = false
				projected.State = conversation.Denied
			case "interrupted":
				projected.Warning = true
				projected.Running = false
				projected.State = conversation.StateInterrupted
			case "cancelled":
				// Stop closed the request: the call reads as a plain row of a
				// stopped turn, not as one that failed or lost its approval.
				projected.Warning = false
				projected.Pending = false
				projected.Running = false
				projected.State = conversation.StateNone
			}
		}
		if a.State != "pending" {
			continue
		}
		call := callAt(page, a.Entry, a.Index)
		v := projectApproval(a, call, language)
		if call != nil {
			if projected := calls[toolCallKey(a.Entry, a.Index)]; projected != nil {
				projected.Pending = true
				projected.State = conversation.Pending
			}
		}
		interactions.Approvals = append(interactions.Approvals, v)
	}
	for _, q := range page.Questions {
		call := callAt(page, q.Entry, q.Index)
		if call == nil || call.Summary == nil || call.Summary.Question == nil || *call.Summary.Question == nil {
			continue
		}
		spec := **call.Summary.Question
		if position, ok := positions[owners[q.Turn]]; ok {
			reply := &turns[position].Reply
			history := projectQuestion(q, spec, calls[toolCallKey(q.Entry, q.Index)], reply.Status, language)
			replaced := false
			for i, block := range reply.Blocks {
				if len(block.Calls) == 1 && block.Calls[0].ID == history.CallID {
					reply.Blocks[i] = conversation.Block{Kind: conversation.QuestionBlock, Question: &history}
					replaced = true
					break
				}
			}
			if !replaced {
				reply.Blocks = append(reply.Blocks, conversation.Block{Kind: conversation.QuestionBlock, Question: &history})
			}
		}
		if q.State.Kind != "pending" || interactions.Question != nil {
			continue
		}
		interactions.Question = projectAsk(q, spec)
	}
	ownedChildren := map[protocol.TurnID][]protocol.ConversationChild{}
	for _, child := range page.Children {
		owner := owners[child.Origin.Turn]
		ownedChildren[owner] = append(ownedChildren[owner], child)
	}
	children := map[protocol.TurnID][]conversation.Child{}
	for owner, owned := range ownedChildren {
		for _, child := range client.LatestConversationChildren(owned) {
			status := conversation.ChildCompleted
			switch child.Run.Status {
			case "queued":
				status = conversation.ChildQueued
			case "running":
				status = conversation.ChildRunning
			case "stopping":
				status = conversation.ChildStopping
			case "continued":
				status = conversation.ChildContinued
			case "failed":
				status = conversation.ChildFailed
			case "interrupted":
				status = conversation.ChildInterrupted
			case "cancelled":
				status = conversation.ChildCancelled
			}
			title := childTitle(&page, child, language)
			children[owner] = append(children[owner], conversation.Child{ID: child.Run.Session.String(), Title: title, Status: status})
		}
	}
	for owner, list := range children {
		if position, ok := positions[owner]; ok {
			reply := &turns[position].Reply
			reply.Blocks = append(reply.Blocks, conversation.Block{Kind: conversation.ChildrenBlock, Children: list})
			reply.Waiting = waitingConversationChildren(snapshot, owner)
			if reply.Waiting > 0 {
				reply.Phase = conversation.WaitingChildren
			}
		}
	}
	var queue []composer.Queued
	for _, item := range page.Queue.Items {
		q := composer.Queued{ID: item.Turn.String(), Text: item.Preview, Revision: item.Revision, Notice: item.Notice, Compaction: item.Kind == "compaction", Guided: item.Guidance != nil}
		// A queued continuation is an automatic row named by its marker: its
		// prompt is not a message to read, edit or send ahead.
		if index := slices.IndexFunc(page.Runs, func(r protocol.ConversationRun) bool { return r.Turn == item.Turn && r.Continuation != nil }); index >= 0 {
			marker := continuationMarker(page.Runs[index].Continuation)
			q.Text, q.Notice, q.Marker = "", true, &composer.Text{Label: marker.Label, Locales: marker.Locales}
		}
		for _, file := range item.Attachments {
			q.Attachments = append(q.Attachments, &composer.Attachment{ID: file.ID.String(), Name: file.Spec.Name, Mime: file.Spec.MediaType, Size: int64(file.Spec.Size), Status: composer.Ready})
		}
		queue = append(queue, q)
	}
	return turns, session, interactions, queue
}

// continuationMarker is the label a package declared for its continuation;
// without one the transcript names the turn generically.
func continuationMarker(continuation *protocol.ConversationContinuation) *conversation.Marker {
	if continuation.Marker == nil {
		return &conversation.Marker{}
	}
	return &conversation.Marker{Label: continuation.Marker.Label, Locales: continuation.Marker.Locales}
}

func waitingConversationChildren(snapshot protocol.ConversationSnapshot, turn protocol.TurnID) int {
	page := snapshot.Page
	runIndex := slices.IndexFunc(page.Runs, func(run protocol.ConversationRun) bool { return run.Turn == turn })
	if runIndex < 0 || page.Runs[runIndex].Status != "running" || page.Runs[runIndex].Kind != "task" {
		return 0
	}
	for _, draft := range snapshot.Drafts {
		if draft.Turn == turn && draft.Author != "user" {
			return 0
		}
	}
	for _, approval := range page.Approvals {
		if approval.Turn == turn && approval.State == "pending" {
			return 0
		}
	}
	for _, question := range page.Questions {
		if question.Turn == turn && question.State.Kind == "pending" {
			return 0
		}
	}
	for _, item := range page.Queue.Items {
		if item.Guidance != nil && *item.Guidance == turn {
			return 0
		}
	}
	// Guidance is pending for the turn that takes it: the turn it joined,
	// which for a batch that outlived its recipient is the successor, not the
	// recipient it was sent to; or, not joined yet, the turn it waits for.
	for _, run := range page.Runs {
		if run.Joined != nil {
			if *run.Joined == turn && !slices.ContainsFunc(page.Entries, func(entry protocol.ConversationEntry) bool {
				return entry.Turn == run.Turn && entry.Author == "user"
			}) {
				return 0
			}
			continue
		}
		if run.Status == "queued" && run.Guidance != nil && *run.Guidance == turn {
			return 0
		}
	}
	pending := map[string][]toolAssociation{}
	settled := map[string]bool{}
	for _, entry := range page.Entries {
		if entry.Turn != turn || entry.Author == "user" {
			continue
		}
		for index, part := range entry.Parts {
			switch part.Kind {
			case "tool_call":
				if call, ok := detail[protocol.ConversationPartToolCallPayload](part.Data); ok && call.ID != nil {
					key := toolPendingKey(turn, entry.Branch, *call.ID)
					pending[key] = append(pending[key], toolAssociation{ID: toolCallKey(entry.ID, uint64(index)), Name: call.Name})
				}
			case "tool_result":
				if result, ok := detail[protocol.ConversationPartToolResultPayload](part.Data); ok && result.ID != nil {
					key := toolPendingKey(turn, entry.Branch, *result.ID)
					matches := pending[key]
					delete(pending, key)
					if len(matches) == 1 && matches[0].Name == result.Name {
						settled[matches[0].ID] = true
					}
				}
			}
		}
	}
	modelOutput := false
scan:
	for entryIndex := len(page.Entries) - 1; entryIndex >= 0; entryIndex-- {
		entry := page.Entries[entryIndex]
		if entry.Turn != turn || entry.Author == "user" {
			continue
		}
		for partIndex := len(entry.Parts) - 1; partIndex >= 0; partIndex-- {
			switch entry.Parts[partIndex].Kind {
			case "tool_result":
				continue
			case "tool_call":
				if !settled[toolCallKey(entry.ID, uint64(partIndex))] {
					return 0
				}
				modelOutput = true
			case "text", "thinking", "image":
				modelOutput = true
				break scan
			default:
				return 0
			}
		}
	}
	if !modelOutput {
		return 0
	}
	owned := slices.DeleteFunc(slices.Clone(page.Children), func(child protocol.ConversationChild) bool {
		return child.Origin.Session != page.Session || child.Origin.Turn != turn
	})
	count := 0
	for _, child := range client.LatestConversationChildren(owned) {
		if child.Run.Status != "queued" && child.Run.Status != "running" && child.Run.Status != "stopping" {
			continue
		}
		call := callAt(page, child.Origin.Entry, child.Origin.Index)
		if call != nil && (call.Summary == nil || !call.Summary.Delegation.Background) {
			count++
		}
	}
	return count
}

// projectBackground is a background command's state for the row of the call
// that started it: running, or how it ended, the exit code of one that
// failed, and whether its result still waits for a turn to take it.
func projectBackground(task protocol.ConversationBackgroundTask) *conversation.Background {
	background := &conversation.Background{Running: task.State == protocol.ConversationBackgroundStateRunning, Waiting: task.State == protocol.ConversationBackgroundStatePending}
	if task.Outcome != nil {
		background.Outcome = noticeOutcome(*task.Outcome)
	}
	if task.ExitCode != nil && *task.ExitCode != 0 {
		code := int(*task.ExitCode)
		background.Exit = &code
	}
	return background
}

func noticeOutcome(outcome protocol.ConversationBackgroundOutcome) conversation.NoticeOutcome {
	switch outcome {
	case "failed":
		return conversation.OutcomeFailed
	case "cancelled":
		return conversation.OutcomeCancelled
	case "stopped":
		return conversation.OutcomeStopped
	case "timed_out":
		return conversation.OutcomeTimedOut
	case "interrupted":
		return conversation.OutcomeInterrupted
	case "unknown":
		return conversation.OutcomeUnknown
	case "stalled":
		return conversation.OutcomeStalled
	default:
		return conversation.OutcomeCompleted
	}
}

func runStatus(status protocol.ConversationStatus) conversation.RunStatus {
	switch status {
	case "queued":
		return conversation.Queued
	case "running":
		return conversation.Running
	case "stopping":
		return conversation.Stopping
	case "continued":
		return conversation.Continued
	case "cancelled":
		return conversation.Cancelled
	case "interrupted":
		return conversation.Interrupted
	case "failed":
		return conversation.Failed
	}
	return conversation.Completed
}

func callAt(page protocol.ConversationPage, entry string, index uint64) *protocol.ConversationPartToolCallPayload {
	for _, e := range page.Entries {
		if e.ID == entry && index < uint64(len(e.Parts)) && e.Parts[index].Kind == "tool_call" {
			if v, ok := detail[protocol.ConversationPartToolCallPayload](e.Parts[index].Data); ok {
				return &v
			}
		}
	}
	return nil
}

type toolAssociation struct {
	ID, Name string
	Hidden   bool
	Progress bool
	Block    int
}

func toolCallKey(entry string, index uint64) string {
	return entry + "/" + strconv.FormatUint(index, 10)
}
func toolPendingKey(turn protocol.TurnID, branch, id string) string {
	return turn.String() + "\x00" + branch + "\x00" + id
}
func projectedCall(reply *conversation.Reply, id string) *conversation.Call {
	for bi := range reply.Blocks {
		for ci := range reply.Blocks[bi].Calls {
			if reply.Blocks[bi].Calls[ci].ID == id {
				return &reply.Blocks[bi].Calls[ci]
			}
		}
	}
	return nil
}

func projectParts(reply *conversation.Reply, parts []protocol.ConversationPart, entry string, turn protocol.TurnID, branch string, pending map[string][]toolAssociation, started map[string]string, language string) {
	for index, part := range parts {
		switch part.Kind {
		case "thinking":
			continue
		case "text":
			if text, ok := detail[string](part.Data); ok && text != "" {
				if n := len(reply.Blocks); n > 0 && reply.Blocks[n-1].Kind == conversation.TextBlock {
					reply.Blocks[n-1].Text += text
				} else {
					reply.Blocks = append(reply.Blocks, conversation.Block{Kind: conversation.TextBlock, Text: text})
				}
			}
		case "compaction":
			if text, ok := detail[string](part.Data); ok {
				reply.Blocks = append(reply.Blocks, conversation.Block{Kind: conversation.CompactionBlock, Label: toolCallKey(entry, uint64(index)), Text: text})
			}
		case "image":
			if image, ok := detail[protocol.ConversationImage](part.Data); ok {
				reply.Blocks = append(reply.Blocks, conversation.Block{Kind: conversation.ImagesBlock, Images: []conversation.Image{{ID: image.Attachment.ID.String(), Name: image.Attachment.Spec.Name}}})
			}
		case "attachment":
			if a, ok := detail[protocol.Attachment](part.Data); ok {
				reply.Blocks = append(reply.Blocks, conversation.Block{Kind: conversation.FilesBlock, Files: []conversation.File{{ID: a.ID.String(), Path: a.Spec.Name, Mime: a.Spec.MediaType, Size: int64(a.Spec.Size)}}})
			}
		case "tool_call":
			if source, ok := detail[protocol.ConversationPartToolCallPayload](part.Data); ok {
				id := toolCallKey(entry, uint64(index))
				progress := source.Summary != nil && source.Summary.Progress != nil
				if source.ID != nil {
					key := toolPendingKey(turn, branch, *source.ID)
					pending[key] = append(pending[key], toolAssociation{ID: id, Name: source.Name, Hidden: source.Presentation == "hidden", Progress: progress, Block: len(reply.Blocks)})
				}
				if source.Presentation == "hidden" {
					continue
				}
				if progress {
					reply.Blocks = append(reply.Blocks, conversation.Block{Kind: conversation.ProgressBlock, Steps: projectSteps(*source.Summary.Progress)})
					continue
				}
				call := projectCall(source.Name, source.Summary, language)
				call.ID = id
				call.Standalone = source.Grouping == "standalone"
				call.Terminal = terminalInteraction(source.Name)
				if !reply.Status.Active() {
					call.State = conversation.NotExecuted
					call.Warning = true
				}
				reply.Blocks = append(reply.Blocks, conversation.Block{Kind: conversation.ToolsBlock, Calls: []conversation.Call{call}})
			}
		case "tool_result":
			if result, ok := detail[protocol.ConversationPartToolResultPayload](part.Data); ok {
				var call *conversation.Call
				if result.ID != nil {
					key := toolPendingKey(turn, branch, *result.ID)
					matches := pending[key]
					delete(pending, key)
					if len(matches) == 1 && matches[0].Name == result.Name {
						if matches[0].Hidden {
							continue
						}
						if matches[0].Progress {
							if result.Summary != nil && result.Summary.Progress != nil {
								reply.Blocks[matches[0].Block].Steps = projectSteps(*result.Summary.Progress)
							}
							continue
						}
						call = projectedCall(reply, matches[0].ID)
					}
				}
				if call == nil {
					if result.Summary != nil && result.Summary.Progress != nil {
						reply.Blocks = append(reply.Blocks, conversation.Block{Kind: conversation.ProgressBlock, Steps: projectSteps(*result.Summary.Progress)})
						continue
					}
					value := projectCall(result.Name, result.Summary, language)
					value.ID = toolCallKey(entry, uint64(index))
					reply.Blocks = append(reply.Blocks, conversation.Block{Kind: conversation.ToolsBlock, Calls: []conversation.Call{value}})
					call = &reply.Blocks[len(reply.Blocks)-1].Calls[0]
				}
				projected := projectCall(result.Name, result.Summary, language)
				projected.ID = call.ID
				projected.Standalone, projected.Terminal = call.Standalone, terminalInteraction(result.Name)
				*call = projected
				call.Running = false
				if result.Summary != nil && result.Summary.Task != nil {
					started[call.ID] = *result.Summary.Task
				}
				// The pictures a tool returned show under its output.
				for _, image := range result.Images {
					if call.Output == nil {
						call.Output = &conversation.Output{}
					}
					picture := conversation.Image{ID: image.Attachment.ID.String(), Name: image.Attachment.Spec.Name}
					if result.ID != nil {
						picture.Reference = *result.ID + "/" + strconv.FormatUint(image.Index, 10)
					}
					call.Output.Images = append(call.Output.Images, picture)
				}
			}
		case "resource":
			if raw, ok := detail[json.RawMessage](part.Data); ok {
				var started struct{ Type, ID, Name string }
				if json.Unmarshal(raw, &started) == nil && started.Type == "tool_started" {
					matches := pending[toolPendingKey(turn, branch, started.ID)]
					if len(matches) == 1 && matches[0].Name == started.Name {
						if call := projectedCall(reply, matches[0].ID); call != nil {
							call.Running = reply.Status.Active()
							call.State = conversation.StateNone
							call.Warning = false
						}
					}
					continue
				}
			}
			projectResource(reply, part.Data)
		}
	}
}

func draftPhase(parts []protocol.ConversationPart) conversation.Phase {
	phase := conversation.PhaseNone
	for _, part := range parts {
		switch part.Kind {
		case "text":
			if text, ok := detail[string](part.Data); ok && text != "" && phase != conversation.Compacting {
				phase = conversation.Generating
			}
		case "thinking":
			if text, ok := detail[string](part.Data); ok && text != "" && phase == conversation.PhaseNone {
				phase = conversation.Thinking
			}
		case "resource":
			if raw, ok := detail[json.RawMessage](part.Data); ok {
				var value struct{ Type string }
				if json.Unmarshal(raw, &value) == nil && value.Type == "compacting" {
					phase = conversation.Compacting
				}
			}
		}
	}
	return phase
}

func terminalInteraction(name string) bool {
	return name == "ask_user" || name == "mcp_elicitation" || name == "plan_review"
}

// projectAsk is the pending question as the composer asks it.
func projectAsk(q protocol.ConversationQuestion, spec protocol.ConversationQuestionSpec) *composer.Ask {
	ask := &composer.Ask{ID: q.ID.String(), Prompt: spec.Prompt, Other: spec.Input.Kind == "text", Declinable: spec.Input.Kind == "form" || spec.Input.Kind == "url"}
	switch spec.Input.Kind {
	case "choice":
		if choice, ok := detail[protocol.ConversationQuestionInputChoicePayload](spec.Input.Data); ok {
			ask.Options = choice.Options
			ask.Multi = choice.Multiple
			ask.Other = choice.AllowOther
		}
	case "plan":
		ask.Kind = composer.AskPlan
	case "url":
		ask.Kind = composer.AskLink
		if link, ok := detail[protocol.ConversationQuestionInputUrlPayload](spec.Input.Data); ok {
			ask.URL = link.Url
		}
	case "form":
		ask.Kind = composer.AskForm
		form, _ := detail[protocol.ConversationQuestionInputFormPayload](spec.Input.Data)
		for _, field := range form.Fields {
			view := composer.Field{Name: field.Name, Title: field.Title, Required: field.Required}
			if field.Description != nil {
				view.Description = *field.Description
			}
			switch field.Input.Kind {
			case "text":
				if text, ok := detail[protocol.ConversationQuestionFormInputTextPayload](field.Input.Data); ok {
					view.Multiline, view.MinLength, view.MaxLength = text.Format == nil, text.MinLength, text.MaxLength
				}
			case "number":
				view.Kind = composer.FieldNumber
				if number, ok := detail[protocol.ConversationQuestionFormInputNumberPayload](field.Input.Data); ok {
					view.Integer = number.Integer
					for bound, target := range map[*json.Number]**float64{number.Minimum: &view.Minimum, number.Maximum: &view.Maximum} {
						if bound != nil {
							if value, err := bound.Float64(); err == nil {
								*target = &value
							}
						}
					}
				}
			case "boolean":
				view.Kind = composer.FieldBoolean
			case "choice":
				view.Kind = composer.FieldChoice
				if choice, ok := detail[protocol.ConversationQuestionFormInputChoicePayload](field.Input.Data); ok {
					view.Multi, view.MinItems, view.MaxItems = choice.Multiple, choice.MinItems, choice.MaxItems
					for _, option := range choice.Options {
						view.Options = append(view.Options, option.Title)
					}
				}
			}
			ask.Fields = append(ask.Fields, view)
		}
	}
	return ask
}

// formDisplay is a form value as its history line shows it.
func formDisplay(field protocol.ConversationQuestionFormField, raw json.RawMessage, locale string) string {
	title := func(value string) string {
		if choice, ok := detail[protocol.ConversationQuestionFormInputChoicePayload](field.Input.Data); ok {
			for _, option := range choice.Options {
				if option.Value == value {
					return option.Title
				}
			}
		}
		return value
	}
	var flag bool
	var text string
	var list []string
	switch {
	case field.Input.Kind == "boolean" && json.Unmarshal(raw, &flag) == nil:
		if flag {
			return i18n.In(locale, "question_yes")
		}
		return i18n.In(locale, "question_no")
	case json.Unmarshal(raw, &text) == nil:
		return title(text)
	case field.Input.Kind == "choice" && json.Unmarshal(raw, &list) == nil:
		for i := range list {
			list[i] = title(list[i])
		}
		return strings.Join(list, ", ")
	}
	return string(raw)
}

// projectQuestion is the question's history row. A plan review is titled as
// the plan and holds the plan; a form answer lists its fields.
func projectQuestion(q protocol.ConversationQuestion, spec protocol.ConversationQuestionSpec, call *conversation.Call, status conversation.RunStatus, locale string) conversation.Question {
	v := conversation.Question{CallID: toolCallKey(q.Entry, q.Index), Prompt: spec.Prompt, State: conversation.QuestionPending}
	if choice, ok := detail[protocol.ConversationQuestionInputChoicePayload](spec.Input.Data); spec.Input.Kind == "choice" && ok {
		v.Options = choice.Options
		v.Multi = choice.Multiple
	}
	switch q.State.Kind {
	case "answered":
		v.State = conversation.QuestionAnswered
		if call != nil && call.State == conversation.NotExecuted {
			v.State = conversation.QuestionUnconsumed
		} else if status.Active() {
			v.State = conversation.QuestionAccepted
		}
		if a, ok := detail[protocol.ConversationQuestionAnswer](q.State.Data); ok {
			switch a.Kind {
			case "text":
				if text, ok := detail[string](a.Data); ok {
					v.Answers = []string{text}
				}
			case "form":
				values, _ := detail[map[string]json.RawMessage](a.Data)
				form, _ := detail[protocol.ConversationQuestionInputFormPayload](spec.Input.Data)
				for _, field := range form.Fields {
					if raw, ok := values[field.Name]; ok {
						v.Answers = append(v.Answers, field.Title+": "+formDisplay(field, raw, locale))
					}
				}
			case "choices":
				if choices, ok := detail[protocol.ConversationQuestionAnswerChoicesPayload](a.Data); ok {
					for _, i := range choices.Selected {
						if i < uint64(len(v.Options)) {
							v.Answers = append(v.Answers, v.Options[i])
						}
					}
					if choices.Other != nil {
						v.Answers = append(v.Answers, *choices.Other)
					}
				}
			}
		}
	case "cancelled":
		v.State = conversation.QuestionCancelled
	case "declined":
		v.State = conversation.QuestionDeclined
	case "interrupted":
		v.State = conversation.QuestionInterrupted
	case "timed_out":
		v.State = conversation.QuestionTimedOut
	}
	if spec.Input.Kind == "plan" {
		v.Prompt, v.Answers = i18n.In(locale, "composer_mode_plan"), nil
		if spec.Prompt != "" {
			v.Answers = []string{spec.Prompt}
		}
	}
	return v
}

func projectCall(name string, summary *protocol.ToolDetailSummary, language string) conversation.Call {
	call := conversation.Call{Name: name, Label: name}
	// A built-in tool has no declaration: its label and icon are the
	// application's until a captured title or resolved label replaces them.
	if key, icon := coreTool(name); key != "" {
		call.Label, call.LabelKey, call.Icon = "", key, icon
	}
	if summary == nil {
		return call
	}
	if summary.Title != nil {
		call.Label = *summary.Title
	}
	if summary.Path != nil {
		call.Detail = *summary.Path
	}
	call.Error = toolFailure(language, summary)
	if summary.Failure != nil {
		call.Failed = summary.Failure.Origin == "runtime"
		call.Warning = summary.Failure.Origin == "tool"
		if code := summary.Failure.Code; code != nil {
			switch *code {
			case protocol.ErrorCodeCancelled:
				call.Failed = false
				call.Warning = false
			case protocol.ErrorCodeOutcomeUnknown:
				call.Failed = false
				call.Warning = true
				call.State = conversation.StateInterrupted
			case protocol.ErrorCodeNotFound:
				if summary.Expected != nil {
					call.Failed = false
					call.Warning = false
				}
			}
		}
		if call.Failed {
			call.State = conversation.StateFailed
		}
	}
	if summary.Icon != nil && summary.Icon.Kind == "Name" {
		if name, ok := detail[string](summary.Icon.Data); ok {
			call.Icon = name
		}
	}
	if resolved := summary.Resolved; resolved != nil {
		if resolved.Label != nil {
			call.Label = resolved.Label.Label
		}
		if resolved.Input.Summary != nil {
			call.Detail = resolved.Input.Summary.Text
			call.Code = resolved.Input.Summary.Code
		}
		if output := resolved.Output; output != nil {
			call.Output = &conversation.Output{Text: output.Preview, Paths: slices.Clone(output.Paths), Command: resolved.Command != nil}
			if output.Body != nil {
				call.Output.Text = output.Body.Text
			}
		}
	}
	if call.Error != "" {
		if call.Output == nil {
			call.Output = &conversation.Output{}
		}
		call.Output.Failure = call.Error
		call.Output.ToolFailure = call.Warning
	}
	return call
}

func projectResource(reply *conversation.Reply, data any) {
	var raw json.RawMessage
	if v, ok := detail[json.RawMessage](data); ok {
		raw = v
	} else {
		return
	}
	var resource struct {
		Kind string          `json:"kind"`
		Type string          `json:"type"`
		Data json.RawMessage `json:"data"`
	}
	if json.Unmarshal(raw, &resource) != nil {
		return
	}
	kind := resource.Kind
	if kind == "" {
		kind = resource.Type
	}
	switch kind {
	case "model_retry":
		var r protocol.ConversationRecoveryRetry
		if json.Unmarshal(raw, &r) == nil {
			v := &conversation.Retry{Attempt: int(r.Attempt)}
			if r.Limit != nil {
				v.Limit = int(*r.Limit)
			}
			// A fallback's retry shows no provider message.
			if r.Error != nil && !r.Fallback && strings.TrimSpace(*r.Error) != "" {
				v.Detail = *r.Error
			}
			reply.Blocks = append(reply.Blocks, conversation.Block{Kind: conversation.RetryBlock, Retry: v})
		}
	case "compacting":
		reply.Phase = conversation.Compacting
		var started struct {
			Started int64 `json:"started_ms"`
		}
		if json.Unmarshal(raw, &started) == nil && started.Started > 0 {
			reply.PhaseElapsed = max(0, time.Since(time.UnixMilli(started.Started)))
		}
	case "output_limit":
		reply.Blocks = append(reply.Blocks, conversation.Block{Kind: conversation.LimitBlock})
	case "compaction_failure":
		var v struct {
			Failure protocol.ConversationCompactionFailure `json:"failure"`
		}
		if json.Unmarshal(raw, &v) == nil {
			message := ""
			if v.Failure.Cause != nil {
				message = *v.Failure.Cause
			}
			reply.Blocks = append(reply.Blocks, conversation.Block{Kind: conversation.ErrorBlock, Label: "chat_compact_failed", Text: message})
		}
	}
}

func projectSteps(p protocol.ConversationProgress) []conversation.Step {
	var out []conversation.Step
	for _, s := range p.Steps {
		state := conversation.StepPending
		switch s.State {
		case "in_progress":
			state = conversation.StepInProgress
		case "completed":
			state = conversation.StepCompleted
		case "skipped":
			state = conversation.StepSkipped
		}
		out = append(out, conversation.Step{Text: s.Description, State: state})
	}
	return out
}
