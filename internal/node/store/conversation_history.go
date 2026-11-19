package store

import (
	"cmp"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const visibleCanonical = `CASE WHEN json_extract(e.body,'$.kind')='compaction' THEN 1 ELSE coalesce((SELECT json_extract(v.body,'$.user_visible') FROM agent_events v WHERE json_extract(v.body,'$.message')=json_extract(e.body,'$.id') AND json_extract(v.body,'$.kind')='operation' AND json_extract(v.body,'$.operation')='visibility' AND EXISTS(SELECT 1 FROM conversation_events visibility WHERE visibility.session=h.session AND visibility.sequence=v.sequence) ORDER BY v.sequence DESC LIMIT 1),json_extract(e.body,'$.metadata.runtime.user_visible'),1) END`

func historyRevision(ctx context.Context, db *sql.Conn, session protocol.SessionID) (uint64, error) {
	var revision uint64
	err := db.QueryRowContext(ctx, "SELECT history_revision FROM sessions WHERE id=?1 AND state!='removed'", session.String()).Scan(&revision)
	if errors.Is(err, sql.ErrNoRows) {
		return 0, fault(protocol.ErrorCodeNotFound, "session does not exist")
	}
	return revision, err
}
func hydratePublicRun(ctx context.Context, db *sql.Conn, run protocol.ConversationRun) (protocol.ConversationRun, error) {
	run.Input = nil
	if run.Kind != protocol.ConversationRunKindTask || run.Continuation != nil || run.Notice {
		return run, nil
	}
	var body []byte
	var canonicalInput bool
	err := db.QueryRowContext(ctx, "SELECT input,EXISTS(SELECT 1 FROM agent_events WHERE turn=?1 AND json_extract(body,'$.provenance.author')='user') FROM agent_runs WHERE turn=?1", run.Turn.String()).Scan(&body, &canonicalInput)
	if err != nil {
		return run, err
	}
	if canonicalInput {
		return run, nil
	}
	if len(body) == 0 && run.Status == protocol.ConversationStatusQueued {
		var guided bool
		if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_pending WHERE turn=?1 AND recipient IS NOT NULL)", run.Turn.String()).Scan(&guided); err != nil {
			return run, err
		}
		if guided {
			q, e := readQueuedMessage(ctx, db, run.Turn)
			if e != nil {
				return run, e
			}
			body, e = encode(q.Message)
			if e != nil {
				return run, e
			}
		}
	}
	if len(body) > 0 {
		var input protocol.ConversationInput
		if err = json.Unmarshal(body, &input); err != nil {
			return run, err
		}
		attachments, e := ListTurnAttachments(ctx, db, run.Turn)
		if e != nil {
			return run, e
		}
		run.Input = &protocol.ConversationSentInput{Message: input, Attachments: attachments}
	}
	return run, nil
}
func historyEntries(ctx context.Context, db *sql.Conn, session protocol.SessionID, turns []protocol.TurnID, before *uint64, limit uint16, byTurn bool) ([]protocol.ConversationEntry, bool, error) {
	boundary := uint64(math.MaxInt64)
	if before != nil {
		boundary = *before
	}
	type rowValue struct {
		sequence uint64
		record   canonical.Record
	}
	requests := toolRequests{}
	records := []rowValue{}
	size := 0
	more := false
	read := func(query string, selector string) error {
		rows, err := db.QueryContext(ctx, query, session.String(), selector, boundary, int(limit)-len(records)+1)
		if err != nil {
			return err
		}
		defer rows.Close()
		for rows.Next() {
			var seq uint64
			var body sql.RawBytes
			if err = rows.Scan(&seq, &body); err != nil {
				return err
			}
			if len(records) >= int(limit) || len(records) > 0 && size+len(body) > 3*1024*1024 {
				more = true
				break
			}
			var record canonical.Record
			if err = json.Unmarshal(body, &record); err != nil {
				return err
			}
			requests.include(record)
			records = append(records, rowValue{seq, record})
			size += len(body)
		}
		return errors.Join(rows.Err(), rows.Close())
	}
	if byTurn || len(turns) == 1 {
		// Backward browsing uses a turn cursor. Fill the page in that same order
		// so a long earlier turn cannot strand newer, short turns behind individual reads.
		const query = "SELECT e.sequence,e.body FROM agent_events e CROSS JOIN conversation_events h ON h.sequence=e.sequence AND h.session=?1 WHERE e.turn=?2 AND e.sequence<?3 AND " + visibleCanonical + " ORDER BY e.sequence DESC LIMIT ?4"
		for i := len(turns) - 1; i >= 0 && !more; i-- {
			if err := read(query, turns[i].String()); err != nil {
				return nil, false, err
			}
		}
	} else {
		// The live head needs the newest events across all interleaved turns.
		ids := make([]string, len(turns))
		for i, id := range turns {
			ids[i] = id.String()
		}
		members, err := encode(ids)
		if err != nil {
			return nil, false, err
		}
		const query = "SELECT e.sequence,e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND e.turn IN (SELECT value FROM json_each(?2)) AND h.sequence<?3 AND " + visibleCanonical + " ORDER BY h.sequence DESC LIMIT ?4"
		if err = read(query, string(members)); err != nil {
			return nil, false, err
		}
	}
	if byTurn && len(turns) > 1 {
		slices.SortFunc(records, func(a, b rowValue) int { return cmp.Compare(a.sequence, b.sequence) })
	} else {
		slices.Reverse(records)
	}
	out := make([]protocol.ConversationEntry, 0, len(records))
	for i := range records {
		entry, err := projectStoredEntry(ctx, db, records[i].record, records[i].sequence, requests)
		if err != nil {
			return nil, false, err
		}
		records[i] = rowValue{}
		out = append(out, entry)
	}
	return out, more, nil
}
func readConversationPage(ctx context.Context, db *sql.Conn, session protocol.SessionID, runs []protocol.ConversationRun, entries []protocol.ConversationEntry, next *protocol.TurnID) (protocol.ConversationPage, error) {
	page := protocol.DefaultConversationPage()
	page.Session = session
	page.Runs = runs
	page.Entries = entries
	page.NextBefore = next
	var err error
	if page.Revision, err = historyRevision(ctx, db, session); err != nil {
		return page, err
	}
	if page.Queue, err = ReadQueue(ctx, db, session); err != nil {
		return page, err
	}
	ids := []string{}
	for _, entry := range entries {
		ids = append(ids, entry.ID)
	}
	body, err := encode(ids)
	if err != nil {
		return page, err
	}
	if page.Approvals, err = readBodies[protocol.ConversationApproval](ctx, db, "SELECT a.body FROM agent_approvals a JOIN conversation_turns h ON h.turn=a.turn WHERE h.session=?1 AND (json_extract(a.body,'$.state')='pending' OR json_extract(a.body,'$.entry') IN (SELECT value FROM json_each(?2))) ORDER BY a.rowid", session.String(), string(body)); err != nil {
		return page, err
	}
	for i := range page.Approvals {
		page.Approvals[i].Session = session
	}
	if page.Questions, err = readBodies[protocol.ConversationQuestion](ctx, db, "SELECT q.body FROM agent_questions q JOIN conversation_turns h ON h.turn=q.turn WHERE h.session=?1 AND (json_extract(q.body,'$.state.kind')='pending' OR json_extract(q.body,'$.entry') IN (SELECT value FROM json_each(?2))) ORDER BY q.rowid", session.String(), string(body)); err != nil {
		return page, err
	}
	for i := range page.Questions {
		page.Questions[i].Session = session
	}
	page.Children, err = conversationChildren(ctx, db, entries)
	if err != nil {
		return page, err
	}
	page.Background, err = ReadBackgroundTasks(ctx, db, session)
	if err != nil {
		return page, err
	}
	retainConversationMetadata(&page)
	for i := range page.Runs {
		page.Runs[i], err = hydratePublicRun(ctx, db, page.Runs[i])
		if err != nil {
			return page, err
		}
	}
	return page, nil
}

