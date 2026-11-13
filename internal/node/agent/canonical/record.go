// Package canonical owns the durable conversation representation independently
// of the execution engine and provider transport.
package canonical

import (
	"encoding/json"
	"fmt"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const ExecutableKey = "sailry.executable"
const ExternalDispatchKey = "sailry.external_dispatch"

type Record struct {
	ID         string          `json:"id"`
	Created    time.Time       `json:"created"`
	Turn       protocol.TurnID `json:"turn"`
	Provenance Provenance      `json:"provenance"`
	Metadata   Metadata        `json:"metadata"`
	Kind       string          `json:"-"`
	Data       any             `json:"-"`
}
type Provenance struct {
	Author string `json:"author"`
	Branch string `json:"branch"`
}
type Message struct {
	Role      Role           `json:"role"`
	PartCount uint64         `json:"part_count"`
	Content   []Block        `json:"content,omitempty"`
	Model     *string        `json:"model,omitempty"`
	Usage     *Usage         `json:"usage,omitempty"`
	Finish    *Finish        `json:"finish,omitempty"`
	Error     *ResponseError `json:"error,omitempty"`
	Sources   Sources        `json:"sources"`
}
type Role string

const (
	User      Role = "user"
	Assistant Role = "assistant"
)

type Finish string

const (
	Completed   Finish = "completed"
	Interrupted Finish = "interrupted"
	Failed      Finish = "failed"
)

type ResponseError struct {
	Code    *string `json:"code,omitempty"`
	Message string  `json:"message"`
	// Keep the provider code and the recovery outcome independently so a
	// reloaded turn distinguishes exhausted attempts from exhausted waiting.
	RetryExhausted     bool `json:"retry_exhausted,omitempty"`
	RetryWaitExhausted bool `json:"retry_wait_exhausted,omitempty"`
}
type Usage struct {
	Input      *uint64         `json:"input,omitempty"`
	Output     *uint64         `json:"output,omitempty"`
	Total      *uint64         `json:"total,omitempty"`
	CacheRead  *uint64         `json:"cache_read,omitempty"`
	CacheWrite *uint64         `json:"cache_write,omitempty"`
	Reasoning  *uint64         `json:"reasoning,omitempty"`
	Cost       *float64        `json:"cost,omitempty"`
	Provider   json.RawMessage `json:"provider,omitempty"`
}
type Sources struct {
	Citations         []Citation  `json:"citations,omitempty"`
	Grounding         []Grounding `json:"grounding,omitempty"`
	SearchSuggestions *string     `json:"search_suggestions,omitempty"`
}
type Citation struct {
	URI   *string `json:"uri,omitempty"`
	Title *string `json:"title,omitempty"`
	Start *int64  `json:"start,omitempty"`
	End   *int64  `json:"end,omitempty"`
}
type Grounding struct {
	URI   string  `json:"uri"`
	Title *string `json:"title,omitempty"`
}
type Compaction struct {
	Boundary string          `json:"boundary"`
	Summary  Message         `json:"summary"`
	Start    time.Time       `json:"start"`
	End      time.Time       `json:"end"`
	Files    []CompactedFile `json:"files,omitempty"`
}
type CompactedFile struct {
	Path     string  `json:"path"`
	Modified *bool   `json:"modified,omitempty"`
	Summary  *string `json:"summary,omitempty"`
}
type Configuration struct {
	Context Context `json:"context"`
}
type ToolIdentity struct {
	ID        *string         `json:"id,omitempty"`
	Name      string          `json:"name"`
	Arguments json.RawMessage `json:"arguments"`
}

type Notice struct {
	Kind string
	Data any
}
type Elicitation struct {
	Parent  ToolIdentity                      `json:"parent"`
	Request json.RawMessage                   `json:"request"`
	Spec    protocol.ConversationQuestionSpec `json:"spec"`
	Schema  json.RawMessage                   `json:"schema"`
}
type ElicitationResult struct {
	Call     string                                `json:"call"`
	Response protocol.ConversationQuestionResponse `json:"response"`
}
type PlanReview struct {
	Source string `json:"source"`
}
type PlanResult = ElicitationResult

type Operation struct {
	Kind string
	Data any
}
type Visibility struct {
	Message      string `json:"message"`
	UserVisible  bool   `json:"user_visible"`
	AgentVisible bool   `json:"agent_visible"`
}
type ToolMeta struct {
	Call  string          `json:"call"`
	Patch json.RawMessage `json:"patch"`
}
type Note struct {
	Message string        `json:"message"`
	Note    OperationNote `json:"note"`
}
type OperationNote struct {
	Kind string
	Data any
}
type SubagentDelivered struct {
	Session string `json:"session"`
}
type SubagentsCancelled struct {
	Sessions []string `json:"sessions"`
}

func NewRecord(turn protocol.TurnID, kind string, data any, now time.Time) Record {
	return Record{ID: protocol.NewRequestID().String(), Created: now.UTC(), Turn: turn, Kind: kind, Data: data}
}
func NewMessage(role Role, blocks ...Block) Message {
	return Message{Role: role, PartCount: uint64(len(blocks)), Content: blocks}
}
func Text(index uint64, text string) Block {
	return Block{Index: index, Kind: "text", Data: &TextContent{Text: text}}
}
func (r *Record) Message() *Message {
	if r.Kind != "message" {
		return nil
	}
	switch v := r.Data.(type) {
	case *Message:
		return v
	case Message:
		r.Data = &v
		return &v
	}
	return nil
}
func (r *Record) MutableMessage() *Message {
	if r.Kind == "compaction" {
		switch v := r.Data.(type) {
		case *Compaction:
			return &v.Summary
		case Compaction:
			r.Data = &v
			return &v.Summary
		}
	}
	return r.Message()
}

func (r Record) Validate() error {
	if r.ID == "" {
		return fmt.Errorf("conversation record identifier is invalid")
	}
	if r.Kind == "compaction" {
		c, ok := r.Data.(*Compaction)
		if !ok {
			v, yes := r.Data.(Compaction)
			if !yes {
				return fmt.Errorf("invalid compaction record")
			}
			c = &v
		}
		if c.Boundary == "" {
			return fmt.Errorf("context summary has no boundary")
		}
	}
	message := r.MutableMessage()
	if message == nil {
		return nil
	}
	seen := map[uint64]bool{}
	for _, b := range message.Content {
		if b.Index >= message.PartCount {
			return fmt.Errorf("conversation content slot is outside its message")
		}
		if seen[b.Index] {
			return fmt.Errorf("conversation content slots are duplicated")
		}
		seen[b.Index] = true
		switch v := b.Data.(type) {
		case *ToolRequest:
			if v.ID == "" {
				return fmt.Errorf("tool call identifier is required")
			}
		case *ToolResponse:
			if v.ID == "" {
				return fmt.Errorf("tool call identifier is required")
			}
			if len(v.Revision) != 64 {
				return fmt.Errorf("tool response is invalid")
			}
		}
	}
	return nil
}
