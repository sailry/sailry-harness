package output

import (
	"bytes"
	"encoding/json"
	"maps"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func clonePtr[T any](value *T) *T {
	if value == nil {
		return nil
	}
	copied := *value
	return &copied
}

func cloneOptionalRaw(raw json.RawMessage) json.RawMessage {
	if len(raw) == 0 || bytes.Equal(bytes.TrimSpace(raw), []byte("null")) {
		return nil
	}
	return bytes.Clone(raw)
}

func cloneRequiredRaw(raw json.RawMessage) json.RawMessage {
	if len(raw) == 0 {
		return json.RawMessage("null")
	}
	return bytes.Clone(raw)
}

func cloneOptionalSlice[T any](src []T) []T {
	if len(src) == 0 {
		return nil
	}
	return slices.Clone(src)
}

func cloneRawMap(src map[string]json.RawMessage) map[string]json.RawMessage {
	if len(src) == 0 {
		return nil
	}
	dst := make(map[string]json.RawMessage, len(src))
	for k, v := range src {
		dst[k] = cloneRequiredRaw(v)
	}
	return dst
}

func cloneRawMapPtr(src *map[string]json.RawMessage) *map[string]json.RawMessage {
	if src == nil || *src == nil {
		return nil
	}
	dst := make(map[string]json.RawMessage, len(*src))
	for k, v := range *src {
		dst[k] = cloneRequiredRaw(v)
	}
	return &dst
}

func cloneAssetRef(src canonical.AssetRef) canonical.AssetRef {
	return canonical.AssetRef{
		Digest:      src.Digest,
		Mime:        src.Mime,
		Size:        src.Size,
		URI:         clonePtr(src.URI),
		Annotations: cloneOptionalRaw(src.Annotations),
	}
}

func cloneMetadata(src canonical.Metadata) canonical.Metadata {
	var dst canonical.Metadata
	if src.Input != nil {
		in := &canonical.Input{
			Turn:           clonePtr(src.Input.Turn),
			Source:         clonePtr(src.Input.Source),
			TextPart:       clonePtr(src.Input.TextPart),
			CapabilityPart: clonePtr(src.Input.CapabilityPart),
			NoticePart:     clonePtr(src.Input.NoticePart),
			NoticeIDs:      cloneOptionalSlice(src.Input.NoticeIDs),
		}
		if len(src.Input.References) != 0 {
			in.References = clone(src.Input.References)
		}
		dst.Input = in
	}
	if src.Presentation != nil {
		pres := &canonical.Presentation{}
		if len(src.Presentation.Groupings) != 0 {
			pres.Groupings = clone(src.Presentation.Groupings)
		}
		if len(src.Presentation.Displays) != 0 {
			pres.Displays = clone(src.Presentation.Displays)
		}
		if len(src.Presentation.Tools) != 0 {
			pres.Tools = clone(src.Presentation.Tools)
		}
		if len(src.Presentation.Delegations) != 0 {
			delegations := slices.Clone(src.Presentation.Delegations)
			slices.Sort(delegations)
			pres.Delegations = slices.Compact(delegations)
		}
		if len(src.Presentation.Failures) != 0 {
			pres.Failures = make([]canonical.ToolFailure, len(src.Presentation.Failures))
			for i, failure := range src.Presentation.Failures {
				pres.Failures[i] = canonical.ToolFailure{
					Index:   failure.Index,
					ID:      failure.ID,
					Name:    failure.Name,
					Failure: protocol.ToolFailure{Code: clonePtr(failure.Failure.Code), Origin: failure.Failure.Origin},
					Message: clonePtr(failure.Message),
				}
			}
		}
		dst.Presentation = pres
	}
	if src.Execution != nil {
		exec := &canonical.Execution{
			Purpose:       clonePtr(src.Execution.Purpose),
			Source:        clonePtr(src.Execution.Source),
			Media:         clonePtr(src.Execution.Media),
			ContextTokens: clonePtr(src.Execution.ContextTokens),
		}
		if src.Execution.Timing != nil {
			exec.Timing = &canonical.Timing{
				ElapsedUs:    clonePtr(src.Execution.Timing.ElapsedUs),
				FirstTokenUs: clonePtr(src.Execution.Timing.FirstTokenUs),
			}
		}
		if src.Execution.Observation != nil {
			exec.Observation = clone(src.Execution.Observation)
		}
		if src.Execution.Phases != nil {
			exec.Phases = &protocol.ConversationObservationPhases{
				ActiveUs:  clonePtr(src.Execution.Phases.ActiveUs),
				WaitingUs: clonePtr(src.Execution.Phases.WaitingUs),
				OutputUs:  clonePtr(src.Execution.Phases.OutputUs),
				ToolsUs:   clonePtr(src.Execution.Phases.ToolsUs),
				OtherUs:   clonePtr(src.Execution.Phases.OtherUs),
			}
		}
		if src.Execution.Retry != nil {
			exec.Retry = clone(src.Execution.Retry)
		}
		if src.Execution.ToolStarted != nil {
			exec.ToolStarted = &canonical.ToolStarted{
				ID:   clonePtr(src.Execution.ToolStarted.ID),
				Name: src.Execution.ToolStarted.Name,
			}
		}
		dst.Execution = exec
	}
	if src.Context != nil {
		ctx := cloneContext(*src.Context)
		dst.Context = &ctx
	}
	if src.State != nil {
		dst.State = &canonical.StateChanges{
			Session:     cloneRawMap(src.State.Session),
			Application: cloneRawMap(src.State.Application),
			User:        cloneRawMap(src.State.User),
		}
	}
	if src.Compaction != nil {
		dst.Compaction = &canonical.Maintenance{
			LatestInstruction: clonePtr(src.Compaction.LatestInstruction),
		}
		if src.Compaction.Failure != nil {
			dst.Compaction.Failure = clone(src.Compaction.Failure)
		}
	}
	if src.Runtime != nil {
		rt := &canonical.Runtime{
			Created:            clonePtr(src.Runtime.Created),
			After:              clonePtr(src.Runtime.After),
			UserVisible:        src.Runtime.UserVisible,
			AgentVisible:       src.Runtime.AgentVisible,
			OutputLimitReached: src.Runtime.OutputLimitReached,
			Steer:              src.Runtime.Steer,
			TurnContext:        src.Runtime.TurnContext,
			Logs:               cloneOptionalSlice(src.Runtime.Logs),
		}
		if src.Runtime.Inference != nil {
			rt.Inference = &canonical.Inference{
				Provider:        src.Runtime.Inference.Provider,
				RequestedModel:  src.Runtime.Inference.RequestedModel,
				ResolvedModel:   clonePtr(src.Runtime.Inference.ResolvedModel),
				ProviderSession: clonePtr(src.Runtime.Inference.ProviderSession),
				ReplayPrefix:    src.Runtime.Inference.ReplayPrefix,
			}
		}
		if src.Runtime.Notes != nil {
			notes := &canonical.OperationNotes{
				EmptyResponse:      src.Runtime.Notes.EmptyResponse,
				DeliveredSubagent:  clonePtr(src.Runtime.Notes.DeliveredSubagent),
				CancelledSubagents: cloneOptionalSlice(src.Runtime.Notes.CancelledSubagents),
				TurnLimit:          src.Runtime.Notes.TurnLimit,
			}
			if src.Runtime.Notes.Context != nil {
				pc := &canonical.PromptContext{
					State:    src.Runtime.Notes.Context.State,
					Retained: cloneOptionalSlice(src.Runtime.Notes.Context.Retained),
				}
				if len(src.Runtime.Notes.Context.Parts) != 0 {
					pc.Parts = maps.Clone(src.Runtime.Notes.Context.Parts)
				}
				notes.Context = pc
			}
			rt.Notes = notes
		}
		dst.Runtime = rt
	}
	return dst
}

func cloneContext(src canonical.Context) canonical.Context {
	dst := canonical.Context{
		Captured:    src.Captured,
		Title:       clonePtr(src.Title),
		Policy:      clonePtr(src.Policy),
		Environment: clonePtr(src.Environment),
		Personal:    clonePtr(src.Personal),
	}
	if len(src.Guides) != 0 {
		dst.Guides = maps.Clone(src.Guides)
	}
	if src.LoadedTools != nil {
		var tools []string
		if *src.LoadedTools == nil {
			tools = []string{}
		} else {
			tools = slices.Clone(*src.LoadedTools)
		}
		dst.LoadedTools = &tools
	}
	return dst
}

func cloneUsage(src *canonical.Usage) *canonical.Usage {
	if src == nil {
		return nil
	}
	dst := &canonical.Usage{
		Input:      clonePtr(src.Input),
		Output:     clonePtr(src.Output),
		Total:      clonePtr(src.Total),
		CacheRead:  clonePtr(src.CacheRead),
		CacheWrite: clonePtr(src.CacheWrite),
		Reasoning:  clonePtr(src.Reasoning),
		Cost:       clonePtr(src.Cost),
		Provider:   cloneOptionalRaw(src.Provider),
	}
	if src.Pricing != nil {
		pricing := &canonical.UsagePricing{}
		if src.Pricing.Estimate != nil {
			est := &protocol.UsageCost{
				UsdMicros: src.Pricing.Estimate.UsdMicros,
				Responses: src.Pricing.Estimate.Responses,
			}
			if src.Pricing.Estimate.Breakdown != nil {
				est.Breakdown = clone(src.Pricing.Estimate.Breakdown)
			}
			pricing.Estimate = est
		}
		dst.Pricing = pricing
	}
	return dst
}

func cloneSources(src canonical.Sources) canonical.Sources {
	dst := canonical.Sources{
		SearchSuggestions: clonePtr(src.SearchSuggestions),
	}
	if len(src.Citations) != 0 {
		dst.Citations = make([]canonical.Citation, len(src.Citations))
		for i, item := range src.Citations {
			dst.Citations[i] = canonical.Citation{
				URI:   clonePtr(item.URI),
				Title: clonePtr(item.Title),
				Start: clonePtr(item.Start),
				End:   clonePtr(item.End),
			}
		}
	}
	if len(src.Grounding) != 0 {
		dst.Grounding = make([]canonical.Grounding, len(src.Grounding))
		for i, item := range src.Grounding {
			dst.Grounding[i] = canonical.Grounding{
				URI:   item.URI,
				Title: clonePtr(item.Title),
			}
		}
	}
	return dst
}

func cloneMessage(src *canonical.Message, borrowToolValues bool) *canonical.Message {
	if src == nil {
		return nil
	}
	dst := &canonical.Message{
		Role:      src.Role,
		PartCount: src.PartCount,
		Model:     clonePtr(src.Model),
		Usage:     cloneUsage(src.Usage),
		Finish:    clonePtr(src.Finish),
		Sources:   cloneSources(src.Sources),
	}
	if src.Error != nil {
		dst.Error = &canonical.ResponseError{
			Code:               clonePtr(src.Error.Code),
			Message:            src.Error.Message,
			RetryExhausted:     src.Error.RetryExhausted,
			RetryWaitExhausted: src.Error.RetryWaitExhausted,
		}
	}
	if len(src.Content) != 0 || borrowToolValues && src.Content != nil {
		dst.Content = make([]canonical.Block, len(src.Content))
		for i, block := range src.Content {
			dst.Content[i] = cloneBlock(block, borrowToolValues)
		}
	}
	return dst
}

func cloneBlock(block canonical.Block, borrowToolValues bool) canonical.Block {
	dst := canonical.Block{Index: block.Index, Kind: block.Kind}
	switch block.Kind {
	case "text":
		switch v := block.Data.(type) {
		case *canonical.TextContent:
			if v != nil {
				copied := *v
				dst.Data = &copied
				return dst
			}
		case canonical.TextContent:
			dst.Data = &v
			return dst
		}
	case "image", "document":
		switch v := block.Data.(type) {
		case *canonical.AssetContent:
			if v != nil {
				dst.Data = &canonical.AssetContent{Asset: cloneAssetRef(v.Asset)}
				return dst
			}
		case canonical.AssetContent:
			dst.Data = &canonical.AssetContent{Asset: cloneAssetRef(v.Asset)}
			return dst
		}
	case "file":
		switch v := block.Data.(type) {
		case *canonical.FileContent:
			if v != nil {
				dst.Data = &canonical.FileContent{File: canonical.FileRef{URI: v.File.URI, Mime: clonePtr(v.File.Mime)}}
				return dst
			}
		case canonical.FileContent:
			dst.Data = &canonical.FileContent{File: canonical.FileRef{URI: v.File.URI, Mime: clonePtr(v.File.Mime)}}
			return dst
		}
	case "thinking":
		switch v := block.Data.(type) {
		case *canonical.Thinking:
			if v != nil {
				dst.Data = &canonical.Thinking{Text: v.Text, Signature: clonePtr(v.Signature)}
				return dst
			}
		case canonical.Thinking:
			dst.Data = &canonical.Thinking{Text: v.Text, Signature: clonePtr(v.Signature)}
			return dst
		}
	case "redacted_thinking":
		var src *canonical.RedactedThinking
		switch v := block.Data.(type) {
		case *canonical.RedactedThinking:
			src = v
		case canonical.RedactedThinking:
			src = &v
		}
		if src != nil {
			out := &canonical.RedactedThinking{
				Data:   []string{},
				Before: []*uint64{},
			}
			if len(src.Data) != 0 {
				out.Data = slices.Clone(src.Data)
			}
			if len(src.Before) != 0 {
				out.Before = make([]*uint64, len(src.Before))
				for i, item := range src.Before {
					out.Before[i] = clonePtr(item)
				}
			}
			dst.Data = out
			return dst
		}
	case "tool_request":
		var src *canonical.ToolRequest
		switch v := block.Data.(type) {
		case *canonical.ToolRequest:
			src = v
		case canonical.ToolRequest:
			src = &v
		}
		if src != nil {
			out := &canonical.ToolRequest{
				ID:        src.ID,
				Name:      src.Name,
				Arguments: cloneRequiredRaw(src.Arguments),
				Provider:  cloneRawMapPtr(src.Provider),
				ToolMeta:  cloneOptionalRaw(src.ToolMeta),
			}
			if src.Error != nil {
				out.Error = &canonical.ToolError{
					Code:    src.Error.Code,
					Message: src.Error.Message,
					Data:    cloneOptionalRaw(src.Error.Data),
				}
			}
			dst.Data = out
			return dst
		}
	case "tool_response":
		var src *canonical.ToolResponse
		switch v := block.Data.(type) {
		case *canonical.ToolResponse:
			src = v
		case canonical.ToolResponse:
			src = &v
		}
		if src != nil {
			out := &canonical.ToolResponse{
				ID:       src.ID,
				Name:     src.Name,
				Status:   src.Status,
				Revision: src.Revision,
				Provider: cloneRawMapPtr(src.Provider),
				Format:   src.Format,
				Meta:     cloneRawMapPtr(src.Meta),
			}
			if len(src.Attachments) != 0 {
				out.Attachments = make([]canonical.Attachment, len(src.Attachments))
				for i, att := range src.Attachments {
					out.Attachments[i] = canonical.Attachment{Asset: cloneAssetRef(att.Asset)}
				}
			}
			if len(src.Result.Assets) != 0 {
				out.Result.Assets = make([]canonical.BinarySlot, len(src.Result.Assets))
				for i, slot := range src.Result.Assets {
					out.Result.Assets[i] = canonical.BinarySlot{
						Pointer:  slot.Pointer,
						Asset:    cloneAssetRef(slot.Asset),
						Encoding: slot.Encoding,
					}
				}
			}
			if borrowToolValues {
				out.Result.Value = src.Result.Value
				out.OriginalID = src.OriginalID
			} else {
				raw, err := json.Marshal(src.Result.Value)
				if err != nil {
					panic(err)
				}
				out.Result.Value = raw
			}
			dst.Data = out
			return dst
		}
	}
	return clone(block)
}

func cloneCompaction(src *canonical.Compaction) *canonical.Compaction {
	if src == nil {
		return nil
	}
	dst := &canonical.Compaction{
		Boundary: src.Boundary,
		Summary:  *cloneMessage(&src.Summary, false),
		Start:    src.Start.Round(0),
		End:      src.End.Round(0),
	}
	if len(src.Files) != 0 {
		dst.Files = make([]canonical.CompactedFile, len(src.Files))
		for i, file := range src.Files {
			dst.Files[i] = canonical.CompactedFile{
				Path:     file.Path,
				Modified: clonePtr(file.Modified),
				Summary:  clonePtr(file.Summary),
			}
		}
	}
	if src.Native != nil {
		dst.Native = &canonical.NativeCompaction{
			Source: src.Native.Source,
			Items:  cloneRequiredRaw(src.Native.Items),
		}
	}
	return dst
}

func cloneOperation(src *canonical.Operation) *canonical.Operation {
	if src == nil {
		return nil
	}
	dst := &canonical.Operation{Kind: src.Kind}
	switch src.Kind {
	case "visibility":
		switch v := src.Data.(type) {
		case *canonical.Visibility:
			if v != nil {
				copied := *v
				dst.Data = &copied
				return dst
			}
		case canonical.Visibility:
			dst.Data = &v
			return dst
		}
	case "tool_meta":
		switch v := src.Data.(type) {
		case *canonical.ToolMeta:
			if v != nil {
				dst.Data = &canonical.ToolMeta{Call: v.Call, Patch: cloneRequiredRaw(v.Patch)}
				return dst
			}
		case canonical.ToolMeta:
			dst.Data = &canonical.ToolMeta{Call: v.Call, Patch: cloneRequiredRaw(v.Patch)}
			return dst
		}
	case "note":
		var note *canonical.Note
		switch v := src.Data.(type) {
		case *canonical.Note:
			note = v
		case canonical.Note:
			note = &v
		}
		if note != nil {
			out := &canonical.Note{Message: note.Message, Note: canonical.OperationNote{Kind: note.Note.Kind}}
			switch note.Note.Kind {
			case "empty_response", "turn_limit":
				dst.Data = out
				return dst
			case "subagent_delivered":
				switch d := note.Note.Data.(type) {
				case *canonical.SubagentDelivered:
					if d != nil {
						copied := *d
						out.Note.Data = &copied
						dst.Data = out
						return dst
					}
				case canonical.SubagentDelivered:
					out.Note.Data = &d
					dst.Data = out
					return dst
				}
			case "subagents_cancelled":
				var cancelled *canonical.SubagentsCancelled
				switch d := note.Note.Data.(type) {
				case *canonical.SubagentsCancelled:
					cancelled = d
				case canonical.SubagentsCancelled:
					cancelled = &d
				}
				if cancelled != nil {
					sessions := []string{}
					if len(cancelled.Sessions) != 0 {
						sessions = slices.Clone(cancelled.Sessions)
					}
					out.Note.Data = &canonical.SubagentsCancelled{Sessions: sessions}
					dst.Data = out
					return dst
				}
			}
		}
	}
	copied := clone(*src)
	return &copied
}

// Project borrows tool payloads until it selects the final output.
func cloneRecord(record canonical.Record, borrowToolValues bool) canonical.Record {
	copied := canonical.Record{
		ID:         record.ID,
		Created:    record.Created.Round(0),
		Turn:       record.Turn,
		Provenance: record.Provenance,
		Metadata:   cloneMetadata(record.Metadata),
		Kind:       record.Kind,
	}
	switch record.Kind {
	case "message":
		if msg := record.Message(); msg != nil {
			copied.Data = cloneMessage(msg, borrowToolValues)
			return copied
		}
	case "compaction":
		switch v := record.Data.(type) {
		case *canonical.Compaction:
			if v != nil {
				copied.Data = cloneCompaction(v)
				return copied
			}
		case canonical.Compaction:
			copied.Data = cloneCompaction(&v)
			return copied
		}
	case "configuration":
		switch v := record.Data.(type) {
		case *canonical.Configuration:
			if v != nil {
				copied.Data = &canonical.Configuration{Context: cloneContext(v.Context)}
				return copied
			}
		case canonical.Configuration:
			copied.Data = &canonical.Configuration{Context: cloneContext(v.Context)}
			return copied
		}
	case "operation":
		switch v := record.Data.(type) {
		case *canonical.Operation:
			if v != nil {
				copied.Data = cloneOperation(v)
				return copied
			}
		case canonical.Operation:
			copied.Data = cloneOperation(&v)
			return copied
		}
	}
	return clone(record)
}
