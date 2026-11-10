package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"net/url"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/santhosh-tekuri/jsonschema/v6"
)

func elicitationInput(record canonical.Record) *canonical.Elicitation {
	if notice := planNotice(record); notice != nil && notice.Kind == "elicitation" {
		if input, ok := notice.Data.(*canonical.Elicitation); ok {
			return input
		}
	}
	return nil
}

func ValidateMCPElicitation(ctx context.Context, db *sql.Conn, record canonical.Record) error {
	input := elicitationInput(record)
	if input == nil {
		return nil
	}
	if record.Provenance.Author != "mcp_elicitation" {
		return invalid("MCP input source is invalid")
	}
	if _, err := activeAgentRun(ctx, db, record.Turn); err != nil {
		return err
	}
	if err := validateMCPSpec(input.Spec); err != nil {
		return err
	}
	_, _, _, err := locateAgentCall(ctx, db, record.Turn, input.Parent)
	return err
}

func validateMCPSpec(spec protocol.ConversationQuestionSpec) error {
	if strings.TrimSpace(spec.Prompt) == "" || len(spec.Prompt) > 64*1024 {
		return invalid("MCP question prompt is invalid")
	}
	switch value := spec.Input.Data.(type) {
	case *protocol.ConversationQuestionInputFormPayload:
		if spec.Input.Kind != "form" || len(value.Fields) > 64 {
			return invalid("MCP form is invalid")
		}
		seen := map[string]bool{}
		for _, field := range value.Fields {
			if field.Name == "" || field.Title == "" || seen[field.Name] {
				return invalid("MCP form fields are invalid")
			}
			seen[field.Name] = true
		}
	case *protocol.ConversationQuestionInputUrlPayload:
		address, err := url.Parse(value.Url)
		if spec.Input.Kind != "url" || err != nil || address.Host == "" || address.User != nil || address.Scheme != "https" && address.Scheme != "http" || value.ElicitationID == "" {
			return invalid("MCP input URL is invalid")
		}
	default:
		return invalid("MCP input must not change model content")
	}
	return nil
}

func BeginMCPElicitation(ctx context.Context, db *sql.Conn, record canonical.Record) (protocol.ConversationEntry, protocol.ConversationQuestion, []protocol.Event, error) {
	value := protocol.DefaultConversationQuestion()
	if elicitationInput(record) == nil {
		return protocol.ConversationEntry{}, value, nil, invalid("MCP input has no canonical request")
	}
	if err := ValidateMCPElicitation(ctx, db, record); err != nil {
		return protocol.ConversationEntry{}, value, nil, err
	}
	run, err := activeAgentRun(ctx, db, record.Turn)
	if err != nil {
		return protocol.ConversationEntry{}, value, nil, err
	}
	entry, changes, err := AppendCanonical(ctx, db, run.Session, record)
	if err != nil {
		return entry, value, nil, err
	}
	value.ID, value.Session, value.Turn = protocol.NewQuestionID(), run.Session, record.Turn
	value.Entry, value.Index, value.State = record.ID, 0, protocol.ConversationQuestionState{Kind: "pending"}
	body, err := encode(value)
	if err != nil {
		return entry, value, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO agent_questions(id,turn,call,body) VALUES(?1,?2,?3,?4)", value.ID.String(), value.Turn.String(), record.ID, body); err != nil {
		return entry, value, nil, err
	}
	events, err := activityEvent(ctx, db, value.Session)
	return entry, value, append(changes, events...), err
}

func CancelMCPElicitation(ctx context.Context, db *sql.Conn, id protocol.QuestionID) (protocol.ConversationQuestion, []protocol.Event, error) {
	value, err := ReadQuestion(ctx, db, id)
	if err != nil || value.State.Kind != "pending" {
		return value, nil, err
	}
	value.State = protocol.ConversationQuestionState{Kind: "cancelled"}
	if err = putQuestion(ctx, db, value); err != nil {
		return value, nil, err
	}
	changes, err := activityEvent(ctx, db, value.Session)
	return value, changes, err
}

