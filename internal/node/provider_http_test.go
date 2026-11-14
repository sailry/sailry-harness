package node

import (
	"context"
	"errors"
	"net/http"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type providerWaitingContext struct {
	context.Context
	waiting chan struct{}
	once    sync.Once
}

func (ctx *providerWaitingContext) Done() <-chan struct{} {
	ctx.once.Do(func() { close(ctx.waiting) })
	return ctx.Context.Done()
}

func TestProviderResourceWaiterFetchesAfterAnotherCallersFailure(t *testing.T) {
	node := &Node{auth: &authControls{resources: map[[32]byte]*providerDocument{}}}
	key := [32]byte{1}
	started, release, first := make(chan struct{}), make(chan struct{}), make(chan error, 1)
	var releaseOnce sync.Once
	unblock := func() { releaseOnce.Do(func() { close(release) }) }
	t.Cleanup(unblock)
	go func() {
		_, err := node.providerResourceCached(t.Context(), key, time.Minute, func() (protocol.PluginHttpResponse, error) {
			close(started)
			<-release
			return protocol.PluginHttpResponse{}, errors.New("first caller went away")
		})
		first <- err
	}()
	<-started
	type outcome struct {
		response protocol.PluginHttpResponse
		err      error
	}
	second := make(chan outcome, 1)
	ctx := &providerWaitingContext{Context: t.Context(), waiting: make(chan struct{})}
	go func() {
		response, err := node.providerResourceCached(ctx, key, time.Minute, func() (protocol.PluginHttpResponse, error) {
			return protocol.PluginHttpResponse{Status: http.StatusOK}, nil
		})
		second <- outcome{response, err}
	}()
	// Observe the wait itself; successive time.Now calls can be equal.
	select {
	case <-ctx.waiting:
	case got := <-second:
		t.Fatalf("second caller returned before waiting: %+v, %v", got.response, got.err)
	}
	unblock()
	if err := <-first; err == nil {
		t.Fatal("first caller lost its own failure")
	}
	if got := <-second; got.err != nil || got.response.Status != http.StatusOK {
		t.Fatalf("waiting caller = %+v, %v; want its own fetch", got.response, got.err)
	}
}
