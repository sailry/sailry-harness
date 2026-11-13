package model

import (
	"context"
	"time"

	"charm.land/fantasy"
	"charm.land/fantasy/providers/anthropic"
	"charm.land/fantasy/providers/google"
	"charm.land/fantasy/providers/openai"
	"github.com/cenkalti/backoff/v5"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/clock"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type Purpose string

const (
	Main           Purpose = "main"
	SummaryPurpose Purpose = "summary"
)

type Attempt struct {
	RequestID      string
	RequestStarted time.Time
	Index          uint64
	Started        time.Time
	Finished       time.Time
	FirstToken     *time.Time
	Usage          canonical.Usage
	Failure        *Failure
	// RetryDelay is the wait before the next attempt; nil on the last attempt.
	RetryDelay        *time.Duration
	Status            *uint16
	ResponseUs        *uint64
	Categories        []protocol.ConversationObservationCategory
	CompleteContext   bool
	ReasoningStreamed bool
}

func PartUsage(part fantasy.StreamPart) canonical.Usage {
	var result canonical.Usage
	metadata, ok := part.ProviderMetadata[UsageMetadataKey].(*UsageMetadata)
	if !ok || metadata == nil || !metadata.Reported {
		return result
	}
	count := func(value *int64) *uint64 {
		if value == nil || *value < 0 {
			return nil
		}
		v := uint64(*value)
		return &v
	}
	result.Input = count(metadata.Input)
	result.Output = count(metadata.Output)
	result.Total = count(metadata.Total)
	result.CacheRead = count(metadata.CacheRead)
	result.CacheWrite = count(metadata.CacheWrite)
	result.Reasoning = count(metadata.Reasoning)
	result.Provider = append([]byte(nil), metadata.Raw...)
	return result
}
func mergeUsage(previous, current canonical.Usage) canonical.Usage {
	for _, pair := range []struct {
		from *uint64
		to   **uint64
	}{{current.Input, &previous.Input}, {current.Output, &previous.Output}, {current.Total, &previous.Total}, {current.CacheRead, &previous.CacheRead}, {current.CacheWrite, &previous.CacheWrite}, {current.Reasoning, &previous.Reasoning}} {
		if pair.from != nil {
			*pair.to = pair.from
		}
	}
	if len(current.Provider) > 0 {
		previous.Provider = current.Provider
	}
	return previous
}

// MainRetries is how often a request, a main one or a summary, is sent again
// after a transient failure, each time after retryDelay: a fixed wait, which
// a provider's Retry-After neither shortens nor lengthens.
const (
	MainRetries = 9
	retryDelay  = 60 * time.Second
)

func Run(ctx context.Context, languageModel fantasy.LanguageModel, call fantasy.Call, purpose Purpose, timer clock.Clock, observe func(Attempt) error, yield func(fantasy.StreamPart) bool) error {
	retries := uint64(MainRetries)
	buffered := purpose == SummaryPurpose
	schedule := backoff.NewConstantBackOff(retryDelay)
	requestID, requestStarted := protocol.NewRequestID().String(), timer.Now()
	var secrets []protocol.Secret
	if sdk, ok := languageModel.(*sdkModel); ok {
		secrets = append(secrets, sdk.config.Protection...)
		if sdk.config.Key != "" {
			secrets = append(secrets, protocol.NewSecret(sdk.config.Key))
		}
	}
	for index := uint64(0); ; index++ {
		if err := ctx.Err(); err != nil {
			return Classify(err)
		}
		attempt := Attempt{RequestID: requestID, RequestStarted: requestStarted, Index: index, Started: timer.Now()}
		owner, cancel := context.WithCancel(ctx)
		owner, capture := captureAttempt(owner, call, timer, attempt.Started)
		stream, err := languageModel.Stream(owner, call)
		var pending []fantasy.StreamPart
		escaped, finished, released := false, false, false
		if err == nil && stream != nil {
			for part := range stream {
				attempt.Usage = mergeUsage(attempt.Usage, PartUsage(part))
				if part.Type == fantasy.StreamPartTypeReasoningDelta && part.Delta != "" {
					attempt.ReasoningStreamed = true
				}
				if part.Type == fantasy.StreamPartTypeReasoningEnd {
					if metadata, ok := part.ProviderMetadata[anthropic.Name].(*anthropic.ReasoningOptionMetadata); ok && metadata.RedactedData != "" {
						attempt.ReasoningStreamed = true
					}
				}
				if metadata, ok := part.ProviderMetadata[responseMetadataKey].(*responseMetadata); ok {
					attempt.Status = metadata.Status
				}
				if attempt.FirstToken == nil && outputPart(part) {
					first := timer.Now()
					attempt.FirstToken = &first
				}
				if part.Type == fantasy.StreamPartTypeError {
					err = part.Error
					if err == nil {
						err = &Failure{Kind: InvalidResponse, Message: "model stream failed without an error"}
					}
					break
				}
				if part.Type == fantasy.StreamPartTypeFinish {
					finished = true
				}
				if buffered {
					pending = append(pending, part)
				} else {
					escaped = true
					if yield != nil && !yield(part) {
						released = true
						err = context.Canceled
						break
					}
				}
			}
		}
		if err == nil && !finished {
			err = &Failure{Kind: InvalidResponse, Message: "Model stream ended before a completion event"}
		}
		if ctx.Err() != nil {
			err = ctx.Err()
		}
		cancel()
		attempt.Finished = timer.Now()
		capture.apply(&attempt)
		exhausted := false
		if err != nil {
			attempt.Failure = Classify(err, secrets...)
			if attempt.Failure.status != nil {
				attempt.Status = attempt.Failure.status
			}
			if !released && !escaped && attempt.Failure.Retryable() && ctx.Err() == nil {
				if index < retries {
					delay := schedule.NextBackOff()
					attempt.RetryDelay = &delay
				} else {
					exhausted = true
				}
			}
		}
		if observe != nil {
			if observationErr := observe(attempt); observationErr != nil {
				return observationErr
			}
		}
		if contextErr := ctx.Err(); contextErr != nil {
			return Classify(contextErr)
		}
		if err == nil {
			if buffered && yield != nil {
				for _, part := range pending {
					if !yield(part) {
						return Classify(context.Canceled)
					}
				}
			}
			return nil
		}
		if attempt.RetryDelay == nil {
			if exhausted {
				// Observations retain the provider failure. Only the terminal
				// result names the exhausted recovery for the turn's final fault.
				failure := *attempt.Failure
				failure.Exhausted = true
				return &failure
			}
			return attempt.Failure
		}
		if err := timer.Sleep(ctx, *attempt.RetryDelay); err != nil {
			return Classify(err)
		}
	}
}
func outputPart(part fantasy.StreamPart) bool {
	if PluginOutput(part) {
		return true
	}
	switch part.Type {
	case fantasy.StreamPartTypeTextDelta:
		return part.Delta != ""
	case fantasy.StreamPartTypeToolInputStart, fantasy.StreamPartTypeToolInputDelta, fantasy.StreamPartTypeToolCall:
		return true
	case fantasy.StreamPartTypeReasoningDelta:
		if part.Delta != "" {
			return true
		}
		fallthrough
	case fantasy.StreamPartTypeReasoningEnd:
		for _, value := range part.ProviderMetadata {
			switch metadata := value.(type) {
			case *anthropic.ReasoningOptionMetadata:
				if metadata.Signature != "" || metadata.RedactedData != "" {
					return true
				}
			case *google.ReasoningMetadata:
				if metadata.Signature != "" {
					return true
				}
			case *openai.ResponsesReasoningMetadata:
				if metadata.EncryptedContent != nil || len(metadata.Summary) > 0 {
					return true
				}
			}
		}
		return false
	}
	return false
}
