package client

import (
	"cmp"
	"context"
	"reflect"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	turnHistoryPageLimit    uint16 = 500
	fullHistoryRestoreLimit uint16 = 1000
)

func (c *Client) ReadConversation(ctx context.Context, session protocol.SessionID, before *protocol.TurnID) (protocol.ConversationHistory, error) {
	out, err := c.read(ctx, protocol.Command{Kind: "read_conversation", Data: protocol.CommandReadConversationPayload{Session: session, Before: before, Limit: 20}})
	if err != nil {
		return protocol.ConversationHistory{}, err
	}
	h, err := payload[protocol.ConversationHistory](out.Data)
	if out.Kind != "conversation" || err != nil {
		return h, fault(protocol.ErrorCodeInvalidRequest, "conversation history response expected")
	}
	if h.Page.Session != session {
		return h, fault(protocol.ErrorCodeWrongTarget, "history belongs to another conversation")
	}
	if err = validateConversationPage(h.Page); err != nil {
		return h, err
	}
	for _, turn := range h.Missing {
		if !slices.ContainsFunc(h.Page.Runs, func(r protocol.ConversationRun) bool { return r.Turn == turn }) {
			return h, fault(protocol.ErrorCodeInvalidRequest, "missing history references an unknown turn")
		}
	}
	return h, nil
}

func (c *Client) readConversationTurn(ctx context.Context, session protocol.SessionID, turn protocol.TurnID, revision uint64, before *uint64, limit uint16) (protocol.ConversationTurnHistory, error) {
	out, err := c.read(ctx, protocol.Command{Kind: "read_turn", Data: protocol.CommandReadTurnPayload{Session: session, Turn: turn, ExpectedRevision: revision, Before: before, Limit: limit}})
	if err != nil {
		return protocol.ConversationTurnHistory{}, err
	}
	h, err := payload[protocol.ConversationTurnHistory](out.Data)
	if out.Kind != "turn_history" || err != nil {
		return h, fault(protocol.ErrorCodeInvalidRequest, "turn history response expected")
	}
	if h.Run.Session != session || h.Run.Turn != turn {
		return h, fault(protocol.ErrorCodeWrongTarget, "turn history belongs to another query")
	}
	if len(h.Entries) == 0 && h.NextBefore != nil {
		return h, fault(protocol.ErrorCodeInvalidRequest, "empty history continuation did not advance")
	}
	for _, e := range h.Entries {
		if e.Turn != turn || before != nil && e.Sequence >= *before {
			return h, fault(protocol.ErrorCodeInvalidRequest, "history continuation crossed its boundary")
		}
	}
	page := protocol.ConversationPage{Session: session, Revision: h.Revision, Entries: h.Entries, Runs: []protocol.ConversationRun{h.Run}, Approvals: h.Approvals, Questions: h.Questions, Children: h.Children}
	if err := validateConversationPage(page); err != nil {
		return h, err
	}
	if h.NextBefore != nil && (len(h.Entries) == 0 || h.Entries[0].Sequence != *h.NextBefore) {
		return h, fault(protocol.ErrorCodeInvalidRequest, "history continuation cursor did not advance")
	}
	return h, nil
}

// LoadOlderConversation coalesces repeated requests and completes the oldest
// partial turn before requesting the next twenty logical turns.
func (c *Client) LoadOlderConversation(ctx context.Context, session protocol.SessionID) error {
	return c.loadOlderConversation(ctx, session, turnHistoryPageLimit, false)
}

// LoadBlankConversationTurn restores one turn omitted entirely from a fitted
// page. It leaves partially loaded turns and earlier pages for scrolling.
func (c *Client) LoadBlankConversationTurn(ctx context.Context, session protocol.SessionID) error {
	return c.loadOlderConversation(ctx, session, turnHistoryPageLimit, true)
}

// HasBlankHistoryTurn reports a displayed run whose entries were omitted
// from its page. A joined member has no separate row to recover.
func (s ConversationState) HasBlankHistoryTurn() bool {
	return missingConversationTurn(s.Snapshot, true) != nil
}

