package client

import (
	"context"
	"encoding/json"
	"reflect"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const conversationDetailLimit = 64 * 1024 * 1024

type conversationDetail struct {
	reference protocol.ToolDetailReference
	value     protocol.ToolDetail
	bytes     uint64
}

type conversationDetailRead struct {
	done  chan struct{}
	value protocol.ToolDetail
	err   error
}

func detailKey(ref protocol.ToolDetailReference) string { b, _ := json.Marshal(ref); return string(b) }

func (c *Client) ConversationDetail(ref protocol.ToolDetailReference) (protocol.ToolDetail, bool) {
	c.conversationMu.Lock()
	defer c.conversationMu.Unlock()
	v, ok := c.conversationDetails[detailKey(ref)]
	if !ok || !c.currentConversationDetail(ref) {
		return protocol.ToolDetail{}, false
	}
	return copyValue(v.value), true
}

// ReadConversationDetail validates bounded pages against the captured revision.
func (c *Client) ReadConversationDetail(ctx context.Context, ref protocol.ToolDetailReference) (protocol.ToolDetail, error) {
	if v, ok := c.ConversationDetail(ref); ok {
		return v, nil
	}
	key := detailKey(ref)
	c.conversationMu.Lock()
	if pending := c.conversationDetailReads[key]; pending != nil {
		c.conversationMu.Unlock()
		select {
		case <-ctx.Done():
			return protocol.ToolDetail{}, ctx.Err()
		case <-pending.done:
			return copyValue(pending.value), pending.err
		}
	}
	if c.conversationDetailReads == nil {
		c.conversationDetailReads = map[string]*conversationDetailRead{}
	}
	pending := &conversationDetailRead{done: make(chan struct{})}
	c.conversationDetailReads[key] = pending
	c.conversationMu.Unlock()
	value, err := c.readConversationDetail(ctx, ref)
	c.conversationMu.Lock()
	pending.value, pending.err = copyValue(value), err
	delete(c.conversationDetailReads, key)
	close(pending.done)
	c.conversationMu.Unlock()
	return value, err
}

func (c *Client) readConversationDetail(ctx context.Context, ref protocol.ToolDetailReference) (protocol.ToolDetail, error) {
	var data []byte
	var total *uint64
	for {
		var cursor *uint64
		if len(data) > 0 {
			offset := uint64(len(data))
			cursor = &offset
		}
		out, err := c.Do(ctx, protocol.Command{Kind: "read_tool_detail", Data: protocol.CommandReadToolDetailPayload{Session: ref.Session, Turn: ref.Turn, Entry: ref.Entry, Index: ref.Index, Call: ref.Call, ExpectedRevision: ref.Revision, Cursor: cursor, Limit: 64 * 1024}})
		if err != nil {
			return protocol.ToolDetail{}, err
		}
		p, err := payload[protocol.ToolDetailPage](out.Data)
		if err != nil || out.Kind != "tool_detail" {
			return protocol.ToolDetail{}, fault(protocol.ErrorCodeInvalidRequest, "tool detail page expected")
		}
		end := p.Offset + uint64(len(p.Text))
		if p.TotalBytes > conversationDetailLimit {
			return protocol.ToolDetail{}, fault(protocol.ErrorCodeInvalidRequest, "tool detail exceeds the transfer limit")
		}
		if !reflect.DeepEqual(ref, p.Reference) || p.Offset != uint64(len(data)) || len(p.Text) > 64*1024 || end > p.TotalBytes || total != nil && *total != p.TotalBytes || p.NextOffset != nil && (*p.NextOffset != end || len(p.Text) == 0 || end >= p.TotalBytes) || p.NextOffset == nil && end != p.TotalBytes {
			return protocol.ToolDetail{}, fault(protocol.ErrorCodeInvalidRequest, "tool detail page is inconsistent")
		}
		total = &p.TotalBytes
		data = append(data, p.Text...)
		if p.NextOffset == nil {
			break
		}
	}
	var value protocol.ToolDetail
	if json.Unmarshal(data, &value) != nil {
		return value, fault(protocol.ErrorCodeInvalidRequest, "tool detail JSON is invalid")
	}
	allocation := retainedDetailBytes(reflect.ValueOf(value)) + retainedDetailBytes(reflect.ValueOf(ref))
	if allocation > conversationDetailLimit {
		return protocol.ToolDetail{}, fault(protocol.ErrorCodeInvalidRequest, "tool detail exceeds the display memory budget")
	}
	key := detailKey(ref)
	c.conversationMu.Lock()
	if !c.currentConversationDetail(ref) {
		c.conversationMu.Unlock()
		return protocol.ToolDetail{}, fault(protocol.ErrorCodeRevisionConflict, "tool detail changed during transfer")
	}
	if c.conversationDetails == nil {
		c.conversationDetails = map[string]conversationDetail{}
	}
	if old, exists := c.conversationDetails[key]; exists {
		c.conversationDetailBytes -= old.bytes
		delete(c.conversationDetails, key)
		c.conversationDetailOrder = slices.DeleteFunc(c.conversationDetailOrder, func(s string) bool { return s == key })
	}
	for (c.conversationDetailBytes+allocation > conversationDetailLimit || len(c.conversationDetailOrder) >= 64) && len(c.conversationDetailOrder) > 0 {
		old := c.conversationDetailOrder[0]
		c.conversationDetailOrder = c.conversationDetailOrder[1:]
		c.conversationDetailBytes -= c.conversationDetails[old].bytes
		delete(c.conversationDetails, old)
	}
	c.conversationDetailOrder = append(c.conversationDetailOrder, key)
	c.conversationDetails[key] = conversationDetail{ref, copyValue(value), allocation}
	c.conversationDetailBytes += allocation
	w := c.conversations[ref.Session]
	c.conversationMu.Unlock()
	if w != nil {
		c.conversationChanged(w)
	}
	return value, nil
}

// conversationMu is held by the caller. Cached details are scoped to the
// currently retained canonical card, including its revision and optional call ID.
func (c *Client) currentConversationDetail(ref protocol.ToolDetailReference) bool {
	w := c.conversations[ref.Session]
	if w == nil || w.state.Removed || w.projection.snapshot == nil {
		return false
	}
	for _, entry := range w.projection.snapshot.Page.Entries {
		if entry.ID != ref.Entry || entry.Turn != ref.Turn || ref.Index >= uint64(len(entry.Parts)) {
			continue
		}
		part := entry.Parts[ref.Index]
		var id *string
		var summary *protocol.ToolDetailSummary
		switch part.Kind {
		case "tool_call":
			if p, err := payload[protocol.ConversationPartToolCallPayload](part.Data); err == nil {
				id, summary = p.ID, p.Summary
			}
		case "tool_result":
			if p, err := payload[protocol.ConversationPartToolResultPayload](part.Data); err == nil {
				id, summary = p.ID, p.Summary
			}
		}
		return summary != nil && summary.HasDetails && summary.Revision == ref.Revision && reflect.DeepEqual(id, ref.Call)
	}
	return false
}

func (c *Client) pruneConversationDetails() {
	for key, value := range c.conversationDetails {
		if !c.currentConversationDetail(value.reference) {
			c.conversationDetailBytes -= value.bytes
			delete(c.conversationDetails, key)
		}
	}
	c.conversationDetailOrder = slices.DeleteFunc(c.conversationDetailOrder, func(key string) bool { _, ok := c.conversationDetails[key]; return !ok })
}

func retainedDetailBytes(value reflect.Value) uint64 {
	if !value.IsValid() {
		return 0
	}
	base := uint64(value.Type().Size())
	switch value.Kind() {
	case reflect.String:
		return base + uint64(value.Len())
	case reflect.Pointer:
		if !value.IsNil() {
			return base + retainedDetailBytes(value.Elem())
		}
	case reflect.Interface:
		if !value.IsNil() {
			return base + retainedDetailBytes(value.Elem())
		}
	case reflect.Struct:
		for i := 0; i < value.NumField(); i++ {
			base += retainedDetailBytes(value.Field(i)) - uint64(value.Field(i).Type().Size())
		}
	case reflect.Slice:
		base += uint64(value.Cap()) * uint64(value.Type().Elem().Size())
		for i := 0; i < value.Len(); i++ {
			base += retainedDetailBytes(value.Index(i)) - uint64(value.Type().Elem().Size())
		}
	case reflect.Array:
		for i := 0; i < value.Len(); i++ {
			base += retainedDetailBytes(value.Index(i)) - uint64(value.Type().Elem().Size())
		}
	case reflect.Map:
		base += 64
		iter := value.MapRange()
		for iter.Next() {
			base += 32 + 2*(retainedDetailBytes(iter.Key())+retainedDetailBytes(iter.Value()))
		}
	}
	return base
}