func validateMCPAnswer(ctx context.Context, db *sql.Conn, question protocol.ConversationQuestion, answer protocol.ConversationQuestionAnswer) error {
	if answer.Kind != "form" {
		return nil
	}
	record, err := readBody[canonical.Record](ctx, db, "SELECT body FROM agent_events WHERE turn=?1 AND id=?2", question.Turn.String(), question.Entry)
	if err != nil {
		return err
	}
	if record == nil || elicitationInput(*record) == nil {
		return invalid("MCP input has no canonical request")
	}
	validator, err := compileMCPForm(elicitationInput(*record).Schema)
	if err != nil {
		return err
	}
	body, err := json.Marshal(answer.Data)
	if err != nil {
		return invalid("invalid MCP answer")
	}
	var values any
	if json.Unmarshal(body, &values) != nil || validator.Validate(values) != nil {
		return invalid("answer does not match the requested MCP form")
	}
	return nil
}

func compileMCPForm(schema json.RawMessage) (*jsonschema.Schema, error) {
	var document any
	decoder := json.NewDecoder(bytes.NewReader(schema))
	decoder.UseNumber()
	if decoder.Decode(&document) != nil {
		return nil, invalid("invalid MCP input schema")
	}
	compiler := jsonschema.NewCompiler()
	compiler.UseLoader(jsonschema.SchemeURLLoader{})
	compiler.AssertFormat()
	if err := compiler.AddResource("urn:sailry:mcp-form", document); err != nil {
		return nil, invalid("invalid MCP input schema")
	}
	value, err := compiler.Compile("urn:sailry:mcp-form")
	if err != nil {
		return nil, invalid("invalid MCP input schema")
	}
	return value, nil
}

func MCPFormSpec(message string, schema json.RawMessage) (protocol.ConversationQuestionSpec, error) {
	spec := protocol.ConversationQuestionSpec{Prompt: message, Input: protocol.ConversationQuestionInput{Kind: "form"}}
	var document struct {
		Type       string                     `json:"type"`
		Properties map[string]json.RawMessage `json:"properties"`
		Required   []string                   `json:"required"`
		Order      []string                   `json:"propertyOrder"`
	}
	if len(schema) > 256*1024 || json.Unmarshal(schema, &document) != nil || document.Type != "object" || document.Properties == nil || len(document.Properties) > 64 {
		return spec, invalid("unsupported or invalid MCP form")
	}
	for _, name := range document.Required {
		if _, exists := document.Properties[name]; !exists {
			return spec, invalid("unsupported or invalid MCP form")
		}
	}
	names := []string{}
	for name := range document.Properties {
		names = append(names, name)
	}
	slices.Sort(names)
	ordered := []string{}
	for _, name := range append(document.Order, names...) {
		if _, exists := document.Properties[name]; !exists {
			return spec, invalid("unsupported or invalid MCP form")
		}
		if !slices.Contains(ordered, name) {
			ordered = append(ordered, name)
		}
	}
	form := &protocol.ConversationQuestionInputFormPayload{Fields: []protocol.ConversationQuestionFormField{}}
	for _, name := range ordered {
		field, err := mcpFormField(name, document.Properties[name])
		if err != nil {
			return spec, err
		}
		field.Required = slices.Contains(document.Required, name)
		form.Fields = append(form.Fields, field)
	}
	spec.Input.Data = form
	if err := validateMCPSpec(spec); err != nil {
		return spec, err
	}
	_, err := compileMCPForm(schema)
	return spec, err
}

