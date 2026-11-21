package main

import (
	"context"
	"encoding/json"
	"fmt"
	"time"

	"github.com/sailry/sailry-harness/internal/acceptance"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const cacheToolsSuite = "cache-tools"

func cacheToolsCase() acceptance.CaseResult {
	return acceptance.CaseResult{ID: cacheToolsSuite, Group: "model", Title: "Compare fresh Responses input tokens with and without tool declarations"}
}

func runAcceptanceCacheTools(ctx context.Context, execution *node.Node, config protocol.SessionConfig, timeout time.Duration) acceptance.CaseResult {
	started := time.Now()
	caseCtx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()
	result := cacheToolsCase()
	samples, err := execution.RunCacheToolsAcceptance(caseCtx, config)
	for _, sample := range samples {
		raw, _ := json.Marshal(sample)
		result.Evidence = append(result.Evidence, acceptance.Evidence{Kind: "cache-tools.request", Value: string(raw)})
	}
	result.Duration = time.Since(started)
	switch {
	case ctx.Err() != nil:
		result.Status, result.Detail = "canceled", "The comparison was canceled"
	case caseCtx.Err() != nil:
		result.Status, result.Detail = "blocked", "The comparison timed out"
	case err != nil:
		result.Status, result.Detail = "blocked", err.Error()
	default:
		result.Status = "passed"
		result.Detail = fmt.Sprintf("Warm fresh-input delta (with tools minus without tools) was %+d and %+d tokens.",
			int64(*samples[3].Fresh)-int64(*samples[2].Fresh), int64(*samples[5].Fresh)-int64(*samples[4].Fresh))
	}
	return result
}
