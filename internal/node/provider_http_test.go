package node

import (
	"errors"
	"net/http"
	"runtime"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProviderResourceWaiterFetchesAfterAnotherCallersFailure(t *testing.T) {
	node := &Node{auth: &authControls{resources: map[[32]byte]*providerDocument{}}}
	key := [32]byte{1}
	started, release, first := make(chan struct{}), make(chan struct{}), make(chan error, 1)
	go func() {
		_, err := node.providerResourceCached(t.Context(), key, time.Minute, func() (protocol.PluginHttpResponse, error) {
			close(started)
			<-release
			return protocol.PluginHttpResponse{}, errors.New("first caller went away")
		})
		first <- err
	}()
	<-started
	node.auth.mu.Lock()
	document := node.auth.resources[key]
	used := document.used
	node.auth.mu.Unlock()
	type outcome struct {
		response protocol.PluginHttpResponse
		err      error
	}
	second := make(chan outcome, 1)
	go func() {
		response, err := node.providerResourceCached(t.Context(), key, time.Minute, func() (protocol.PluginHttpResponse, error) {
			return protocol.PluginHttpResponse{Status: http.StatusOK}, nil
		})
		second <- outcome{response, err}
	}()
	// The second caller marks the document used on its way to waiting.
	for waiting := false; !waiting; runtime.Gosched() {
		node.auth.mu.Lock()
		waiting = document.used != used
		node.auth.mu.Unlock()
	}
	close(release)
	if err := <-first; err == nil {
		t.Fatal("first caller lost its own failure")
	}
	if got := <-second; got.err != nil || got.response.Status != http.StatusOK {
		t.Fatalf("waiting caller = %+v, %v; want its own fetch", got.response, got.err)
	}
}