// historyOwners maps each run to the recipient whose history it belongs to.
// An orphan in older stored history can still be read on its own.
func historyOwners(runs []protocol.ConversationRun) map[protocol.TurnID]protocol.ConversationRun {
	owners := make(map[protocol.TurnID]protocol.ConversationRun, len(runs))
	for _, run := range runs {
		owners[run.Turn] = run
	}
	for _, run := range runs {
		if run.Joined != nil {
			if recipient, ok := owners[*run.Joined]; ok && recipient.Joined == nil {
				owners[run.Turn] = recipient
			}
		}
	}
	return owners
}

const historyUnjoined = `NOT EXISTS(SELECT 1 FROM conversation_turns recipient
WHERE recipient.session=h.session AND recipient.turn=json_extract(a.body,'$.joined'))`

// historyRuns selects the page's recipients before reading their guidance.
// A recipient's admission sequence is the exclusive page boundary for its
// entire group, including guidance admitted on either side of the boundary.
func historyRuns(ctx context.Context, db *sql.Conn, session protocol.SessionID, before *protocol.TurnID, limit uint16) ([]protocol.ConversationRun, *protocol.TurnID, error) {
	boundary := uint64(math.MaxInt64)
	var cursor string
	if before != nil {
		run, err := visibleAgentRun(ctx, db, session, *before)
		if err != nil {
			return nil, nil, err
		}
		boundary, cursor = run.Sequence, run.Turn.String()
	}
	runs, err := readBodies[protocol.ConversationRun](ctx, db, `SELECT a.body FROM agent_runs a JOIN turns t ON t.id=a.turn JOIN conversation_turns h ON h.turn=a.turn
WHERE h.session=?1 AND t.rowid<?2 AND json_extract(a.body,'$.status')!='queued'
AND (json_extract(a.body,'$.status')!='cancelled' OR a.input IS NOT NULL OR EXISTS(SELECT 1 FROM agent_events e WHERE e.turn=a.turn))
AND `+historyUnjoined+` ORDER BY t.rowid DESC LIMIT ?3`, session.String(), boundary, int(limit)+1)
	if err != nil {
		return nil, nil, err
	}
	var next *protocol.TurnID
	if len(runs) > int(limit) {
		runs = runs[:limit]
		next = new(runs[len(runs)-1].Turn)
	}
	if before == nil {
		active, err := readBodies[protocol.ConversationRun](ctx, db, `SELECT a.body FROM agent_runs a JOIN turns t ON t.id=a.turn JOIN conversation_turns h ON h.turn=a.turn
WHERE h.session=?1 AND (json_extract(a.body,'$.status') IN ('running','stopping','queued') OR (json_extract(a.body,'$.status')='cancelled' AND a.input IS NULL AND NOT EXISTS(SELECT 1 FROM agent_events e WHERE e.turn=a.turn)))
AND `+historyUnjoined+` ORDER BY (json_extract(a.body,'$.status') IN ('running','stopping')) DESC,t.rowid DESC LIMIT 100`, session.String())
		if err != nil {
			return nil, nil, err
		}
		for _, run := range active {
			if !slices.ContainsFunc(runs, func(candidate protocol.ConversationRun) bool { return candidate.Turn == run.Turn }) {
				runs = append(runs, run)
			}
		}
	}
	if len(runs) == 0 {
		return runs, next, nil
	}
	ids := make([]protocol.TurnID, len(runs))
	for i, run := range runs {
		ids[i] = run.Turn
	}
	selected, err := encode(ids)
	if err != nil {
		return nil, nil, err
	}
	// A page from an older Node could already include a guidance run as
	// its cursor. Fill in that run's recipient without repeating the cursor.
	guidance, err := readBodies[protocol.ConversationRun](ctx, db, `SELECT a.body FROM conversation_turns h CROSS JOIN agent_runs a ON a.turn=h.turn
WHERE h.session=?1 AND json_extract(a.body,'$.joined') IN (SELECT value FROM json_each(?2))
AND h.turn!=?3`, session.String(), string(selected), cursor)
	if err != nil {
		return nil, nil, err
	}
	runs = append(runs, guidance...)
	slices.SortFunc(runs, func(a, b protocol.ConversationRun) int { return cmp.Compare(a.Sequence, b.Sequence) })
	for i := range runs {
		if runs[i].Session != session {
			origin := runs[i].Session
			runs[i].Origin = &origin
			runs[i].Session = session
		}
	}
	return runs, next, nil
}

