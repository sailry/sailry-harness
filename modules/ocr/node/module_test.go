package ocrnode

import (
	"context"
	"encoding/json"
	"slices"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type ocrHost struct {
	moduleapi.Host
	t      *testing.T
	models []protocol.ModelsLocalModel
	scoped []moduleapi.ScopedToolConfig
}

func (host *ocrHost) ListLocalModels(_ context.Context, kind protocol.ModelsKind) ([]protocol.ModelsLocalModel, error) {
	if kind != protocol.ModelsKindOcr {
		host.t.Fatalf("listed models of kind %q", kind)
	}
	return host.models, nil
}

func (host *ocrHost) ScopedTool(_ *store.AgentInvocation, cfg moduleapi.ScopedToolConfig) (engine.Tool, error) {
	host.scoped = append(host.scoped, cfg)
	return engine.Tool{Definition: fantasy.FunctionTool{Name: cfg.Spec.Name}}, nil
}

func TestOCRToolCatalogAndArguments(t *testing.T) {
	host := &ocrHost{t: t, models: []protocol.ModelsLocalModel{
		{ID: "installed", State: protocol.ModelsLocalState{Kind: "installed"}},
		{ID: "missing", State: protocol.ModelsLocalState{Kind: "missing"}},
	}}
	tools, err := Module().ToolsForTurn(t.Context(), host, &store.AgentInvocation{})
	if err != nil || len(tools) != 3 || len(host.scoped) != 2 {
		t.Fatalf("tools = %d, scoped = %d, error = %v", len(tools), len(host.scoped), err)
	}
	list := tools[0]
	if list.Definition.Name != "list_ocr_models" || !list.ReadOnly || !list.Concurrent || list.Approval {
		t.Fatalf("list tool policy = %+v", list)
	}
	for _, row := range []struct {
		args string
		want []string
	}{
		{`{}`, []string{"installed", "missing"}},
		{`{"installed_only":true}`, []string{"installed"}},
	} {
		result := list.Execute(t.Context(), canonical.ToolRequest{Arguments: json.RawMessage(row.args)})
		if result.Failure != nil {
			t.Fatal(result.Failure)
		}
		var ids []string
		for _, model := range result.Value.(map[string]any)["models"].([]protocol.ModelsLocalModel) {
			ids = append(ids, model.ID)
		}
		if !slices.Equal(ids, row.want) {
			t.Fatalf("models for %s = %v", row.args, ids)
		}
	}
	if result := list.Execute(t.Context(), canonical.ToolRequest{Arguments: json.RawMessage(`{"installed_only":1}`)}); result.Failure == nil || result.Failure.Code != protocol.ErrorCodeInvalidRequest {
		t.Fatalf("invalid list arguments = %+v", result)
	}
	id := strings.Repeat("a", 64)
	for i, operation := range []protocol.ToolOperation{protocol.ToolOperationInstallModel, protocol.ToolOperationRemoveModel} {
		cfg := host.scoped[i]
		if cfg.Spec.Operation != operation {
			t.Fatalf("model lifecycle operation = %q, want %q", cfg.Spec.Operation, operation)
		}
		if got, err := cfg.PrepareArgs(t.Context(), json.RawMessage(`{"id":"`+id+`"}`)); err != nil || string(got) != `{"model":{"id":"`+id+`","kind":"ocr","target":"node"}}` {
			t.Fatalf("%s arguments = %s, %v", operation, got, err)
		}
		if _, err := cfg.PrepareArgs(t.Context(), json.RawMessage(`{"id":"`+strings.ToUpper(id)+`"}`)); err == nil {
			t.Fatalf("%s accepted a noncanonical model ID", operation)
		}
	}
}
