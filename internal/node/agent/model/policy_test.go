package model

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"reflect"
	"strings"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type policyModel struct {
	fantasy.LanguageModel
	calls       int
	responses   [][]fantasy.StreamPart
	startErrors []error
	onCall      func(int)
}

func (model *policyModel) Stream(context.Context, fantasy.Call) (fantasy.StreamResponse, error) {
	index := model.calls
	model.calls++
	if model.onCall != nil {
		model.onCall(index)
	}
	if index < len(model.startErrors) && model.startErrors[index] != nil {
		return nil, model.startErrors[index]
	}
	if index >= len(model.responses) {
		return nil, errors.New("unexpected extra attempt")
	}
	return func(yield func(fantasy.StreamPart) bool) {
		for _, part := range model.responses[index] {
			if !yield(part) {
				return
			}
		}
	}, nil
}

type policyClock struct {
	now         time.Time
	delays      []time.Duration
	cancel      context.CancelFunc
	beforeSleep func()
}

func (clock *policyClock) Now() time.Time {
	clock.now = clock.now.Add(time.Millisecond)
	return clock.now
}
func (clock *policyClock) Sleep(ctx context.Context, duration time.Duration) error {
	if clock.beforeSleep != nil {
		clock.beforeSleep()
	}
	clock.delays = append(clock.delays, duration)
	clock.now = clock.now.Add(duration)
	if clock.cancel != nil {
		clock.cancel()
	}
	return ctx.Err()
}