func ReadConversation(ctx context.Context, db *sql.Conn, session protocol.SessionID, before *protocol.TurnID, limit uint16) (protocol.ConversationHistory, error) {
	value := protocol.DefaultConversationHistory()
	if limit == 0 {
		return value, invalid("history limit must be positive")
	}
	limit = min(limit, 100)
	if err := requireSession(ctx, db, session); err != nil {
		return value, err
	}
	runs, next, err := historyRuns(ctx, db, session, before, limit)
	if err != nil {
		return value, err
	}
	owners := historyOwners(runs)
	ids := make([]protocol.TurnID, len(runs))
	for i, run := range runs {
		ids[i] = run.Turn
	}
	if before != nil {
		// Pack entries from newer logical turns first even when an older
		// recipient's guidance was admitted after them.
		slices.SortStableFunc(ids, func(a, b protocol.TurnID) int { return cmp.Compare(owners[a].Sequence, owners[b].Sequence) })
	}
	entries, _, err := historyEntries(ctx, db, session, ids, nil, 100, before != nil)
	if err != nil {
		return value, err
	}
	// Closed groups entirely preceding the oldest retained group remain on
	// older pages.
	if len(entries) > 0 {
		oldest := uint64(math.MaxUint64)
		for _, entry := range entries {
			oldest = min(oldest, owners[entry.Turn].Sequence)
		}
		newestRemoved := uint64(0)
		blocked := false
		for _, r := range runs {
			if owners[r.Turn].Turn == r.Turn && r.Sequence < oldest && !activeRunStatus(r.Status) {
				newestRemoved = max(newestRemoved, r.Sequence)
			}
		}
		for _, r := range runs {
			if activeRunStatus(r.Status) && r.Sequence < newestRemoved {
				blocked = true
			}
		}
		if !blocked && newestRemoved > 0 {
			runs = slices.DeleteFunc(runs, func(r protocol.ConversationRun) bool {
				owner := owners[r.Turn]
				return owner.Sequence < oldest && !activeRunStatus(owner.Status)
			})
			for _, run := range runs {
				if owners[run.Turn].Turn == run.Turn {
					next = new(run.Turn)
					break
				}
			}
		}
	}
	page, err := readConversationPage(ctx, db, session, runs, entries, next)
	if err != nil {
		return value, err
	}
	trimmed, err := FitConversationPage(&page, 0)
	if err != nil {
		return value, err
	}
	missing := map[protocol.TurnID]bool{}
	for _, id := range trimmed {
		missing[id] = true
	}
	// The turn-first scan stops at its first visible sequence. Drive the
	// membership check by that index instead of walking the whole session.
	const firstVisible = "SELECT (SELECT e.sequence FROM agent_events e CROSS JOIN conversation_events h ON h.sequence=e.sequence AND h.session=?1 WHERE e.turn=?2 AND " + visibleCanonical + " ORDER BY e.sequence LIMIT 1)"
	for _, r := range page.Runs {
		var first sql.NullInt64
		if err = db.QueryRowContext(ctx, firstVisible, session.String(), r.Turn.String()).Scan(&first); err != nil {
			return value, err
		}
		if first.Valid && !slices.ContainsFunc(page.Entries, func(e protocol.ConversationEntry) bool { return e.Sequence == uint64(first.Int64) }) {
			missing[r.Turn] = true
		}
	}
	value.Page = page
	for _, r := range page.Runs {
		if missing[r.Turn] {
			value.Missing = append(value.Missing, r.Turn)
		}
	}
	return value, nil
}
func activeRunStatus(status protocol.ConversationStatus) bool {
	return status == protocol.ConversationStatusQueued || status == protocol.ConversationStatusRunning || status == protocol.ConversationStatusStopping
}
func ReadConversationTurn(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID, revision uint64, before *uint64, limit uint16) (protocol.ConversationTurnHistory, error) {
	value := protocol.DefaultConversationTurnHistory()
	if limit == 0 {
		return value, invalid("history limit must be positive")
	}
	if before != nil && *before > math.MaxInt64 {
		return value, invalid("turn history cursor is invalid")
	}
	actual, err := historyRevision(ctx, db, session)
	if err != nil {
		return value, err
	}
	if err = checkRevision(actual, revision); err != nil {
		return value, err
	}
	run, err := visibleAgentRun(ctx, db, session, turn)
	if err != nil {
		return value, err
	}
	entries, more, err := historyEntries(ctx, db, session, []protocol.TurnID{turn}, before, min(limit, 1000), true)
	if err != nil {
		return value, err
	}
	count := len(entries)
	page, err := readConversationPage(ctx, db, session, []protocol.ConversationRun{run}, entries, nil)
	if err != nil {
		return value, err
	}
	if _, err = fitConversationPage(&page, 0, false); err != nil {
		return value, err
	}
	value.Revision = actual
	value.Run = page.Runs[0]
	value.Entries = page.Entries
	value.Approvals = page.Approvals
	value.Questions = page.Questions
	value.Children = page.Children
	if (more || len(page.Entries) != count) && len(page.Entries) > 0 {
		n := page.Entries[0].Sequence
		value.NextBefore = &n
	}
	return value, nil
}
func FitConversationPage(page *protocol.ConversationPage, extraBytes int) ([]protocol.TurnID, error) {
	return fitConversationPage(page, extraBytes, true)
}
func fitConversationPage(page *protocol.ConversationPage, extraBytes int, prune bool) ([]protocol.TurnID, error) {
	budget := protocol.MAX_FRAME_BYTES - 64*1024 - extraBytes
	if budget < 0 {
		return nil, invalid("conversation resources exceed the transport limit")
	}
	// Each entry is encoded once. The page's size is its metadata, its entries
	// and the commas between them, so dropping an entry re-encodes only the
	// metadata.
	sizes := make([]int, len(page.Entries))
	entryBytes := 0
	for i := range page.Entries {
		body, err := encode(page.Entries[i])
		if err != nil {
			return nil, err
		}
		sizes[i] = len(body)
		entryBytes += len(body)
	}
	trimmed := map[protocol.TurnID]bool{}
	owners := historyOwners(page.Runs)
	for {
		entries := page.Entries
		page.Entries = entries[:0]
		metadata, err := encode(page)
		page.Entries = entries
		if err != nil {
			return nil, err
		}
		if len(metadata)+entryBytes+max(len(entries)-1, 0) <= budget {
			break
		}
		if len(page.Entries) > 1 {
			trimmed[page.Entries[0].Turn] = true
			page.Entries[0] = protocol.ConversationEntry{}
			page.Entries = page.Entries[1:]
			entryBytes -= sizes[0]
			sizes = sizes[1:]
			retainConversationMetadata(page)
			continue
		}
		var first, next *protocol.TurnID
		for _, run := range page.Runs {
			if owners[run.Turn].Turn != run.Turn || activeRunStatus(run.Status) {
				continue
			}
			if first == nil {
				first = new(run.Turn)
			} else {
				next = new(run.Turn)
				break
			}
		}
		if !prune || next == nil {
			return nil, invalid("conversation metadata exceeds the transport limit")
		}
		dropped := func(turn protocol.TurnID) bool { return owners[turn].Turn == *first }
		page.Runs = slices.DeleteFunc(page.Runs, func(r protocol.ConversationRun) bool { return dropped(r.Turn) })
		kept := 0
		for i, entry := range page.Entries {
			if dropped(entry.Turn) {
				entryBytes -= sizes[i]
				continue
			}
			page.Entries[kept], sizes[kept] = entry, sizes[i]
			kept++
		}
		clear(page.Entries[kept:])
		page.Entries, sizes = page.Entries[:kept], sizes[:kept]
		page.Approvals = slices.DeleteFunc(page.Approvals, func(a protocol.ConversationApproval) bool { return dropped(a.Turn) })
		page.Questions = slices.DeleteFunc(page.Questions, func(q protocol.ConversationQuestion) bool { return dropped(q.Turn) })
		page.Children = slices.DeleteFunc(page.Children, func(c protocol.ConversationChild) bool { return dropped(c.Origin.Turn) })
		page.NextBefore = next
	}
	ids := []protocol.TurnID{}
	for _, r := range page.Runs {
		if trimmed[r.Turn] {
			ids = append(ids, r.Turn)
		}
	}
	return ids, nil
}
func retainConversationMetadata(page *protocol.ConversationPage) {
	present := map[string]bool{}
	for _, e := range page.Entries {
		present[e.ID] = true
	}
	page.Approvals = slices.DeleteFunc(page.Approvals, func(a protocol.ConversationApproval) bool {
		return a.State != protocol.ConversationApprovalStatePending && !present[a.Entry]
	})
	page.Questions = slices.DeleteFunc(page.Questions, func(q protocol.ConversationQuestion) bool { return q.State.Kind != "pending" && !present[q.Entry] })
	page.Children = slices.DeleteFunc(page.Children, func(c protocol.ConversationChild) bool { return !present[c.Origin.Entry] })
}
