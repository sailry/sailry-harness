package store

import (
	"context"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// ToolPresentationLookup resolves presentation metadata for assembled native
// module tools (by canonical name or superseded legacy plugin alias).
type ToolPresentationLookup func(name string) (protocol.ToolPresentation, protocol.ToolGrouping, *protocol.ToolDisplay, bool)

type toolPresentationContextKey struct{}

// WithToolPresentation attaches a per-Node native module presentation resolver
// to ctx for canonical record preparation.
func WithToolPresentation(ctx context.Context, lookup ToolPresentationLookup) context.Context {
	if lookup == nil {
		return ctx
	}
	return context.WithValue(ctx, toolPresentationContextKey{}, lookup)
}

func toolPresentationLookup(ctx context.Context) ToolPresentationLookup {
	lookup, _ := ctx.Value(toolPresentationContextKey{}).(ToolPresentationLookup)
	return lookup
}

func prepareCanonicalRecord(ctx context.Context, db *sql.Conn, record *canonical.Record) error {
	message := record.MutableMessage()
	if message == nil {
		return nil
	}
	for _, block := range message.Content {
		if file, ok := block.Data.(*canonical.FileContent); ok && strings.HasPrefix(file.File.URI, "sailry-attachment://") {
			id, err := protocol.ParseAttachmentID(strings.TrimPrefix(file.File.URI, "sailry-attachment://"))
			if err != nil {
				return err
			}
			attachment, err := GetTurnAttachment(ctx, db, record.Turn, id)
			if err != nil {
				return err
			}
			file.File.Mime = &attachment.Spec.MediaType
		}
	}
	catalog, err := operations.Bundled()
	if err != nil {
		return err
	}
	capture, err := readTurnCapture(ctx, db, record.Turn)
	if err != nil {
		return err
	}
	// Installed packages declare their tools in the captured version.
	installed := map[protocol.PluginReference]*protocol.PluginExtension{}
	for _, block := range message.Content {
		name := ""
		switch value := block.Data.(type) {
		case *canonical.ToolRequest:
			name = value.Name
		case *canonical.ToolResponse:
			name = value.Name
		}
		if name == "" {
			continue
		}
		var presentation protocol.ToolPresentation
		var grouping protocol.ToolGrouping
		var display *protocol.ToolDisplay
		declared := false
		if name == operations.UpdatePlanTool {
			presentation, grouping, declared = protocol.ToolPresentationProgress, protocol.ToolGroupingStandalone, true
		} else if name == operations.PresentFilesTool {
			presentation, grouping, declared = protocol.ToolPresentationContent, protocol.ToolGroupingStandalone, true
		} else {
			for _, tool := range catalog.Tools {
				if tool.Name == name && slices.Contains(capture.Plugins, tool.Owner) {
					presentation, grouping, display, declared = tool.Presentation, tool.Grouping, tool.Display, true
					break
				}
			}
		}
		for _, reference := range capture.Plugins {
			if declared || !strings.HasPrefix(name, "plugin_") && !strings.HasPrefix(name, "mcp_") {
				break
			}
			extension, read := installed[reference]
			if !read {
				// A version that is no longer readable keeps the default presentation.
				if info, err := ReadPluginVersion(ctx, db, reference); err == nil {
					extension = info.Extension
				}
				installed[reference] = extension
			}
			if extension == nil {
				continue
			}
			for _, tool := range extension.Tools {
				alias := operations.Alias(reference.Name, tool.Name)
				if tool.Server != nil {
					alias = mcp.Alias(reference.Name, *tool.Server, tool.Name)
				}
				if alias == name {
					presentation, grouping, display, declared = tool.Presentation, tool.Grouping, tool.Display, true
					break
				}
			}
		}
		if !declared && name == operations.LegacyUpdatePlanTool {
			presentation, grouping, declared = protocol.ToolPresentationProgress, protocol.ToolGroupingStandalone, true
		}
		if !declared {
			if lookup := toolPresentationLookup(ctx); lookup != nil {
				if p, g, d, ok := lookup(name); ok {
					presentation, grouping, display, declared = p, g, d, true
				}
			}
		}
		if !declared {
			command := name
			if !operations.IsCommandTool(command) {
				command = operations.LegacyCommandTool(name)
			}
			if command != "" {
				specs, err := operations.CommandSpecs()
				if err != nil {
					return err
				}
				for _, spec := range specs {
					if spec.Name == command {
						presentation, grouping, display, declared = spec.Presentation, protocol.ToolGroupingSequence, &spec.Display, true
						break
					}
				}
			}
		}
		if !declared {
			continue
		}
		if record.Metadata.Presentation == nil {
			record.Metadata.Presentation = &canonical.Presentation{}
		}
		p := record.Metadata.Presentation
		if p.Tools == nil {
			p.Tools = map[string]protocol.ToolPresentation{}
		}
		if p.Groupings == nil {
			p.Groupings = map[string]protocol.ToolGrouping{}
		}
		p.Tools[name] = presentation
		p.Groupings[name] = grouping
		if display != nil {
			if p.Displays == nil {
				p.Displays = map[string]protocol.ToolDisplay{}
			}
			p.Displays[name] = *display
		}
	}
	denials, err := readBodies[protocol.ConversationApproval](ctx, db, "SELECT body FROM agent_approvals WHERE turn=?1 AND json_extract(body,'$.state')='denied' AND json_type(body,'$.reason')='text' ORDER BY rowid", record.Turn.String())
	if err != nil {
		return err
	}
	for _, denial := range denials {
		request, e := approvalRequest(ctx, db, denial)
		if e != nil {
			return e
		}
		for _, block := range message.Content {
			if response, ok := block.Data.(*canonical.ToolResponse); ok && (response.Format == canonical.JSON || response.Format == canonical.MCPError) && response.ID == request.ID && response.Name == request.Name {
				reason := fmt.Sprintf("Tool execution was denied. Reason: %s", *denial.Reason)
				var value any = map[string]string{"error": reason}
				if response.Format == canonical.MCPError {
					// The engine reports a denial as a fault, which carries
					// no reason: the model reads the reason as the call's
					// error result instead.
					value = map[string]any{"content": []map[string]string{{"type": "text", "text": reason}}, "isError": true}
					response.Format = canonical.MCP
				}
				body, e := encode(value)
				if e != nil {
					return e
				}
				sum := blake3.Sum256(body)
				response.Revision = hex.EncodeToString(sum[:])
				response.Result = canonical.Payload{Value: body}
				response.Status = canonical.ToolErrorStatus
			}
		}
	}
	for _, block := range message.Content {
		response, ok := block.Data.(*canonical.ToolResponse)
		if !ok || response.Status != canonical.ToolErrorStatus {
			continue
		}
		if record.Metadata.Presentation == nil {
			record.Metadata.Presentation = &canonical.Presentation{}
		}
		presentation := record.Metadata.Presentation
		exists := false
		for _, v := range presentation.Failures {
			if v.Index == block.Index && v.ID == response.ID && v.Name == response.Name {
				exists = true
				break
			}
		}
		if exists {
			continue
		}
		presentation.Failures = append(presentation.Failures, canonical.ToolFailure{
			Index: block.Index, ID: response.ID, Name: response.Name, Failure: agentToolFailure(response),
		})
	}
	return nil
}

func agentToolFailure(response *canonical.ToolResponse) protocol.ToolFailure {
	switch response.Format {
	case canonical.MCP:
		return protocol.ToolFailure{Origin: protocol.ToolFailureOriginTool}
	case canonical.MCPError:
		var envelope struct {
			Data json.RawMessage `json:"data"`
		}
		var fault protocol.Fault
		if json.Unmarshal(response.Result.Value, &envelope) != nil || json.Unmarshal(envelope.Data, &fault) != nil {
			return protocol.ToolFailure{Origin: protocol.ToolFailureOriginTool}
		}
		failure := protocol.ToolFailure{Origin: protocol.ToolFailureOriginRuntime, Code: &fault.Code}
		switch fault.Code {
		case protocol.ErrorCodeInvalidRequest, protocol.ErrorCodeNotFound, protocol.ErrorCodeConflict, protocol.ErrorCodeRevisionConflict:
			// The call ran and the tool refused what it was asked.
			failure.Origin = protocol.ToolFailureOriginTool
		}
		return failure
	default:
		return protocol.ToolFailure{Origin: protocol.ToolFailureOriginRuntime}
	}
}
