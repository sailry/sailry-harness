//go:build integration

package link

import (
	"context"
	"io"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link/iroh"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

type routedFixtureHandler struct {
	resourceFixtureHandler
	open func(context.Context, protocol.NodeID, protocol.StreamID) (Stream, error)
}

func (handler routedFixtureHandler) Open(ctx context.Context, peer protocol.NodeID, stream protocol.StreamID) (Stream, error) {
	return handler.open(ctx, peer, stream)
}

type routedFixtureSubscription struct {
	node   protocol.NodeID
	first  bool
	closed chan struct{}
	once   sync.Once
}

func (subscription *routedFixtureSubscription) Next(ctx context.Context) (protocol.Update, error) {
	if !subscription.first {
		subscription.first = true
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = subscription.node
		batch := strings.Repeat("paged", wire.PageBytes)
		snapshot.ModelCatalog.Scores.Batch = &batch
		return protocol.Update{Kind: "snapshot", Data: snapshot}, nil
	}
	<-ctx.Done()
	return protocol.Update{}, ctx.Err()
}

func (subscription *routedFixtureSubscription) Close() error {
	subscription.once.Do(func() { close(subscription.closed) })
	return nil
}

func TestMultiHostRoutesCommandsSubscriptionsBytesAndReverseKnowledge(t *testing.T) {
	if err := iroh.LoadDefault(true); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), time.Minute)
	defer cancel()
	identity := func() Identity {
		value, err := OpenIdentity(t.TempDir())
		if err != nil {
			t.Fatal(err)
		}
		return value
	}
	aIdentity, bIdentity, cIdentity, phoneIdentity := identity(), identity(), identity(), identity()
	bind := func(id Identity, handler Handler) *Link {
		value, err := Bind(ctx, EndpointOptions{
			Identity: id, Scope: NetworkScope{Bind: "127.0.0.1:0"}, Handler: handler,
			Peers: &fixturePeerStore{trusted: map[protocol.NodeID]bool{}, addresses: map[protocol.NodeID]EndpointAddress{}},
		})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = value.Close() })
		return value
	}
	bSubscription := &routedFixtureSubscription{node: bIdentity.ID(), closed: make(chan struct{})}
	b := bind(bIdentity, routedFixtureHandler{
		resourceFixtureHandler: resourceFixtureHandler{resource: func(_ context.Context, caller protocol.NodeID, call protocol.ResourceCall) (Admission, error) {
			if caller != aIdentity.ID() || call.Request.Target != bIdentity.ID() {
				t.Error("B resource execution lost its controller or target")
			}
			done := make(chan Response, 1)
			done <- Response{Output: protocol.Output{Kind: "notifications_updated"}}
			return Admission{Receipt: protocol.Receipt{ID: call.Request.ID}, Completion: done}, nil
		}, fixtureHandler: fixtureHandler{
			dispatch: func(_ context.Context, caller protocol.NodeID, request protocol.Request) (Admission, error) {
				if caller != aIdentity.ID() || request.Target != bIdentity.ID() {
					t.Error("B executed under another controller or target")
				}
				done := make(chan Response, 1)
				done <- Response{Output: protocol.Output{Kind: "notifications_updated"}}
				return Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: done}, nil
			},
			subscribe: func(_ context.Context, caller protocol.NodeID, _ protocol.Topic) (Subscription, error) {
				if caller != aIdentity.ID() {
					t.Error("B subscription used the phone's identity")
				}
				return bSubscription, nil
			},
		}},
		open: func(_ context.Context, caller protocol.NodeID, _ protocol.StreamID) (Stream, error) {
			if caller != aIdentity.ID() {
				t.Error("B byte stream used the phone's identity")
			}
			client, server := StreamPair()
			go func() {
				defer server.Close()
				uploaded, err := io.ReadAll(server)
				if err != nil {
					t.Error(err)
					return
				}
				_, _ = io.WriteString(server, "stored:"+string(uploaded))
				_ = server.CloseWrite()
			}()
			return client, nil
		},
	})
	var a *Link
	var bAddress EndpointAddress
	a = bind(aIdentity, resourceFixtureHandler{
		fixtureHandler: fixtureHandler{},
		resource: func(ctx context.Context, caller protocol.NodeID, call protocol.ResourceCall) (Admission, error) {
			if caller != cIdentity.ID() {
				t.Error("A reverse resource callback lost the certified executor")
			}
			return a.Remote(bAddress).Resource(ctx, call)
		},
	})
	c := bind(cIdentity, fixtureHandler{})
	phone := bind(phoneIdentity, nil)
	pair := func(host, client *Link) EndpointAddress {
		offer, err := host.Invite(ctx)
		if err != nil {
			t.Fatal(err)
		}
		defer offer.Close()
		address, err := client.Pair(ctx, offer.Ticket())
		if err != nil {
			t.Fatal(err)
		}
		return address
	}
	bAddress = pair(b, a)
	aAddress := pair(a, phone)
	_ = pair(c, a)
	remote := phone.Via(aAddress, bAddress.ID)
	if remote.Route() != (protocol.RouteKey{Entry: aAddress.ID, Target: bAddress.ID}) {
		t.Fatal("relay substituted the actual resource identity")
	}
	request := protocol.NewRequest(bAddress.ID, protocol.Command{Kind: "snapshot"})
	admission, err := remote.Dispatch(ctx, request)
	if err != nil || admission.Receipt.ID != request.ID {
		t.Fatalf("phone to A to B admission: %#v %v", admission, err)
	}
	if result := <-admission.Completion; result.Fault != nil {
		t.Fatal(result.Fault)
	}
	subscription, err := remote.Subscribe(ctx, protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	update, err := subscription.Next(ctx)
	if err != nil || update.Kind != "snapshot" || update.Data.(*protocol.Snapshot).Node != bAddress.ID {
		t.Fatalf("relayed paged snapshot: %#v %v", update.Kind, err)
	}
	if err := subscription.Close(); err != nil {
		t.Fatal(err)
	}
	select {
	case <-bSubscription.closed:
	case <-ctx.Done():
		t.Fatal("closing the phone subscription did not release B's observation")
	}
	stream, err := remote.Open(ctx, protocol.NewStreamID())
	if err != nil {
		t.Fatal(err)
	}
	defer stream.Close()
	if _, err := io.WriteString(stream, "upload"); err != nil {
		t.Fatal(err)
	}
	if err := stream.CloseWrite(); err != nil {
		t.Fatal(err)
	}
	acknowledgement, err := io.ReadAll(stream)
	if err != nil || string(acknowledgement) != "stored:upload" {
		t.Fatalf("relay lost upload half-close or publication acknowledgment: %q %v", acknowledgement, err)
	}
	reverse := protocol.ResourceCall{Request: protocol.NewRequest(bAddress.ID, protocol.Command{Kind: "list_knowledge_collections"})}
	resource, err := c.Remote(aAddress).Resource(ctx, reverse)
	if err != nil || resource.Receipt.ID != reverse.Request.ID {
		t.Fatalf("C to A to B resource route: %#v %v", resource, err)
	}
	if result := <-resource.Completion; result.Fault != nil {
		t.Fatal(result.Fault)
	}
	if _, err := c.Remote(aAddress).Dispatch(ctx, protocol.NewRequest(aAddress.ID, protocol.Command{Kind: "snapshot"})); err == nil {
		t.Fatal("limited reverse resource routing enabled ordinary reverse commands")
	}
}
