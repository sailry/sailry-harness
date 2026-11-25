package model

import (
	"context"
	"io"
	"time"

	"charm.land/fantasy"
	"charm.land/fantasy/providers/anthropic"
	"charm.land/fantasy/providers/google"
	"charm.land/fantasy/providers/openai"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/clock"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type Purpose string

const (
	Main           Purpose = "main"
	SummaryPurpose Purpose = "summary"
	// NativePurpose is a route's own compaction. Sailry's summary can take
	// its place, so it resends once and then leaves the work to that summary.
	NativePurpose Purpose = "native"
)

type Attempt struct {
	RequestID      string
	RequestStarted time.Time
	Index          uint64
	Started        time.Time
	Finished       time.Time
	FirstToken     *time.Time
	Usage          canonical.Usage
	// Context is what the last request of a reply that took several
	// occupied; nil when Usage describes one request.
	Context *uint64
	Failure *Failure
	// RetryLimit is the last resend allowed if the current failure streak continues.
	RetryLimit uint64
	// RetryDelay is the wait before the next attempt; nil on the last attempt.
	RetryDelay        *time.Duration
	Status            *uint16
	ResponseUs        *uint64
	RequestBytes      *uint64
	DispatchUs        *uint64
	RequestSentUs     *uint64
	Categories        []protocol.ConversationObservationCategory
	CompleteContext   bool
	ReasoningStreamed bool
	ResponsesBoundary *protocol.ConversationResponsesBoundary
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

// MainRetries bounds resends of one main or summary request. Both also share
// the request's cumulative wait budget, regardless of the transient cause.
const MainRetries = 10

// Run uses reset to discard provisional output before replaying an interrupted attempt.
// Without it, main output that escaped to the consumer cannot be retried safely.
func Run(ctx context.Context, languageModel fantasy.LanguageModel, call fantasy.Call, purpose Purpose, timer clock.Clock, observe func(Attempt) error, yield func(fantasy.StreamPart) bool, reset func()) error {
	buffered := purpose == SummaryPurpose || purpose == NativePurpose
	schedule := newRetryPolicy()
	var observation requestObservation
	requestID, requestStarted := protocol.NewRequestID().String(), timer.Now()
	sdk, _ := languageModel.(*sdkModel)
	nextModel := languageModel
	var releaseFallback func()
	var secrets []protocol.Secret
	if sdk != nil {
		secrets = append(secrets, sdk.config.Protection...)
		if sdk.config.Key != "" {
			secrets = append(secrets, protocol.NewSecret(sdk.config.Key))
		}
	}
	for index := uint64(0); ; index++ {
		if err := ctx.Err(); err != nil {
			return Classify(err)
		}
		attempt := Attempt{RequestID: requestID, RequestStarted: requestStarted, Index: index, Started: timer.Now(), RetryLimit: MainRetries}
		owner, cancel := context.WithCancel(ctx)
		firstOutputTimer := schedule.firstOutputTimer(attempt.Started, cancel)
		owner, capture := captureAttempt(owner, call, timer, attempt.Started)
		stream, err := nextModel.Stream(owner, call)
		var pending []fantasy.StreamPart
		streamed, escaped, finished, released := false, false, false, false
		if err == nil && stream != nil {
			for part := range stream {
				attempt.Usage = mergeUsage(attempt.Usage, PartUsage(part))
				if metadata, ok := part.ProviderMetadata[UsageMetadataKey].(*UsageMetadata); ok && metadata.Context != nil && *metadata.Context >= 0 {
					context := uint64(*metadata.Context)
					attempt.Context = &context
				}
				if part.Type == fantasy.StreamPartTypeReasoningDelta && part.Delta != "" {
					attempt.ReasoningStreamed = true
				}
				if metadata, ok := part.ProviderMetadata[responseMetadataKey].(*responseMetadata); ok {
					attempt.Status = metadata.Status
				}
				firstOutput := attempt.FirstToken == nil && firstTokenPart(part)
				if firstOutputTimer != nil && (firstOutput || part.Type == fantasy.StreamPartTypeFinish) {
					if !firstOutputTimer.Stop() {
						break
					}
					firstOutputTimer = nil
				}
				if firstOutput {
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
				streamed = true
				if part.Type == fantasy.StreamPartTypeFinish {
					finished = true
				}
				if buffered {
					pending = append(pending, part)
				} else {
					escaped = escaped || yield != nil && outputPart(part)
					if yield != nil && !yield(part) {
						released = true
						err = context.Canceled
						break
					}
				}
			}
		}
		waitExhausted := firstOutputTimer != nil && !firstOutputTimer.Stop() && !released
		if err == nil && !finished {
			err = io.ErrUnexpectedEOF
		}
		if ctx.Err() != nil {
			err, waitExhausted = ctx.Err(), false
		}
		cancel()
		if releaseFallback != nil {
			releaseFallback()
			nextModel, releaseFallback = languageModel, nil
		}
		attempt.Finished = timer.Now()
		capture.apply(&attempt)
		exhausted := false
		if err != nil {
			attempt.Failure = Classify(err, capture.redact(secrets)...)
			if waitExhausted {
				if attempt.Failure.Kind == Cancelled || attempt.Failure.truncated {
					attempt.Failure = &Failure{Kind: Network, Message: "Model retry timed out waiting for output after HTTP 524"}
				}
				waitExhausted = attempt.Failure.Retryable()
			}
			if attempt.Failure.status != nil {
				attempt.Status = attempt.Failure.status
			}
			committed := finished && !buffered && yield != nil
			canReplay := !released && !committed && (!escaped || reset != nil)
			if !waitExhausted && canReplay && attempt.Failure.Retryable() && ctx.Err() == nil {
				attempt.RetryLimit = schedule.limit(purpose, attempt, streamed)
				if index >= attempt.RetryLimit {
					exhausted = true
				} else if delay, allowed := schedule.next(ctx, attempt, timer.Now()); allowed {
					attempt.RetryDelay = &delay
				} else {
					waitExhausted = true
				}
			}
		}
		observation.record(attempt)
		if observe != nil {
			if observationErr := observe(attempt); observationErr != nil {
				return observationErr
			}
		}
		if attempt.RetryDelay != nil && reset != nil {
			// The observer may have durably replaced the old draft. Release
			// its local output even if cancellation arrived during that commit.
			reset()
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
			observation.finish(ctx, false)
			return nil
		}
		if attempt.RetryDelay != nil {
			ready, waitErr := schedule.wait(ctx, timer, attempt)
			if waitErr != nil {
				return Classify(waitErr)
			}
			waitExhausted = !ready
		}
		if exhausted || waitExhausted || attempt.RetryDelay == nil {
			// Observations retain the provider failure. Only the terminal
			// result names the exhausted recovery for the turn's final fault.
			failure := *attempt.Failure
			if exhausted || waitExhausted {
				failure.Exhausted, failure.WaitExhausted = exhausted, waitExhausted
			}
			failure.requestObserved = observation.finish(ctx, true)
			return &failure
		}
		if sdk != nil && attempt.Failure.Kind == Network && http2ProtocolFailure(err) {
			// Spend one admitted retry on HTTP/1.1 without pinning an HTTP/2-only origin to it.
			if fallback, release := sdk.http1Fallback(); fallback != nil {
				nextModel, releaseFallback = fallback, release
			}
			sdk = nil
		}
	}
}

// A call ID or an empty arguments frame can establish provisional stream state
// for retry purposes before the provider delivers any model output.
func firstTokenPart(part fantasy.StreamPart) bool {
	switch part.Type {
	case fantasy.StreamPartTypeTextDelta, fantasy.StreamPartTypeReasoningDelta:
		return part.Delta != ""
	case fantasy.StreamPartTypeToolInputStart:
		return part.ToolCallName != ""
	case fantasy.StreamPartTypeToolInputDelta:
		return part.Delta != ""
	case fantasy.StreamPartTypeToolCall:
		return part.ToolCallName != "" || part.ToolCallInput != ""
	case fantasy.StreamPartTypeKeepalive:
		return PluginOutput(part) && (part.ToolCallName != "" || part.Delta != "")
	default:
		return false
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
