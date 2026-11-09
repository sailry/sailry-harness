package client

import (
	"context"
	"math"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// SearchConversation fills one logical result page across bounded Node scans.
func (c *Client) SearchConversation(ctx context.Context, session protocol.SessionID, query protocol.ConversationSearchQuery) (protocol.ConversationSearchPage, error) {
	result := protocol.ConversationSearchPage{Session: session}
	if query.Limit == 0 || query.Limit > 100 {
		return result, fault(protocol.ErrorCodeInvalidRequest, "conversation search result limit is invalid")
	}
	limit := int(query.Limit)
	identities := map[string]bool{}
	for {
		query.Limit = uint16(limit - len(result.Matches))
		out, err := c.Do(ctx, protocol.Command{Kind: "search_conversation", Data: protocol.CommandSearchConversationPayload{Session: session, Query: query}})
		if err != nil {
			return result, err
		}
		page, err := payload[protocol.ConversationSearchPage](out.Data)
		if err != nil || out.Kind != "conversation_matches" {
			return result, fault(protocol.ErrorCodeInvalidRequest, "conversation search response expected")
		}
		if err := validateConversationSearch(page, session, query); err != nil {
			return result, err
		}
		if result.Revision != 0 && result.Revision != page.Revision {
			return result, fault(protocol.ErrorCodeRevisionConflict, "conversation history changed during search")
		}
		for _, found := range page.Matches {
			if identities[found.Entry] {
				return result, fault(protocol.ErrorCodeInvalidRequest, "conversation search repeated an entry across scans")
			}
			identities[found.Entry] = true
		}
		result.Revision = page.Revision
		result.Matches = append(result.Matches, page.Matches...)
		result.NextBefore = page.NextBefore
		if len(result.Matches) == limit || page.NextBefore == nil {
			return result, nil
		}
		query.Before = page.NextBefore
	}
}

func validateConversationSearch(page protocol.ConversationSearchPage, session protocol.SessionID, query protocol.ConversationSearchQuery) error {
	if page.Revision == 0 || page.Session != session || len(page.Matches) > int(query.Limit) {
		return fault(protocol.ErrorCodeInvalidRequest, "search response belongs to another read or exceeds its limit")
	}
	before := uint64(math.MaxInt64)
	if query.Before != nil {
		before = *query.Before
	}
	initial := before
	ids := map[string]bool{}
	for _, found := range page.Matches {
		start, end := found.Highlight.Start, found.Highlight.End
		if found.Sequence == 0 || found.Sequence >= before || found.TurnSequence == 0 || found.Entry == "" || ids[found.Entry] || len(found.Snippet) > 4096 || start >= end || end > uint64(len(found.Snippet)) || !utf8.ValidString(found.Snippet[:start]) || !utf8.ValidString(found.Snippet[start:end]) {
			return fault(protocol.ErrorCodeInvalidRequest, "conversation search match is invalid")
		}
		before = found.Sequence
		ids[found.Entry] = true
	}
	if page.NextBefore != nil && (*page.NextBefore == 0 || *page.NextBefore > before || *page.NextBefore >= initial) {
		return fault(protocol.ErrorCodeInvalidRequest, "conversation search cursor did not advance")
	}
	return nil
}

// RevealConversation loads through an exact search target, including any older
// continuation of its turn. Observation owns the resulting canonical projection.
func (c *Client) RevealConversation(ctx context.Context, session protocol.SessionID, revision uint64, found protocol.ConversationSearchMatch) error {
	for {
		c.conversationMu.Lock()
		w := c.conversations[session]
		if w == nil || w.projection.snapshot == nil || w.state.Removed {
			c.conversationMu.Unlock()
			return fault(protocol.ErrorCodeUnavailable, "conversation is not loaded")
		}
		snapshot := w.projection.snapshot
		if snapshot.Page.Revision != revision {
			c.conversationMu.Unlock()
			return fault(protocol.ErrorCodeRevisionConflict, "conversation changed during search reveal")
		}
		for _, entry := range snapshot.Page.Entries {
			if entry.ID == found.Entry && entry.Turn == found.Turn {
				c.conversationMu.Unlock()
				return nil
			}
		}
		more := snapshot.Page.NextBefore != nil || len(snapshot.Missing) > 0
		loading, signal := w.state.LoadingOlder, w.signal
		c.conversationMu.Unlock()
		if !more {
			return fault(protocol.ErrorCodeNotFound, "search target is no longer visible")
		}
		if loading {
			select {
			case <-ctx.Done():
				return ctx.Err()
			case <-w.ctx.Done():
				return w.ctx.Err()
			case <-signal:
			}
			continue
		}
		if err := c.LoadOlderConversation(ctx, session); err != nil {
			return err
		}
	}
}
