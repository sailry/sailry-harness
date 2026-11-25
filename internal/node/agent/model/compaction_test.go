package model

import (
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestNativeRejectionFollowsProviderRevision(t *testing.T) {
	binding := Binding{Provider: protocol.ConversationProvider{ID: protocol.NewProviderID(), Revision: 3}}
	const route = "anthropic\x00https://gateway.invalid\x00fixture"
	if NativeRejected(binding, route) {
		t.Fatal("an unseen route was rejected")
	}
	rejectNative(binding, route)
	if !NativeRejected(binding, route) || NativeRejected(binding, route+"-other") {
		t.Fatal("the rejection was not remembered for its route alone")
	}
	edited := binding
	edited.Provider.Revision++
	if NativeRejected(edited, route) {
		t.Fatal("an edited channel kept the rejection")
	}
	if NativeRejected(binding, route) {
		t.Fatal("the stale rejection was not forgotten")
	}
}

func TestNativeRejectionsAreBounded(t *testing.T) {
	binding := Binding{Provider: protocol.ConversationProvider{ID: protocol.NewProviderID()}}
	for index := range nativeRejectionCapacity + 10 {
		rejectNative(binding, fmt.Sprint("route-", index))
	}
	nativeRejections.Lock()
	size := len(nativeRejections.entries)
	nativeRejections.Unlock()
	if size > nativeRejectionCapacity {
		t.Fatalf("remembered %d rejections, capacity %d", size, nativeRejectionCapacity)
	}
}
