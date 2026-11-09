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
func System(instruction string, context canonical.Context, capability *string) string {
	return strings.Join(SystemParts(instruction, context, capability), "\n\n")
}

// SystemParts keeps provenance on the same fragments used by serialization.
func SystemParts(instruction string, context canonical.Context, capability *string) []string {
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
	if capability != nil {
		parts = append(parts, *capability)
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
func visible(record canonical.Record) bool {
	return record.Metadata.Runtime == nil || record.Metadata.Runtime.AgentVisible
}
func contextIdentity(record canonical.Record) *canonical.PromptContext {
	runtime := record.Metadata.Runtime
	if runtime == nil || !runtime.TurnContext || !runtime.AgentVisible || runtime.Notes == nil {
		return nil
	}
	return runtime.Notes.Context
}
func recordMessage(record canonical.Record) *canonical.Message {
	if record.Kind != "message" {
		return nil
	}
	return record.Message()
}

// Pending repeats ordinary state once per task or when changed. Retained owner
// parts repeat only after their earlier context is no longer model-visible.
func Pending(records []canonical.Record, owned, parts []string) []string {
	start := 0
	for i, record := range records {
		if record.Kind == "compaction" {
			start = i + 1
		}
	}
	records = records[start:]
	retained := map[string]bool{}
	kickoff := -1
	fallback := -1
	for i, record := range records {
		if !visible(record) {
			continue
		}
		if identity := contextIdentity(record); identity != nil {
			for _, id := range identity.Retained {
				retained[id] = true
			}
		}
		message := recordMessage(record)
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