func errorPart(kind FailureKind) fantasy.StreamPart {
	return fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &Failure{Kind: kind, Message: string(kind)}}
}
func finishPart() fantasy.StreamPart {
	return fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop}
}
func textPart(text string) fantasy.StreamPart {
	return fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, Delta: text}
}
func TestRetryTransient(t *testing.T) {
	for _, test := range []struct {
		name string
		body string
		wait time.Duration
	}{
		{"service unavailable", `{"error":{"code":"service_unavailable","message":"unavailable"}}`, time.Second},
		{"explicit overload", `{"error":{"code":"api_error","type":"overloaded_error","message":"busy"}}`, 5 * time.Second},
	} {
		t.Run(test.name, func(t *testing.T) {
			fake := &policyModel{responses: [][]fantasy.StreamPart{{{
				Type: fantasy.StreamPartTypeError, Error: &fantasy.ProviderError{StatusCode: 503, ResponseBody: []byte(test.body)},
			}}, {finishPart()}}}
			timer := &policyClock{}
			if err := Run(t.Context(), fake, fantasy.Call{}, Main, timer, nil, nil); err != nil || fake.calls != 2 {
				t.Fatalf("HTTP retry: calls=%d error=%v", fake.calls, err)
			}
			if len(timer.delays) != 1 || timer.delays[0] < test.wait*4/5 || timer.delays[0] > test.wait*6/5 {
				t.Fatalf("HTTP retry waits=%v, expected %v with jitter", timer.delays, test.wait)
			}
		})
	}
	for _, purpose := range []Purpose{Main, SummaryPurpose} {
		for _, kind := range []FailureKind{RateLimited, Network, Unavailable} {
			t.Run(string(purpose)+"_"+string(kind), func(t *testing.T) {
				fake := &policyModel{responses: [][]fantasy.StreamPart{{errorPart(kind)}, {textPart("result"), finishPart()}}}
				timer := &policyClock{}
				var observations []Attempt
				var output []fantasy.StreamPart
				err := Run(t.Context(), fake, fantasy.Call{}, purpose, timer, func(a Attempt) error { observations = append(observations, a); return nil }, func(part fantasy.StreamPart) bool { output = append(output, part); return true })
				if err != nil || fake.calls != 2 || len(observations) != 2 || len(output) != 2 {
					t.Fatalf("retry result: calls=%d observations=%d output=%d error=%v", fake.calls, len(observations), len(output), err)
				}
				// Main and summary requests use the same transient policy.
				want := time.Second
				if kind == RateLimited {
					want = 5 * time.Second
				}
				if len(timer.delays) != 1 || timer.delays[0] < want*4/5 || timer.delays[0] > want*6/5 {
					t.Errorf("retry delays = %v", timer.delays)
				}
				if observations[0].RetryDelay == nil || *observations[0].RetryDelay != timer.delays[0] || observations[1].RetryDelay != nil {
					t.Errorf("announced delays = %v, %v", observations[0].RetryDelay, observations[1].RetryDelay)
				}
				if observations[0].Failure == nil || observations[1].Failure != nil || observations[1].FirstToken == nil {
					t.Errorf("attempt facts = %+v", observations)
				}
				if observations[0].RequestID == "" || observations[0].RequestID != observations[1].RequestID || !observations[0].RequestStarted.Equal(observations[1].RequestStarted) || observations[1].Index != observations[0].Index+1 {
					t.Fatal("retry attempts lost their logical request identity")
				}
			})
		}
	}
	t.Run("main_exhausts_nine_retries", func(t *testing.T) {
		fake := &policyModel{}
		for i := 0; i < 11; i++ {
			fake.responses = append(fake.responses, []fantasy.StreamPart{errorPart(Network)})
		}
		timer := &policyClock{}
		var announced []*time.Duration
		err := Run(t.Context(), fake, fantasy.Call{}, Main, timer, func(attempt Attempt) error { announced = append(announced, attempt.RetryDelay); return nil }, nil)
		if err == nil || fake.calls != 10 || len(timer.delays) != 9 || len(announced) != 10 || announced[9] != nil {
			t.Fatalf("exhaustion: calls=%d delays=%v error=%v", fake.calls, timer.delays, err)
		}
		fault := Fault(err)
		if fault.Details.Kind != "model_recovery" || fault.Details.Data.(protocol.FaultDetailsModelRecoveryPayload).Reason != protocol.ConversationRecoveryStopAttemptsExhausted {
			t.Fatalf("exhaustion lost its recovery reason: %+v", fault)
		}
		intervals := []time.Duration{1, 2, 4, 8, 16, 30, 30, 30, 30}
		for index, delay := range timer.delays {
			base := intervals[index] * time.Second
			if delay < base*4/5 || delay > min(base*6/5, 30*time.Second) || *announced[index] != delay {
				t.Fatalf("delay %d = %v, announced %v", index, delay, *announced[index])
			}
		}
	})
	t.Run("summary_retries_as_a_main_request", func(t *testing.T) {
		fake := &policyModel{}
		for i := 0; i < 11; i++ {
			fake.responses = append(fake.responses, []fantasy.StreamPart{errorPart(Network)})
		}
		timer := &policyClock{}
		if err := Run(t.Context(), fake, fantasy.Call{}, SummaryPurpose, timer, nil, nil); err == nil || fake.calls != 10 || len(timer.delays) != 9 {
			t.Fatalf("summary: calls=%d delays=%v error=%v", fake.calls, timer.delays, err)
		}
		intervals := []time.Duration{1, 2, 4, 8, 16, 30, 30, 30, 30}
		for index, delay := range timer.delays {
			base := intervals[index] * time.Second
			if delay < base*4/5 || delay > min(base*6/5, 30*time.Second) {
				t.Fatalf("summary delay %d = %v", index, delay)
			}
		}
	})
	t.Run("summary_discards_failed_output", func(t *testing.T) {
		fake := &policyModel{responses: [][]fantasy.StreamPart{{textPart("discard"), errorPart(Network)}, {textPart("keep"), finishPart()}}}
		var texts []string
		err := Run(t.Context(), fake, fantasy.Call{}, SummaryPurpose, &policyClock{}, nil, func(part fantasy.StreamPart) bool {
			if part.Type == fantasy.StreamPartTypeTextDelta {
				texts = append(texts, part.Delta)
			}
			return true
		})
		if err != nil || !reflect.DeepEqual(texts, []string{"keep"}) {
			t.Fatalf("buffered output = %v, %v", texts, err)
		}
	})
	t.Run("sleep_cancellation", func(t *testing.T) {
		ctx, cancel := context.WithCancel(t.Context())
		defer cancel()
		fake := &policyModel{responses: [][]fantasy.StreamPart{{errorPart(Network)}}}
		timer := &policyClock{cancel: cancel}
		err := Run(ctx, fake, fantasy.Call{}, Main, timer, nil, nil)
		if Classify(err).Kind != Cancelled || fake.calls != 1 {
			t.Fatalf("cancellation = %v, calls=%d", err, fake.calls)
		}
	})
}

