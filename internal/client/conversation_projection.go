package client

import (
	"cmp"
	"reflect"
	"slices"
	"strconv"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// ConversationProjection retains one session's loaded canonical range.
type ConversationProjection struct {
	node           protocol.NodeID
	session        protocol.SessionID
	generation     uint64
	snapshot       *protocol.ConversationSnapshot
	recovering     bool
	liveStart      *uint64
	artifactCursor uint64
	statisticsErr  error
	hidden         map[string]bool
	revealed       map[string]protocol.ConversationEntry
}

// Reveals outside the loaded extent wait for pagination, so their sequence
// cannot advance the continuation cursor past an unloaded gap.
func (p *ConversationProjection) settle() {
	if p.snapshot == nil {
		return
	}
	s := p.snapshot
	for id, entry := range p.revealed {
		if slices.ContainsFunc(s.Page.Entries, func(e protocol.ConversationEntry) bool { return e.ID == id }) {
			delete(p.revealed, id)
			continue
		}
		loaded := slices.ContainsFunc(s.Page.Runs, func(r protocol.ConversationRun) bool { return r.Turn == entry.Turn })
		if slices.Contains(s.Missing, entry.Turn) {
			first := earliestEntry(*s, entry.Turn)
			loaded = loaded && first != nil && *first < entry.Sequence
		}
		if !loaded {
			continue
		}
		if !slices.ContainsFunc(s.Page.Entries, func(e protocol.ConversationEntry) bool { return e.Sequence == entry.Sequence }) {
			if p.liveStart == nil && (len(s.Page.Entries) == 0 || entry.Sequence > s.Page.Entries[len(s.Page.Entries)-1].Sequence) {
				p.liveStart = new(entry.Sequence)
			}
			s.Page.Entries = append(s.Page.Entries, entry)
			slices.SortFunc(s.Page.Entries, func(a, b protocol.ConversationEntry) int { return cmp.Compare(a.Sequence, b.Sequence) })
			s.Drafts = slices.DeleteFunc(s.Drafts, func(d protocol.ConversationDraft) bool { return d.Turn == entry.Turn && d.ID == entry.ID })
			if entry.Author == "user" {
				for i := range s.Page.Runs {
					if s.Page.Runs[i].Turn == entry.Turn {
						s.Page.Runs[i].Input = nil
					}
				}
			}
		}
		delete(p.revealed, id)
	}
}

func NewConversationProjection(node protocol.NodeID, session protocol.SessionID, generation uint64) *ConversationProjection {
	return &ConversationProjection{node: node, session: session, generation: generation, recovering: true, hidden: map[string]bool{}, revealed: map[string]protocol.ConversationEntry{}}
}

func (p *ConversationProjection) Snapshot() *protocol.ConversationSnapshot {
	if p.snapshot == nil {
		return nil
	}
	v := copyValue(*p.snapshot)
	return &v
}

func (p *ConversationProjection) Reconnect(generation uint64) error {
	if generation <= p.generation {
		return fault(protocol.ErrorCodeInvalidRequest, "conversation generation must increase")
	}
	p.generation, p.recovering = generation, true
	return nil
}

func (p *ConversationProjection) Apply(generation uint64, update protocol.Update) (Apply, error) {
	if generation != p.generation {
		return Ignored, nil
	}
	if update.Kind == "reset_required" {
		p.recovering = true
		return Recover, nil
	}
	if update.Kind == "conversation_snapshot" {
		v, err := payload[protocol.ConversationSnapshot](update.Data)
		if err != nil {
			return Ignored, err
		}
		if v.Node != p.node || v.Page.Session != p.session {
			return Ignored, fault(protocol.ErrorCodeWrongTarget, "conversation snapshot belongs to another resource")
		}
		if err := validateConversationPage(v.Page); err != nil {
			return Ignored, err
		}
		missing := map[protocol.TurnID]bool{}
		for _, turn := range v.Missing {
			if missing[turn] || !slices.ContainsFunc(v.Page.Runs, func(r protocol.ConversationRun) bool { return r.Turn == turn }) {
				return Ignored, fault(protocol.ErrorCodeInvalidRequest, "incomplete history references an unknown or repeated turn")
			}
			missing[turn] = true
		}
		for _, draft := range v.Drafts {
			if err := validateConversationDraft(v.Page, draft); err != nil {
				return Ignored, err
			}
		}
		if !p.recovering && p.snapshot != nil && v.Sequence < p.snapshot.Sequence {
			return Ignored, nil
		}
		if p.artifactCursor == ^uint64(0) {
			return Ignored, fault(protocol.ErrorCodeInvalidRequest, "artifact cursor exhausted")
		}
		p.snapshot = new(copyValue(v))
		p.recovering = false
		p.liveStart = nil
		p.artifactCursor++
		p.statisticsErr = nil
		p.hidden = map[string]bool{}
		p.revealed = map[string]protocol.ConversationEntry{}
		return Applied, nil
	}
	if update.Kind != "conversation_frame" {
		return Ignored, fault(protocol.ErrorCodeInvalidRequest, "unexpected conversation update")
	}
	f, err := payload[protocol.ConversationFrame](update.Data)
	if err != nil {
		return Ignored, err
	}
	if f.Node != p.node || f.Session != p.session {
		return Ignored, fault(protocol.ErrorCodeWrongTarget, "conversation frame belongs to another resource")
	}
	if p.snapshot == nil {
		p.recovering = true
		return Recover, nil
	}
	if f.Sequence <= p.snapshot.Sequence {
		return Ignored, nil
	}
	if p.recovering || p.snapshot.Sequence == ^uint64(0) || f.Sequence != p.snapshot.Sequence+1 {
		p.recovering = true
		return Recover, nil
	}
	// Apply atomically without copying the loaded conversation: every check of a
	// change precedes its first write, so an invalid frame changes nothing. The
	// retained snapshot never leaves this projection; readers receive copies.
	f = copyValue(f)
	v := p.snapshot
	page := &v.Page
	switch f.Change.Kind {
	case "entry", "revealed":
		x, e := payload[protocol.ConversationEntry](f.Change.Data)
		if e != nil {
			return Ignored, e
		}
		if x.Sequence == 0 || x.ID == "" {
			return Ignored, fault(protocol.ErrorCodeInvalidRequest, "invalid canonical entry identity")
		}
		if i := slices.IndexFunc(page.Entries, func(v protocol.ConversationEntry) bool { return v.ID == x.ID }); i >= 0 {
			if !reflect.DeepEqual(page.Entries[i], x) {
				return Ignored, fault(protocol.ErrorCodeInvalidRequest, "canonical entry identity changed")
			}
		} else {
			if f.Change.Kind == "revealed" {
				p.revealed[x.ID] = x
				delete(p.hidden, x.ID)
				break
			}
			if f.Change.Kind == "entry" && len(page.Entries) > 0 && x.Sequence <= page.Entries[len(page.Entries)-1].Sequence {
				return Ignored, fault(protocol.ErrorCodeInvalidRequest, "conversation history order changed")
			}
			page.Entries = append(page.Entries, x)
			slices.SortFunc(page.Entries, func(a, b protocol.ConversationEntry) int { return cmp.Compare(a.Sequence, b.Sequence) })
			if p.liveStart == nil {
				p.liveStart = new(x.Sequence)
			}
		}
		delete(p.hidden, x.ID)
		v.Drafts = slices.DeleteFunc(v.Drafts, func(d protocol.ConversationDraft) bool { return d.Turn == x.Turn && d.ID == x.ID })
		if x.Author == "user" {
			for i := range page.Runs {
				if page.Runs[i].Turn == x.Turn {
					page.Runs[i].Input = nil
				}
			}
		}
	case "hidden":
		x, e := payload[protocol.ConversationChangeHiddenPayload](f.Change.Data)
		if e != nil {
			return Ignored, e
		}
		p.hidden[x.ID] = true
		delete(p.revealed, x.ID)
		page.Entries = slices.DeleteFunc(page.Entries, func(e protocol.ConversationEntry) bool { return e.ID == x.ID && e.Turn == x.Turn })
		v.Drafts = slices.DeleteFunc(v.Drafts, func(d protocol.ConversationDraft) bool { return d.ID == x.ID && d.Turn == x.Turn })
	case "run":
		x, e := payload[protocol.ConversationRun](f.Change.Data)
		if e != nil {
			return Ignored, e
		}
		if !validConversationRun(x, p.session) {
			return Ignored, fault(protocol.ErrorCodeWrongTarget, "run belongs to another conversation")
		}
		if slices.ContainsFunc(page.Entries, func(e protocol.ConversationEntry) bool { return e.Turn == x.Turn && e.Author == "user" }) {
			x.Input = nil
		}
		if i := slices.IndexFunc(page.Runs, func(v protocol.ConversationRun) bool { return v.Turn == x.Turn }); i >= 0 {
			if !sameRun(page.Runs[i], x) {
				return Ignored, fault(protocol.ErrorCodeInvalidRequest, "sent turn identity changed")
			}
			page.Runs[i] = x
		} else {
			if slices.ContainsFunc(page.Runs, func(r protocol.ConversationRun) bool { return r.Sequence == x.Sequence }) {
				return Ignored, fault(protocol.ErrorCodeInvalidRequest, "turn order repeats a sequence")
			}
			page.Runs = append(page.Runs, x)
		}
		slices.SortFunc(page.Runs, func(a, b protocol.ConversationRun) int { return cmp.Compare(a.Sequence, b.Sequence) })
		if !activeConversationRun(x.Status) {
			v.Drafts = slices.DeleteFunc(v.Drafts, func(d protocol.ConversationDraft) bool { return d.Turn == x.Turn })
		}
	case "delta":
		x, e := payload[protocol.ConversationDraft](f.Change.Data)
		if e != nil {
			return Ignored, e
		}
		if err := validateConversationDraft(*page, x); err != nil {
			return Ignored, err
		}
		if i := slices.IndexFunc(v.Drafts, func(d protocol.ConversationDraft) bool { return d.ID == x.ID && d.Turn == x.Turn }); i >= 0 {
			d := &v.Drafts[i]
			if d.Author != x.Author || d.Branch != x.Branch {
				return Ignored, fault(protocol.ErrorCodeInvalidRequest, "streaming identity changed")
			}
			parts := slices.Clone(d.Parts)
			for _, part := range x.Parts {
				if n := len(parts); n > 0 && (part.Kind == "text" || part.Kind == "thinking") && parts[n-1].Kind == part.Kind {
					a, ae := payload[string](parts[n-1].Data)
					b, be := payload[string](part.Data)
					if ae != nil || be != nil {
						return Ignored, fault(protocol.ErrorCodeInvalidRequest, "invalid streaming text")
					}
					parts[n-1].Data = a + b
				} else {
					parts = append(parts, part)
				}
			}
			d.Parts = parts
		} else {
			v.Drafts = append(v.Drafts, x)
		}
	case "queue":
		x, e := payload[protocol.ConversationQueue](f.Change.Data)
		if e != nil {
			return Ignored, e
		}
		if x.Revision < page.Queue.Revision || x.Revision == page.Queue.Revision && !reflect.DeepEqual(x, page.Queue) {
			return Ignored, fault(protocol.ErrorCodeInvalidRequest, "queue revision did not advance")
		}
		if err := validateConversationQueue(x); err != nil {
			return Ignored, err
		}
		for _, item := range x.Items {
			if i := slices.IndexFunc(page.Queue.Items, func(v protocol.ConversationPending) bool { return v.Turn == item.Turn }); i >= 0 {
				previous := page.Queue.Items[i]
				if previous.ConfigRevision != item.ConfigRevision || previous.Kind != item.Kind || previous.Notice != item.Notice || previous.Revision > item.Revision || previous.Revision == item.Revision && (!reflect.DeepEqual(previous.Request, item.Request) || previous.Preview != item.Preview || previous.Truncated != item.Truncated) {
					return Ignored, fault(protocol.ErrorCodeInvalidRequest, "pending input changed without its revision")
				}
			}
		}
		page.Queue = x
	case "approval":
		x, e := payload[protocol.ConversationApproval](f.Change.Data)
		if e != nil {
			return Ignored, e
		}
		if x.Session != p.session {
			return Ignored, fault(protocol.ErrorCodeWrongTarget, "approval belongs to another conversation")
		}
		if err := validateConversationApproval(*page, x); err != nil {
			return Ignored, err
		}
		if i := slices.IndexFunc(page.Approvals, func(a protocol.ConversationApproval) bool { return a.ID == x.ID }); i >= 0 {
			if err := checkApprovalTransition(page.Approvals[i], x, false); err != nil {
				return Ignored, err
			}
		}
		page.Approvals = upsertConversation(page.Approvals, x, func(v protocol.ConversationApproval) protocol.ApprovalID { return v.ID }, false)
	case "question":
		x, e := payload[protocol.ConversationQuestion](f.Change.Data)
		if e != nil {
			return Ignored, e
		}
		if x.Session != p.session {
			return Ignored, fault(protocol.ErrorCodeWrongTarget, "question belongs to another conversation")
		}
		if err := validateConversationQuestion(*page, x); err != nil {
			return Ignored, err
		}
		if i := slices.IndexFunc(page.Questions, func(q protocol.ConversationQuestion) bool { return q.ID == x.ID }); i >= 0 {
			if err := checkQuestionTransition(page.Questions[i], x, false); err != nil {
				return Ignored, err
			}
		} else if slices.ContainsFunc(page.Questions, func(q protocol.ConversationQuestion) bool {
			return q.Turn == x.Turn && q.Entry == x.Entry && q.Index == x.Index
		}) {
			return Ignored, fault(protocol.ErrorCodeInvalidRequest, "tool call has multiple questions")
		}
		page.Questions = upsertConversation(page.Questions, x, func(v protocol.ConversationQuestion) protocol.QuestionID { return v.ID }, false)
	case "child":
		x, e := payload[protocol.ConversationChild](f.Change.Data)
		if e != nil {
			return Ignored, e
		}
		if err := mergeConversationChild(page, x, false); err != nil {
			return Ignored, err
		}
	case "background":
		x, e := payload[protocol.ConversationBackgroundTask](f.Change.Data)
		if e != nil {
			return Ignored, e
		}
		if err := mergeConversationBackground(page, x); err != nil {
			return Ignored, err
		}
	case "statistics":
		x, e := payload[protocol.ConversationStatistics](f.Change.Data)
		if e != nil {
			return Ignored, e
		}
		v.Statistics = &x
		p.statisticsErr = nil
	case "statistics_failed":
		x, e := payload[protocol.Fault](f.Change.Data)
		if e != nil {
			return Ignored, e
		}
		v.Statistics = nil
		p.statisticsErr = &x
	case "artifact":
		x, e := payload[protocol.ArtifactSummary](f.Change.Data)
		if e != nil {
			return Ignored, e
		}
		if p.artifactCursor == ^uint64(0) {
			return Ignored, fault(protocol.ErrorCodeInvalidRequest, "artifact cursor exhausted")
		}
		v.Artifacts.Items = upsertConversation(v.Artifacts.Items, x, func(v protocol.ArtifactSummary) protocol.ArtifactID { return v.ID }, false)
		p.artifactCursor++
	default:
		// A change a newer Node declares belongs to a part of the
		// conversation this build does not hold; the frame still takes its
		// place in the sequence.
		if _, undeclared := f.Change.Data.(protocol.UnknownVariant); !undeclared {
			return Ignored, fault(protocol.ErrorCodeInvalidRequest, "unknown conversation frame change")
		}
	}
	v.Sequence = f.Sequence
	p.settle()
	return Applied, nil
}

func activeConversationRun(s protocol.ConversationStatus) bool {
	return s == "queued" || s == "running" || s == "stopping"
}

func sameRun(a, b protocol.ConversationRun) bool {
	return a.Turn == b.Turn && a.Session == b.Session && a.Revision == b.Revision && a.Sequence == b.Sequence && a.Kind == b.Kind && a.Notice == b.Notice && a.Worktree == b.Worktree && reflect.DeepEqual(a.Origin, b.Origin) && (a.Input == nil || b.Input == nil || reflect.DeepEqual(a.Input, b.Input))
}

func validateConversationPage(p protocol.ConversationPage) error {
	if p.Revision == 0 {
		return fault(protocol.ErrorCodeInvalidRequest, "conversation history revision is missing")
	}
	ids := map[string]bool{}
	turns := map[protocol.TurnID]bool{}
	for i, e := range p.Entries {
		if e.Sequence == 0 || e.ID == "" || ids[e.ID] || i > 0 && p.Entries[i-1].Sequence >= e.Sequence {
			return fault(protocol.ErrorCodeInvalidRequest, "conversation canonical order is inconsistent")
		}
		ids[e.ID] = true
	}
	for i, r := range p.Runs {
		if !validConversationRun(r, p.Session) || turns[r.Turn] || i > 0 && p.Runs[i-1].Sequence >= r.Sequence {
			return fault(protocol.ErrorCodeInvalidRequest, "conversation run order is inconsistent")
		}
		turns[r.Turn] = true
	}
	if p.NextBefore != nil && !turns[*p.NextBefore] {
		return fault(protocol.ErrorCodeInvalidRequest, "history cursor references an unknown turn")
	}
	approvals := map[protocol.ApprovalID]bool{}
	for _, a := range p.Approvals {
		if a.Session != p.Session {
			return fault(protocol.ErrorCodeWrongTarget, "approval belongs to another conversation")
		}
		if approvals[a.ID] {
			return fault(protocol.ErrorCodeInvalidRequest, "conversation repeats an approval")
		}
		approvals[a.ID] = true
		if err := validateConversationApproval(p, a); err != nil {
			return err
		}
	}
	questions := map[protocol.QuestionID]bool{}
	calls := map[string]bool{}
	for _, q := range p.Questions {
		if q.Session != p.Session {
			return fault(protocol.ErrorCodeWrongTarget, "question belongs to another conversation")
		}
		key := q.Turn.String() + "/" + q.Entry + "/" + strconv.FormatUint(q.Index, 10)
		if questions[q.ID] || calls[key] {
			return fault(protocol.ErrorCodeInvalidRequest, "conversation repeats a question")
		}
		questions[q.ID], calls[key] = true, true
		if err := validateConversationQuestion(p, q); err != nil {
			return err
		}
	}
	if err := validateConversationQueue(p.Queue); err != nil {
		return err
	}
	// The merges below write only the list they rebuild.
	children := p
	children.Children = nil
	for _, child := range p.Children {
		if err := mergeConversationChild(&children, child, false); err != nil {
			return err
		}
	}
	background := p
	background.Background = nil
	for _, task := range p.Background {
		if slices.ContainsFunc(background.Background, func(t protocol.ConversationBackgroundTask) bool { return t.ID == task.ID }) {
			return fault(protocol.ErrorCodeInvalidRequest, "background task identity is repeated")
		}
		if err := mergeConversationBackground(&background, task); err != nil {
			return err
		}
	}
	return nil
}

func upsertConversation[T any, K comparable](list []T, value T, key func(T) K, keep bool) []T {
	if i := slices.IndexFunc(list, func(v T) bool { return key(v) == key(value) }); i >= 0 {
		if !keep {
			list[i] = value
		}
		return list
	}
	return append(list, value)
}