func (c *Client) loadOlderConversation(ctx context.Context, session protocol.SessionID, turnLimit uint16, blank bool) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	c.conversationMu.Lock()
	w := c.conversations[session]
	if w == nil || w.projection.snapshot == nil || w.ctx.Err() != nil {
		c.conversationMu.Unlock()
		return fault(protocol.ErrorCodeUnavailable, "conversation is not loaded")
	}
	if w.state.LoadingOlder {
		c.conversationMu.Unlock()
		return nil
	}
	snapshot := w.projection.snapshot
	partial := missingConversationTurn(*snapshot, blank)
	if blank && partial == nil {
		c.conversationMu.Unlock()
		return nil
	}
	w.state.LoadingOlder = true
	w.state.OlderErr = nil
	generation := w.projection.generation
	revision := snapshot.Page.Revision
	var beforeEntry *uint64
	var beforeTurn *protocol.TurnID
	var beforeRunSequence uint64
	var missing []protocol.TurnID
	if partial != nil {
		beforeEntry = earliestEntry(*snapshot, *partial)
		missing = slices.Clone(snapshot.Missing)
	} else if next := snapshot.Page.NextBefore; next != nil {
		beforeTurn = new(*next)
		for _, run := range snapshot.Page.Runs {
			if run.Turn == *next {
				beforeRunSequence = run.Sequence
				break
			}
		}
	}
	c.conversationMu.Unlock()
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()
	stop := context.AfterFunc(w.ctx, cancel)
	defer stop()
	c.conversationChanged(w)
	var incoming protocol.ConversationPage
	var sequence uint64
	var err error
	if partial != nil {
		var h protocol.ConversationTurnHistory
		h, err = c.readConversationTurn(ctx, session, *partial, revision, beforeEntry, turnLimit)
		if err == nil {
			incoming = protocol.ConversationPage{Session: session, Revision: h.Revision, Entries: h.Entries, Runs: []protocol.ConversationRun{h.Run}, Approvals: h.Approvals, Questions: h.Questions, Children: h.Children}
			sequence = h.Sequence
			if h.NextBefore == nil {
				missing = slices.DeleteFunc(missing, func(t protocol.TurnID) bool { return t == *partial })
			}
		}
	} else if beforeTurn != nil {
		var h protocol.ConversationHistory
		h, err = c.ReadConversation(ctx, session, beforeTurn)
		if err == nil {
			incoming, missing, sequence = h.Page, h.Missing, h.Sequence
			if beforeRunSequence != 0 && !historyPrecedes(incoming, beforeRunSequence) {
				err = fault(protocol.ErrorCodeInvalidRequest, "history page crossed its turn boundary")
			}
		}
	} else {
		c.conversationMu.Lock()
		w.state.LoadingOlder = false
		c.conversationMu.Unlock()
		c.conversationChanged(w)
		return nil
	}
	if err == nil {
		err = c.waitConversationSequence(ctx, w, generation, sequence)
	}
	c.conversationMu.Lock()
	if c.conversations[session] != w || generation != w.projection.generation || w.projection.snapshot != snapshot || w.projection.recovering {
		if err == nil {
			err = fault(protocol.ErrorCodeBusy, "conversation reconnected during history read")
		}
	}
	if err == nil {
		current := w.projection.snapshot
		err = mergeConversationPage(&current.Page, incoming, true, w.projection.hidden)
		if err == nil {
			if partial == nil {
				current.Page.NextBefore = incoming.NextBefore
				current.Missing = append(current.Missing, missing...)
			} else {
				current.Missing = missing
			}
			slices.SortFunc(current.Missing, func(a, b protocol.TurnID) int { return slices.Compare(a[:], b[:]) })
			current.Missing = slices.Compact(current.Missing)
			w.projection.settle()
			w.viewVersion++
			w.rememberExtent()
		}
	}
	w.state.LoadingOlder = false
	w.state.OlderErr = err
	c.conversationMu.Unlock()
	c.conversationChanged(w)
	if faultCode(err) == protocol.ErrorCodeRevisionConflict {
		_ = c.RefreshConversation(ctx, session)
	}
	return err
}

// historyPrecedes checks the exclusive cursor against logical runs. Returned
// guidance can cross it only with its recipient present on the older page.
func historyPrecedes(page protocol.ConversationPage, before uint64) bool {
	for _, run := range page.Runs {
		if run.Sequence < before {
			continue
		}
		if run.Sequence == before || (page.NextBefore != nil && *page.NextBefore == run.Turn) ||
			conversationOwnerSequence(page, run) >= before {
			return false
		}
	}
	return true
}