// Expiration is driven by the model call, not a real timer, so the request is
// already in flight when its parent deadline ends.
type policyDeadlineContext struct {
	context.Context
	done chan struct{}
}

func (ctx *policyDeadlineContext) Done() <-chan struct{} { return ctx.done }
func (ctx *policyDeadlineContext) Err() error {
	select {
	case <-ctx.done:
		return context.DeadlineExceeded
	default:
		return nil
	}
}

func TestExpiredParentDoesNotAnnounceRetry(t *testing.T) {
	for _, finalIndex := range []int{0, MainRetries} {
		t.Run(fmt.Sprintf("attempt_%d", finalIndex+1), func(t *testing.T) {
			ctx := &policyDeadlineContext{Context: t.Context(), done: make(chan struct{})}
			fake := &policyModel{onCall: func(index int) {
				if index == finalIndex {
					close(ctx.done)
				}
			}}
			for range MainRetries + 1 {
				fake.responses = append(fake.responses, []fantasy.StreamPart{errorPart(Network)})
			}
			timer := &policyClock{}
			var observed []Attempt
			err := Run(ctx, fake, fantasy.Call{}, Main, timer, func(attempt Attempt) error {
				observed = append(observed, attempt)
				return nil
			}, nil)
			if err == nil || Classify(err).Kind != Network || fake.calls != finalIndex+1 || len(timer.delays) != finalIndex || len(observed) != finalIndex+1 {
				t.Fatalf("expired parent: calls=%d waits=%v observed=%d error=%v", fake.calls, timer.delays, len(observed), err)
			}
			if observed[finalIndex].RetryDelay != nil || Fault(err).Details.Kind == "model_recovery" {
				t.Fatalf("expired parent announced another retry or exhaustion: %+v", observed[finalIndex])
			}
		})
	}
}

func TestRetryExhaustionRequiresUnescapedRetryableFailure(t *testing.T) {
	for _, scenario := range []string{"transient", "partial", "keepalive", "permanent", "canceled", "consumer_release", "observer_canceled", "summary_partial"} {
		t.Run(scenario, func(t *testing.T) {
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			fake := &policyModel{}
			for range MainRetries {
				fake.responses = append(fake.responses, []fantasy.StreamPart{errorPart(Network)})
			}
			last, purpose := []fantasy.StreamPart{errorPart(Network)}, Main
			switch scenario {
			case "partial", "consumer_release", "summary_partial":
				last = []fantasy.StreamPart{textPart("Visible only if unbuffered"), errorPart(Network)}
				if scenario == "summary_partial" {
					purpose = SummaryPurpose
				}
			case "keepalive":
				last = []fantasy.StreamPart{{Type: fantasy.StreamPartTypeKeepalive}, errorPart(Network)}
			case "permanent":
				last = []fantasy.StreamPart{errorPart(Authentication)}
			case "canceled":
				fake.onCall = func(index int) {
					if index == MainRetries {
						cancel()
					}
				}
			}
			fake.responses = append(fake.responses, last)
			timer := &policyClock{}
			var observed []Attempt
			err := Run(ctx, fake, fantasy.Call{}, purpose, timer, func(attempt Attempt) error {
				observed = append(observed, attempt)
				if scenario == "observer_canceled" && attempt.Index == MainRetries {
					cancel()
				}
				return nil
			}, func(fantasy.StreamPart) bool { return scenario != "consumer_release" })
			if err == nil || fake.calls != MainRetries+1 || len(timer.delays) != MainRetries || len(observed) != MainRetries+1 {
				t.Fatalf("unexpected recovery boundary: calls=%d waits=%v observed=%d error=%v", fake.calls, timer.delays, len(observed), err)
			}
			fault := Fault(err)
			exhausted := fault.Details.Kind == "model_recovery"
			if exhausted != (scenario == "transient" || scenario == "summary_partial") {
				t.Fatalf("%s reported the wrong exhaustion outcome: %+v", scenario, fault)
			}
			if exhausted && (observed[MainRetries].Failure.Kind != Network || observed[MainRetries].RetryDelay != nil || fault.Message != string(Network)) {
				t.Fatalf("exhaustion changed the underlying attempt or diagnostic: %+v, %+v", observed[MainRetries], fault)
			}
			if strings.Contains(scenario, "canceled") || scenario == "consumer_release" {
				if Classify(err).Kind != Cancelled {
					t.Fatalf("cancellation was replaced by %v", err)
				}
			}
		})
	}
}

