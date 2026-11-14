// Package prompt separates stable instructions from persisted turn context.
package prompt

import (
	"embed"
	"encoding/binary"
	"fmt"
	"slices"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

//go:embed text/*.txt
var texts embed.FS

func text(name string) string {
	value, err := texts.ReadFile("text/" + name + ".txt")
	if err != nil {
		panic(err)
	}
	return strings.TrimSpace(string(value))
}

func Instruction(child bool) string {
	recipient := "parent"
	if child {
		recipient = "child"
	}
	return text("baseline") + "\n\n" + text(recipient)
}
func Policy(mode protocol.WorkMode, permission protocol.Permission, child bool) string {
	if mode == protocol.WorkModePlan {
		if child {
			return text("plan-child")
		}
		return text("plan")
	}
	switch permission {
	case protocol.PermissionAsk:
		return text("ask")
	case protocol.PermissionProject:
		return text("project")
	case protocol.PermissionFull:
		return text("full")
	}
	panic("invalid captured permission")
}
func Workflow(mode protocol.WorkMode, child bool) string {
	instruction := text("direct")
	if child {
		instruction = "Complete the delegated scope and report to the parent. Delegated children cannot create or control other agents."
	} else if mode == protocol.WorkModeFlagship {
		instruction = "Active work mode: Flagship\n" + text("flagship")
	}
	return "Work mode\n" + instruction + "\nMode instructions do not grant tool authority. Follow the captured mode and tool permissions, including read-only restrictions inherited by delegated tasks."
}

// System never includes current environment, time, turn title or user input.
func System(instruction string, context canonical.Context) string {
	return strings.Join(SystemParts(instruction, context), "\n\n")
}

// SystemParts keeps provenance on the same fragments used by serialization.
func SystemParts(instruction string, context canonical.Context) []string {
	parts := []string{instruction}
	for _, value := range []*string{context.Policy, context.Personal} {
		if value != nil && *value != "" {
			parts = append(parts, *value)
		}
	}
	if len(context.Guides) > 0 {
		parts = append(parts, text("guides"))
		keys := make([]string, 0, len(context.Guides))
		for key := range context.Guides {
			keys = append(keys, key)
		}
		slices.Sort(keys)
		for _, key := range keys {
			parts = append(parts, context.Guides[key].Content)
		}
	}
	return parts
}

func Capture(parts, owned []string) canonical.PromptContext {
	state := blake3.New(32, nil)
	retained := []string{}
	for _, part := range parts {
		if slices.Contains(owned, part) {
			retained = append(retained, digest(part))
		} else {
			var size [8]byte
			binary.LittleEndian.PutUint64(size[:], uint64(len(part)))
			state.Write(size[:])
			state.Write([]byte(part))
		}
	}
	return canonical.PromptContext{State: fmt.Sprintf("%x", state.Sum(nil)), Retained: retained}
}
func digest(value string) string {
	sum := blake3.Sum256([]byte(value))
	return fmt.Sprintf("%x", sum[:])
}
func contextIdentity(record canonical.Record) *canonical.PromptContext {
	runtime := record.Metadata.Runtime
	if runtime == nil || !runtime.TurnContext || !runtime.AgentVisible || runtime.Notes == nil {
		return nil
	}
	return runtime.Notes.Context
}

// Pending repeats ordinary state once per task or when changed. Retained owner
// parts repeat only after their earlier context is no longer model-visible.
func Pending(records []canonical.Record, owned, parts []string) []string {
	start := 0
	summary := -1
	boundary := ""
	visibility := map[string]bool{}
	for i, record := range records {
		if record.Kind == "compaction" {
			start, summary, boundary = i+1, i, ""
			switch value := record.Data.(type) {
			case *canonical.Compaction:
				boundary = value.Boundary
			case canonical.Compaction:
				boundary = value.Boundary
			}
		}
		if record.Kind == "operation" {
			var operation *canonical.Operation
			switch value := record.Data.(type) {
			case *canonical.Operation:
				operation = value
			case canonical.Operation:
				operation = &value
			}
			if operation != nil && operation.Kind == "visibility" {
				switch change := operation.Data.(type) {
				case *canonical.Visibility:
					visibility[change.Message] = change.AgentVisible
				case canonical.Visibility:
					visibility[change.Message] = change.AgentVisible
				}
			}
		}
	}
	// Rolling summaries are appended after the retained tail. Its context
	// remains present until the summary's boundary actually covers it.
	for i := 0; i < summary; i++ {
		if records[i].ID == boundary {
			start = i + 1
			break
		}
	}
	// Identity matching needs only metadata; tool payloads stay shared and are
	// never copied or changed by this projection.
	current := make([]canonical.Record, 0, len(records)-start)
	for _, record := range records[start:] {
		if record.Kind != "message" {
			continue
		}
		visible := record.Metadata.Runtime == nil || record.Metadata.Runtime.AgentVisible
		if override, ok := visibility[record.ID]; ok {
			visible = override
		}
		if !visible {
			continue
		}
		if record.Metadata.Runtime != nil && !record.Metadata.Runtime.AgentVisible {
			runtime := *record.Metadata.Runtime
			runtime.AgentVisible = true
			record.Metadata.Runtime = &runtime
		}
		current = append(current, record)
	}
	records = current
	retained := map[string]bool{}
	kickoff := -1
	fallback := -1
	for i, record := range records {
		if identity := contextIdentity(record); identity != nil {
			for _, id := range identity.Retained {
				retained[id] = true
			}
		}
		message := record.Message()
		if message == nil || message.Role != canonical.User {
			continue
		}
		tool := false
		for _, block := range message.Content {
			if block.Kind == "tool_response" {
				tool = true
				break
			}
		}
		if tool {
			continue
		}
		fallback = i
		if record.Metadata.Runtime == nil || record.Metadata.Runtime.UserVisible {
			kickoff = i
		}
	}
	if kickoff < 0 {
		kickoff = fallback
	}
	var last *canonical.PromptContext
	if kickoff >= 0 {
		for _, record := range records[kickoff:] {
			if identity := contextIdentity(record); identity != nil {
				last = identity
			}
		}
	}
	pending := make([]string, 0, len(parts))
	for _, part := range parts {
		if slices.Contains(owned, part) {
			id := digest(part)
			if retained[id] {
				continue
			}
			retained[id] = true
		}
		pending = append(pending, part)
	}
	if len(pending) == 0 {
		return pending
	}
	identity := Capture(pending, owned)
	if last != nil && last.State == identity.State && len(identity.Retained) == 0 {
		return nil
	}
	return pending
}

func ContextMessage(turn protocol.TurnID, parts, owned []string, now time.Time) *canonical.Record {
	if len(parts) == 0 {
		return nil
	}
	message := canonical.NewMessage(canonical.User, canonical.Text(0, "<turn-context>\n"+strings.Join(parts, "\n\n")+"\n</turn-context>"))
	record := canonical.NewRecord(turn, "message", message, now)
	record.ID = "msg_" + protocol.NewRequestID().String()
	record.Provenance.Author = "assistant"
	created := now.Unix()
	runtime := canonical.DefaultRuntime()
	runtime.Created = &created
	runtime.UserVisible = false
	runtime.TurnContext = true
	identity := Capture(parts, owned)
	runtime.Notes = &canonical.OperationNotes{Context: &identity}
	record.Metadata.Runtime = &runtime
	return &record
}