func (c *Client) waitConversationSequence(ctx context.Context, w *conversationWatch, generation, sequence uint64) error {
	for {
		c.conversationMu.Lock()
		valid := c.conversations[w.session] == w && w.projection.generation == generation
		ready := valid && w.projection.snapshot != nil && w.projection.snapshot.Sequence >= sequence
		observationErr := w.state.Err
		signal := w.signal
		c.conversationMu.Unlock()
		if !valid {
			return fault(protocol.ErrorCodeBusy, "conversation generation changed")
		}
		if ready {
			return nil
		}
		if observationErr != nil && !retryConversation(observationErr) {
			return observationErr
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-w.ctx.Done():
			return w.ctx.Err()
		case <-signal:
		}
	}
}

func mergeConversationPage(current *protocol.ConversationPage, incoming protocol.ConversationPage, keep bool, hidden map[string]bool) error {
	if err := validateConversationPage(incoming); err != nil {
		return err
	}
	if current.Session != incoming.Session {
		return fault(protocol.ErrorCodeWrongTarget, "history merge belongs to another session")
	}
	if current.Revision != incoming.Revision {
		return fault(protocol.ErrorCodeRevisionConflict, "conversation changed during pagination")
	}
	// Merge into private lists so a rejected page leaves current unchanged.
	// Entries and their payloads stay shared and are never written.
	merged := *current
	var err error
	merged.Entries, err = mergeHistoryEntries(current.Entries, incoming.Entries, hidden)
	if err != nil {
		return err
	}
	merged.Runs = slices.Clone(current.Runs)
	merged.Approvals, merged.Questions = slices.Clone(current.Approvals), slices.Clone(current.Questions)
	merged.Children, merged.Background = slices.Clone(current.Children), slices.Clone(current.Background)
	byTurn := make(map[protocol.TurnID]int, len(merged.Runs))
	for i := len(merged.Runs) - 1; i >= 0; i-- {
		byTurn[merged.Runs[i].Turn] = i
	}
	for _, r := range incoming.Runs {
		if i, found := byTurn[r.Turn]; found {
			if !sameRun(merged.Runs[i], r) {
				return fault(protocol.ErrorCodeInvalidRequest, "sent turn changed during pagination")
			}
			if !keep {
				merged.Runs[i] = r
			}
		} else {
			merged.Runs = append(merged.Runs, r)
			byTurn[r.Turn] = len(merged.Runs) - 1
		}
	}
	slices.SortFunc(merged.Runs, func(a, b protocol.ConversationRun) int { return cmp.Compare(a.Sequence, b.Sequence) })
	for _, a := range incoming.Approvals {
		if i := slices.IndexFunc(merged.Approvals, func(v protocol.ConversationApproval) bool { return v.ID == a.ID }); i >= 0 {
			if err := checkApprovalTransition(merged.Approvals[i], a, keep); err != nil {
				return err
			}
		}
		merged.Approvals = upsertConversation(merged.Approvals, a, func(v protocol.ConversationApproval) protocol.ApprovalID { return v.ID }, keep)
	}
	for _, q := range incoming.Questions {
		if i := slices.IndexFunc(merged.Questions, func(v protocol.ConversationQuestion) bool { return v.ID == q.ID }); i >= 0 {
			if err := checkQuestionTransition(merged.Questions[i], q, keep); err != nil {
				return err
			}
		}
		merged.Questions = upsertConversation(merged.Questions, q, func(v protocol.ConversationQuestion) protocol.QuestionID { return v.ID }, keep)
	}
	for _, child := range incoming.Children {
		if err := mergeConversationChild(&merged, child, keep); err != nil {
			return err
		}
	}
	for _, task := range incoming.Background {
		if err := mergeConversationBackground(&merged, task); err != nil {
			return err
		}
	}
	sent := map[protocol.TurnID]bool{}
	for _, e := range merged.Entries {
		if e.Author == "user" {
			sent[e.Turn] = true
		}
	}
	for i := range merged.Runs {
		if sent[merged.Runs[i].Turn] {
			merged.Runs[i].Input = nil
		}
	}
	if err := validateConversationPage(merged); err != nil {
		return err
	}
	*current = merged
	return nil
}