func mcpFormField(name string, raw json.RawMessage) (protocol.ConversationQuestionFormField, error) {
	field := protocol.ConversationQuestionFormField{Name: name, Title: name}
	var property struct {
		Type        string          `json:"type"`
		Title       *string         `json:"title"`
		Description *string         `json:"description"`
		MinLength   *uint64         `json:"minLength"`
		MaxLength   *uint64         `json:"maxLength"`
		Format      *string         `json:"format"`
		Minimum     *json.Number    `json:"minimum"`
		Maximum     *json.Number    `json:"maximum"`
		Enum        []string        `json:"enum"`
		EnumNames   []string        `json:"enumNames"`
		OneOf       json.RawMessage `json:"oneOf"`
		Items       json.RawMessage `json:"items"`
		MinItems    *uint64         `json:"minItems"`
		MaxItems    *uint64         `json:"maxItems"`
	}
	if json.Unmarshal(raw, &property) != nil {
		return field, invalid("unsupported or invalid MCP form")
	}
	if property.Title != nil {
		field.Title = *property.Title
	}
	field.Description = property.Description
	switch property.Type {
	case "string":
		if property.Enum != nil || len(property.OneOf) != 0 {
			options, err := mcpFormChoices(raw, "oneOf")
			if err != nil {
				return field, err
			}
			field.Input = protocol.ConversationQuestionFormInput{Kind: "choice", Data: &protocol.ConversationQuestionFormInputChoicePayload{Options: options}}
		} else {
			field.Input = protocol.ConversationQuestionFormInput{Kind: "text", Data: &protocol.ConversationQuestionFormInputTextPayload{MinLength: property.MinLength, MaxLength: property.MaxLength, Format: property.Format}}
		}
	case "number", "integer":
		field.Input = protocol.ConversationQuestionFormInput{Kind: "number", Data: &protocol.ConversationQuestionFormInputNumberPayload{Integer: property.Type == "integer", Minimum: property.Minimum, Maximum: property.Maximum}}
	case "boolean":
		field.Input = protocol.ConversationQuestionFormInput{Kind: "boolean"}
	case "array":
		options, err := mcpFormChoices(property.Items, "anyOf")
		if err != nil {
			return field, err
		}
		field.Input = protocol.ConversationQuestionFormInput{Kind: "choice", Data: &protocol.ConversationQuestionFormInputChoicePayload{Options: options, Multiple: true, MinItems: property.MinItems, MaxItems: property.MaxItems}}
	default:
		return field, invalid("unsupported or invalid MCP form")
	}
	return field, nil
}

func mcpFormChoices(raw json.RawMessage, titled string) ([]protocol.ConversationQuestionFormChoice, error) {
	var property map[string]json.RawMessage
	if json.Unmarshal(raw, &property) != nil {
		return nil, invalid("unsupported or invalid MCP form")
	}
	options := []protocol.ConversationQuestionFormChoice{}
	if values, exists := property["enum"]; exists {
		var names, titles []string
		if json.Unmarshal(values, &names) != nil {
			return nil, invalid("unsupported or invalid MCP form")
		}
		_ = json.Unmarshal(property["enumNames"], &titles)
		for index, name := range names {
			title := name
			if index < len(titles) {
				title = titles[index]
			}
			options = append(options, protocol.ConversationQuestionFormChoice{Value: name, Title: title})
		}
	} else {
		var values []struct {
			Value *string `json:"const"`
			Title *string `json:"title"`
		}
		if json.Unmarshal(property[titled], &values) != nil {
			return nil, invalid("unsupported or invalid MCP form")
		}
		for _, value := range values {
			if value.Value == nil || value.Title == nil {
				return nil, invalid("unsupported or invalid MCP form")
			}
			options = append(options, protocol.ConversationQuestionFormChoice{Value: *value.Value, Title: *value.Title})
		}
	}
	if len(options) == 0 {
		return nil, invalid("unsupported or invalid MCP form")
	}
	seen := map[string]bool{}
	for _, option := range options {
		if option.Title == "" || seen[option.Value] {
			return nil, invalid("unsupported or invalid MCP form")
		}
		seen[option.Value] = true
	}
	return options, nil
}

func elicitationRaw(record canonical.Record, index uint64) (*rawAgentTool, error) {
	if index != 0 {
		return nil, nil
	}
	raw := &rawAgentTool{Name: "mcp_elicitation", Arguments: json.RawMessage("null"), Presentation: protocol.ToolPresentationSummary, Grouping: protocol.ToolGroupingSequence, Images: []protocol.ConversationImage{}}
	if input := elicitationInput(record); input != nil {
		raw.ID = &record.ID
		body, err := json.Marshal(input.Spec)
		if err != nil {
			return nil, err
		}
		raw.Arguments = body
	} else if notice := planNotice(record); notice != nil && notice.Kind == "elicitation_result" {
		result, ok := notice.Data.(*canonical.ElicitationResult)
		if !ok {
			return nil, invalid("MCP input result is invalid")
		}
		body, err := json.Marshal(result.Response)
		if err != nil {
			return nil, err
		}
		response := json.RawMessage(body)
		raw.ID, raw.Result = &result.Call, &response
	} else {
		return nil, nil
	}
	return raw, nil
}