func TestNoRetryAfterStreamEvent(t *testing.T) {
	for _, event := range []fantasy.StreamPart{textPart("partial"), {Type: fantasy.StreamPartTypeToolInputDelta, ID: "call", Delta: "{"}, {Type: fantasy.StreamPartTypeKeepalive}} {
		t.Run(string(event.Type), func(t *testing.T) {
			fake := &policyModel{responses: [][]fantasy.StreamPart{{event, errorPart(Network)}}}
			timer := &policyClock{}
			observed := 0
			err := Run(t.Context(), fake, fantasy.Call{}, Main, timer, func(Attempt) error { observed++; return nil }, func(fantasy.StreamPart) bool { return true })
			if err == nil || fake.calls != 1 || observed != 1 || len(timer.delays) != 0 {
				t.Fatalf("replayed escaped stream: calls=%d observed=%d delays=%v error=%v", fake.calls, observed, timer.delays, err)
			}
		})
	}
	t.Run("consumer_release", func(t *testing.T) {
		fake := &policyModel{responses: [][]fantasy.StreamPart{{textPart("partial"), finishPart()}}}
		var attempt Attempt
		err := Run(t.Context(), fake, fantasy.Call{}, Main, &policyClock{}, func(a Attempt) error { attempt = a; return nil }, func(fantasy.StreamPart) bool { return false })
		if Classify(err).Kind != Cancelled || attempt.Failure == nil || attempt.Failure.Kind != Cancelled || fake.calls != 1 {
			t.Fatalf("release = %v, attempt=%+v", err, attempt)
		}
	})
}
func TestPermanentErrorTerminal(t *testing.T) {
	for _, kind := range []FailureKind{NotConfigured, Authentication, ContextOverflow, Quota, InvalidRequest, InvalidResponse, Unsupported, Cancelled, Other} {
		t.Run(string(kind), func(t *testing.T) {
			fake := &policyModel{responses: [][]fantasy.StreamPart{{errorPart(kind)}}}
			timer := &policyClock{}
			err := Run(t.Context(), fake, fantasy.Call{}, Main, timer, nil, nil)
			if err == nil || fake.calls != 1 || len(timer.delays) != 0 {
				t.Fatalf("terminal error retried: %v", err)
			}
		})
	}
	t.Run("observation_failure", func(t *testing.T) {
		failure := errors.New("observation storage unavailable")
		fake := &policyModel{responses: [][]fantasy.StreamPart{{errorPart(Network)}}}
		timer := &policyClock{}
		err := Run(t.Context(), fake, fantasy.Call{}, Main, timer, func(Attempt) error { return failure }, nil)
		if !errors.Is(err, failure) || fake.calls != 1 || len(timer.delays) != 0 {
			t.Fatalf("observer failure = %v", err)
		}
	})
	t.Run("truncated", func(t *testing.T) {
		fake := &policyModel{responses: [][]fantasy.StreamPart{{textPart("partial")}}}
		err := Run(t.Context(), fake, fantasy.Call{}, Main, &policyClock{}, nil, func(fantasy.StreamPart) bool { return true })
		if Classify(err).Kind != InvalidResponse || fake.calls != 1 {
			t.Fatalf("truncated = %v", err)
		}
	})
}
func TestErrorMapping(t *testing.T) {
	sdkFailureFixtures(t)
	for _, row := range []struct {
		name   string
		status int
		code   string
		kind   FailureKind
	}{{"redirect", 302, "", InvalidRequest}, {"context", 400, "context_length_exceeded", ContextOverflow}, {"unauthorized", 401, "", Authentication}, {"forbidden", 403, "", Authentication}, {"auth_code", 400, "invalid_api_key", Authentication}, {"quota_before_429", 429, "insufficient_quota", Quota}, {"credits", 400, "credits_exhausted", Quota}, {"rate", 429, "", RateLimited}, {"stream_rate", 200, "rate_limit_error", RateLimited}, {"unavailable", 503, "", Unavailable}, {"overloaded", 200, "overloaded_error", Unavailable}, {"bad_request", 400, "", InvalidRequest}} {
		t.Run(row.name, func(t *testing.T) {
			raw, _ := json.Marshal(map[string]any{"error": map[string]any{"code": row.code, "message": "private-long and private"}})
			failure := Classify(&fantasy.ProviderError{Message: "failed", StatusCode: row.status, ResponseBody: raw, ResponseHeaders: map[string]string{"retry-after": "7"}}, protocol.NewSecret("private"), protocol.NewSecret("private-long"))
			if failure.Kind != row.kind || strings.Contains(failure.Message, "private") {
				t.Fatalf("classification = %+v", failure)
			}
			fault := Fault(failure)
			if fault == nil || fault.Details == nil {
				t.Fatal("missing protocol model failure")
			}
		})
	}
	for _, row := range []struct {
		err  error
		kind FailureKind
	}{{context.Canceled, Cancelled}, {io.ErrUnexpectedEOF, InvalidResponse}, {context.DeadlineExceeded, Network}} {
		if failure := Classify(row.err); failure.Kind != row.kind {
			t.Errorf("%v classified %s", row.err, failure.Kind)
		}
	}
	t.Run("nested_diagnostic", func(t *testing.T) {
		failure := Classify(&fantasy.ProviderError{StatusCode: 400, ResponseBody: []byte(`{"response":{"error":{"code":"invalid_request","message":"nested diagnostic"}}}`)})
		if !strings.Contains(failure.Message, "nested diagnostic") {
			t.Fatalf("diagnostic = %s", failure.Message)
		}
	})
}
func TestUsagePerAttempt(t *testing.T) {
	zero, first, second := int64(0), int64(11), int64(7)
	reported := func(input, output *int64) fantasy.StreamPart {
		part := finishPart()
		part.ProviderMetadata = fantasy.ProviderMetadata{UsageMetadataKey: &UsageMetadata{Reported: true, Precise: true, Input: input, Output: output}}
		return part
	}
	fake := &policyModel{responses: [][]fantasy.StreamPart{{{Type: fantasy.StreamPartTypeError, Error: &Failure{Kind: Network, Message: "temporary"}, ProviderMetadata: fantasy.ProviderMetadata{UsageMetadataKey: &UsageMetadata{Reported: true, Precise: true, Input: &first}}}}, {reported(&zero, &second)}}}
	var observations []Attempt
	err := Run(t.Context(), fake, fantasy.Call{}, Main, &policyClock{}, func(a Attempt) error { observations = append(observations, a); return nil }, func(fantasy.StreamPart) bool { return true })
	if err != nil || len(observations) != 2 {
		t.Fatalf("attempt usage: %v, %+v", err, observations)
	}
	if observations[0].Usage.Input == nil || *observations[0].Usage.Input != 11 || observations[0].Usage.Output != nil {
		t.Fatalf("partial failure usage = %+v", observations[0].Usage)
	}
	if observations[1].Usage.Input == nil || *observations[1].Usage.Input != 0 || observations[1].Usage.Output == nil || *observations[1].Usage.Output != 7 || observations[1].Usage.CacheRead != nil {
		t.Fatalf("success usage = %+v", observations[1].Usage)
	}
	if usage := PartUsage(finishPart()); usage.Input != nil || usage.Output != nil || usage.Total != nil {
		t.Fatalf("unknown usage became zero: %+v", usage)
	}
}
