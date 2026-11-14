package model

import (
	"io"
	"net/http"
	"sync"

	"github.com/openai/openai-go/v3/packages/ssestream"
)

// usageObserverBody leaves the main SDK's bytes untouched. The existing official
// SDK decoder observes only aggregate usage facts on a separate pipe; it never
// stores output, runs a second model request or changes the primary parser.
type usageObserverBody struct {
	body      io.ReadCloser
	writer    *io.PipeWriter
	done      chan struct{}
	closeOnce sync.Once
	closeErr  error
	mu        sync.Mutex
	usage     UsageMetadata
}

func observeUsage(response *http.Response, family string) *usageObserverBody {
	reader, writer := io.Pipe()
	observed := &usageObserverBody{body: response.Body, writer: writer, done: make(chan struct{}), usage: UsageMetadata{Precise: true}}
	decoder := ssestream.NewDecoder(&http.Response{Header: response.Header.Clone(), Body: reader})
	go func() {
		defer close(observed.done)
		defer decoder.Close()
		accumulator := sdkEmitter{}
		accumulator.usage.Precise = true
		for decoder.Next() {
			value, err := decodeSDK(string(decoder.Event().Data))
			if err != nil {
				continue
			}
			var usage map[string]any
			if family == "google" {
				usage = wireObject(value["usageMetadata"])
			} else {
				if wireString(value["type"]) == "message_start" {
					usage = wireObject(wireObject(value["message"])["usage"])
				} else {
					usage = wireObject(value["usage"])
				}
			}
			if usage == nil {
				continue
			}
			accumulator.updateUsage(usage, family)
			observed.mu.Lock()
			observed.usage = accumulator.usage
			observed.mu.Unlock()
		}
	}()
	return observed
}

func (body *usageObserverBody) Read(buffer []byte) (int, error) {
	count, err := body.body.Read(buffer)
	if count > 0 {
		// Writes wait only for the independent SDK decoder to consume bytes.
		// There is no event handoff back to the main SDK and no per-chunk join.
		body.writer.Write(buffer[:count])
	}
	if err != nil {
		body.writer.CloseWithError(err)
	}
	return count, err
}

func (body *usageObserverBody) Close() error {
	body.closeOnce.Do(func() {
		body.closeErr = body.body.Close()
		body.writer.Close()
		<-body.done
	})
	return body.closeErr
}

func (body *usageObserverBody) snapshot() UsageMetadata {
	body.mu.Lock()
	defer body.mu.Unlock()
	return body.usage
}
