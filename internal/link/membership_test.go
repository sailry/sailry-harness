package link

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type membershipFixtureStore struct {
	*fixturePeerStore
	scope InvitationScope
}

func (store *membershipFixtureStore) PairMember(ctx context.Context, id protocol.NodeID, direction PeerDirection, scope InvitationScope) error {
	store.scope = scope
	return store.PairPeer(ctx, id, direction)
}
func (*membershipFixtureStore) Relationships(context.Context) ([]PeerRelationship, error) {
	return nil, nil
}
func (*membershipFixtureStore) ApplyDirectory(context.Context, protocol.NodeID, uint64, []EndpointAddress, ...map[protocol.NodeID]string) error {
	return nil
}

func TestMyDeviceProofComesFromCapturedOfferAndAuthenticatedAcknowledgment(t *testing.T) {
	t.Parallel()
	link := newFixtureLink(t)
	store := &membershipFixtureStore{fixturePeerStore: link.store.(*fixturePeerStore)}
	link.store = store
	user := protocol.NewUserID()
	offer := &pairingOffer{scope: InvitationScope{Kind: protocol.InvitationMyDevice, User: &user}, token: pairingToken{9}, deadline: link.now().Add(invitationLifetime), stop: make(chan struct{}), paired: make(chan struct{})}
	link.offer = offer
	device := protocol.NodeID{2}
	acknowledged, err := link.acceptPairingScope(t.Context(), offer.token, device, EndpointAddress{ID: device})
	if err != nil || acknowledged.Kind != protocol.InvitationMyDevice || store.scope.User == nil || *store.scope.User != user {
		t.Fatalf("captured scope lost: %#v %v", acknowledged, err)
	}
	claim, err := json.Marshal(pairingClaim{Token: offer.token, Address: EndpointAddress{ID: device}})
	if err != nil {
		t.Fatal(err)
	}
	for _, field := range []string{`,"user":"forged"}`, `,"kind":"my_device"}`} {
		var forged pairingClaim
		if json.Unmarshal(append(append([]byte{}, claim[:len(claim)-1]...), []byte(field)...), &forged) == nil {
			t.Fatal("claimant supplied the sharer's membership scope")
		}
	}
	for _, test := range []struct {
		body string
		want bool
	}{{`{"Ok":null}`, false}, {`{"Ok":null,"my_device":true}`, true}} {
		var result pairingResult
		if err := json.Unmarshal([]byte(test.body), &result); err != nil || result.MyDevice != test.want {
			t.Fatalf("authenticated acknowledgment: %s %v", test.body, err)
		}
	}
	for _, body := range []string{`{"Ok":null,"my_device":null}`, `{"Ok":null,"my_device":true,"my_device":false}`, `{"Ok":null,"Ok":null,"my_device":true}`} {
		var result pairingResult
		if json.Unmarshal([]byte(body), &result) == nil {
			t.Fatalf("ambiguous membership proof accepted: %s", body)
		}
	}
}
