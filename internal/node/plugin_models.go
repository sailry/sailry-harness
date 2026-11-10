package node

import (
	"context"
	"database/sql"
	"strings"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) pluginModelCatalog(ctx context.Context, request protocol.Request) (protocol.Output, error) {
	var output protocol.Output
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		if request.Command.Kind == "list_plugin_models" {
			value, err := store.PluginModels(ctx, db)
			output = protocol.Output{Kind: "plugin_models", Data: value}
			return err
		}
		input := request.Command.Data.(*protocol.CommandResolvePluginModelPayload)
		_, value, err := store.ResolvePluginModel(ctx, db, input.Model, input.Effort, input.Config)
		output = protocol.Output{Kind: "session_config", Data: value}
		return err
	})
	return output, err
}

func (node *Node) generatePluginText(ctx context.Context, request protocol.Request) (protocol.Output, error) {
	if request.Plugin == nil {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "plugin provenance is required"}
	}
	ctx, release, err := node.beginPluginExecution(ctx, request)
	if err != nil {
		return protocol.Output{}, err
	}
	defer release()
	input := request.Command.Data.(*protocol.CommandGeneratePluginTextPayload)
	if strings.TrimSpace(input.Prompt) == "" || len(input.Prompt) > 64*1024 {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin prompt must contain 1–65536 bytes"}
	}
	var provider protocol.ConversationProvider
	var config protocol.SessionConfig
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		provider, config, err = store.ResolvePluginModel(ctx, db, input.Model, input.Effort, nil)
		return err
	})
	if err != nil {
		return protocol.Output{}, err
	}
	if err := node.renewProvider(ctx, provider.ID); err != nil {
		return protocol.Output{}, err
	}
	var secret *protocol.Secret
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		provider, config, secret, err = store.CapturePluginGeneration(ctx, db, node.ID(), input.Model, input.Effort)
		return err
	})
	if err != nil {
		return protocol.Output{}, err
	}
	binding, err := model.Capture(provider, config, secret, protocol.NewSessionID())
	if err != nil {
		return protocol.Output{}, err
	}
	languageModel, err := node.newModel(ctx, binding)
	if err != nil {
		return protocol.Output{}, err
	}
	record := canonical.NewRecord(protocol.NewTurnID(), "message", canonical.NewMessage(canonical.User, canonical.Text(0, input.Prompt)), time.Now())
	call, err := model.BuildCall(binding, model.Request{Records: []canonical.Record{record}})
	if err != nil {
		return protocol.Output{}, err
	}
	operation, cancel := context.WithCancel(ctx)
	defer cancel()
	timer := time.AfterFunc(120*time.Second, cancel)
	defer timer.Stop()
	stream, err := languageModel.Stream(operation, call)
	if err != nil {
		return protocol.Output{}, model.Fault(err)
	}
	result := protocol.PluginText{Model: config.Model}
	finished, nonText, textBlock := false, false, false
	for part := range stream {
		if operation.Err() != nil {
			break
		}
		timer.Reset(120 * time.Second)
		switch part.Type {
		case fantasy.StreamPartTypeTextDelta:
			if !textBlock && result.Text != "" {
				result.Text += "\n"
			}
			result.Text += part.Delta
			textBlock = true
		case fantasy.StreamPartTypeReasoningDelta:
			textBlock = false
		case fantasy.StreamPartTypeToolCall, fantasy.StreamPartTypeToolInputStart, fantasy.StreamPartTypeToolInputDelta, fantasy.StreamPartTypeToolInputEnd:
			nonText = true
		case fantasy.StreamPartTypeError:
			return protocol.Output{}, model.Fault(part.Error)
		case fantasy.StreamPartTypeFinish:
			finished = true
		}
		if usage := model.PartUsage(part); usage.Total != nil {
			result.Tokens = *usage.Total
		}
		if len(result.Text) > 64*1024 {
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "plugin model response exceeds the size limit"}
		}
	}
	if ctx.Err() != nil {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "plugin model request was cancelled"}
	}
	if operation.Err() != nil {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "plugin model request timed out"}
	}
	if nonText || !finished {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "plugin model returned non-text or incomplete output"}
	}
	if strings.TrimSpace(result.Text) == "" {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "plugin model returned no text"}
	}
	return protocol.Output{Kind: "plugin_text", Data: result}, nil
}