// Both inputs follow canonical sequence order. The merged page's validation
// still rejects an ID reused at another sequence.
func mergeHistoryEntries(held, incoming []protocol.ConversationEntry, hidden map[string]bool) ([]protocol.ConversationEntry, error) {
	if len(incoming) == 0 {
		return held, nil
	}
	merged := make([]protocol.ConversationEntry, 0, len(held)+len(incoming))
	for _, entry := range incoming {
		if hidden[entry.ID] {
			continue
		}
		for len(held) > 0 && held[0].Sequence < entry.Sequence {
			merged = append(merged, held[0])
			held = held[1:]
		}
		if len(held) > 0 && held[0].Sequence == entry.Sequence {
			if !reflect.DeepEqual(held[0], entry) {
				return nil, fault(protocol.ErrorCodeInvalidRequest, "canonical event changed during pagination")
			}
			entry, held = held[0], held[1:]
		}
		merged = append(merged, entry)
	}
	return append(merged, held...), nil
}

func oldestMissing(s protocol.ConversationSnapshot) *protocol.TurnID {
	return missingConversationTurn(s, false)
}

func missingConversationTurn(s protocol.ConversationSnapshot, blank bool) *protocol.TurnID {
	if len(s.Missing) == 0 {
		return nil
	}
	var oldest *protocol.TurnID
	var sequence uint64
	for _, r := range s.Page.Runs {
		if !slices.Contains(s.Missing, r.Turn) {
			continue
		}
		owner := conversationOwnerSequence(s.Page, r)
		if blank && (owner != r.Sequence || earliestEntry(s, r.Turn) != nil) {
			continue
		}
		if oldest == nil || owner < sequence {
			oldest, sequence = new(r.Turn), owner
		}
	}
	return oldest
}

func conversationOwnerSequence(page protocol.ConversationPage, run protocol.ConversationRun) uint64 {
	if run.Joined != nil {
		for _, recipient := range page.Runs {
			if recipient.Turn == *run.Joined && recipient.Joined == nil {
				return recipient.Sequence
			}
		}
	}
	return run.Sequence
}

func earliestEntry(s protocol.ConversationSnapshot, turn protocol.TurnID) *uint64 {
	for _, e := range s.Page.Entries {
		if e.Turn == turn {
			v := e.Sequence
			return &v
		}
	}
	return nil
}

type conversationExtent struct {
	all     bool
	turn    uint64
	member  uint64
	entry   *uint64
	partial bool
}

func conversationExtentOf(s protocol.ConversationSnapshot) conversationExtent {
	if t := oldestMissing(s); t != nil {
		for _, r := range s.Page.Runs {
			if r.Turn == *t {
				return conversationExtent{turn: conversationOwnerSequence(s.Page, r), member: r.Sequence, entry: earliestEntry(s, *t), partial: true}
			}
		}
	}
	if s.Page.NextBefore == nil {
		return conversationExtent{all: len(s.Page.Runs) > 0}
	}
	for _, r := range s.Page.Runs {
		if r.Turn == *s.Page.NextBefore {
			return conversationExtent{turn: r.Sequence}
		}
	}
	return conversationExtent{}
}
func (e conversationExtent) needsMore(s protocol.ConversationSnapshot) bool {
	if e.all {
		return s.Page.NextBefore != nil || len(s.Missing) > 0
	}
	if e.turn == 0 {
		return false
	}
	for _, r := range s.Page.Runs {
		if s.Page.NextBefore != nil && r.Turn == *s.Page.NextBefore && r.Sequence > e.turn {
			return true
		}
		if slices.Contains(s.Missing, r.Turn) {
			owner := conversationOwnerSequence(s.Page, r)
			if owner > e.turn || owner == e.turn && (!e.partial || r.Sequence < e.member) {
				return true
			}
			if owner == e.turn && r.Sequence == e.member && e.entry != nil {
				v := earliestEntry(s, r.Turn)
				if v == nil || *v > *e.entry {
					return true
				}
			}
		}
	}
	return false
}
