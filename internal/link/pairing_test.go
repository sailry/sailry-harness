package link

import (
	"bytes"
	"context"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link/peertransport"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

func TestPairingTicketMatchesRustAndBounds(t *testing.T) {
	t.Parallel()
	now := time.Unix(100, 0)
	ticket := pairingTicket{
		Version: 1, Address: EndpointAddress{ID: protocol.NodeID{1}},
		Token: pairingToken{1, 2, 3}, ExpiresAtMS: uint64(now.Add(invitationLifetime).UnixMilli()),
	}
	encoded, err := json.Marshal(ticket)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(encoded), `"token":[1,2,3,0`) || !strings.Contains(string(encoded), `"id":"010000`) {
		t.Fatal("ticket lost Rust numeric token or hex endpoint identity")
	}
	local := protocol.NodeID{2}
	decoded, err := decodePairingTicket(string(encoded), local)
	if err != nil || decoded.Token != ticket.Token || decoded.Address.ID != ticket.Address.ID {
		t.Fatalf("valid ticket: %#v, %v", decoded, err)
	}
	// The sharer's clock is not the claimant's. A Host whose clock is ahead
	// issues a ticket that expires more than its lifetime after the
	// claimant's time (7.4 s were observed), and one whose clock is behind
	// issues a ticket that looks expired. Only the sharer, which holds the
	// deadline and accepts the token once, decides.
	for name, skew := range map[string]time.Duration{"sharer_ahead": 7400 * time.Millisecond, "sharer_far_ahead": time.Hour, "sharer_behind": -2 * time.Minute} {
		t.Run(name, func(t *testing.T) {
			skewed := ticket
			skewed.ExpiresAtMS = uint64(now.Add(skew + invitationLifetime).UnixMilli())
			encoded, err := json.Marshal(skewed)
			if err != nil {
				t.Fatal(err)
			}
			if decoded, err := decodePairingTicket(string(encoded), local); err != nil || decoded.Token != ticket.Token {
				t.Fatalf("a ticket from a Host with another clock was refused: %v", err)
			}
		})
	}
	for _, test := range []struct {
		name  string
		field string
		value any
	}{
		{"no_expiry", "expires_at_ms", 0},
		{"wrong_version", "version", 2},
		{"base64_token", "token", "AQID"},
		{"short_token", "token", make([]uint16, 31)},
		{"long_token", "token", make([]uint16, 33)},
		{"null_token", "token", nil},
		{"negative_token", "token", append([]int{-1}, make([]int, 31)...)},
		{"oversized_token_byte", "token", append([]int{256}, make([]int, 31)...)},
		{"unknown_field", "unexpected", true},
	} {
		t.Run(test.name, func(t *testing.T) {
			var fields map[string]any
			if err := json.Unmarshal(encoded, &fields); err != nil {
				t.Fatal(err)
			}
			fields[test.field] = test.value
			modified, err := json.Marshal(fields)
			if err != nil {
				t.Fatal(err)
			}
			if _, err := decodePairingTicket(string(modified), local); err == nil {
				t.Fatal("invalid pairing ticket was accepted")
			}
		})
	}
	t.Run("self_pairing", func(t *testing.T) {
		if _, err := decodePairingTicket(string(encoded), ticket.Address.ID); err == nil {
			t.Fatal("self pairing was accepted")
		}
	})
	t.Run("too_many_addresses", func(t *testing.T) {
		ticket.Address.Addrs = make([]json.RawMessage, 17)
		for index := range ticket.Address.Addrs {
			ticket.Address.Addrs[index] = json.RawMessage(`{"Ip":"127.0.0.1:1"}`)
		}
		modified, err := json.Marshal(ticket)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := decodePairingTicket(string(modified), local); err == nil {
			t.Fatal("oversized address set was accepted")
		}
	})
	t.Run("ticket_size", func(t *testing.T) {
		if _, err := decodePairingTicket(strings.Repeat(" ", 8193), local); err == nil {
			t.Fatal("oversized ticket was accepted")
		}
	})
	t.Run("duplicate_fields", func(t *testing.T) {
		modified := strings.Replace(string(encoded), `"version":1`, `"version":1,"version":1`, 1)
		if _, err := decodePairingTicket(modified, local); err == nil {
			t.Fatal("duplicate pairing ticket field was accepted")
		}
	})
}

type fixturePairEndpoint struct {
	peertransport.Endpoint
	local      EndpointAddress
	dialed     EndpointAddress
	alpn       string
	connection peertransport.Connection
	onlineErr  error
	addresses  int
}

func (endpoint *fixturePairEndpoint) Online(context.Context) error { return endpoint.onlineErr }

func (endpoint *fixturePairEndpoint) Address(context.Context) (EndpointAddress, error) {
	endpoint.addresses++
	return endpoint.local, nil
}

func (endpoint *fixturePairEndpoint) Connect(_ context.Context, address EndpointAddress, alpn string) (peertransport.Connection, error) {
	endpoint.dialed, endpoint.alpn = address, alpn
	return endpoint.connection, nil
}

func TestInviteRequiresReadyAddress(t *testing.T) {
	service := newFixtureLink(t)
	endpoint := &fixturePairEndpoint{local: EndpointAddress{ID: service.nodeID}, onlineErr: peertransport.ErrClosed}
	service.endpoint = endpoint
	if _, err := service.Invite(t.Context()); faultValue(err).Code != protocol.ErrorCodeUnavailable || endpoint.addresses != 0 || service.offer != nil {
		t.Fatalf("unready endpoint produced an invitation: %v", err)
	}
	endpoint.onlineErr = nil
	if _, err := service.Invite(t.Context()); faultValue(err).Code != protocol.ErrorCodeUnavailable || endpoint.addresses != 1 || service.offer != nil {
		t.Fatalf("unadvertised endpoint produced an invitation: %v", err)
	}
	endpoint.local.Addrs = []json.RawMessage{json.RawMessage(`{"Libp2p":"/ip4/127.0.0.1/tcp/4003"}`)}
	invitation, err := service.Invite(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	defer invitation.Close()
	ticket, err := decodePairingTicket(invitation.Ticket(), protocol.NodeID{1})
	if err != nil || !addressIncludes(ticket.Address, endpoint.local) {
		t.Fatalf("ready address not advertised: %#v, %v", ticket.Address, err)
	}
}

func TestPairingReplacesLegacyHintsWithoutChangingSelection(t *testing.T) {
	ctx, service := t.Context(), newFixtureLink(t)
	service.nodeID = protocol.NodeID{1}
	selection := fixtureRelaySelection(fixtureRelayTCP)
	service.selection = &selection
	legacy := EndpointAddress{ID: protocol.NodeID{2}, Addrs: []json.RawMessage{json.RawMessage(`{"Relay":"https://relay.example/"}`)}}
	if err := service.SetTrust(ctx, legacy.ID, true); err != nil {
		t.Fatal(err)
	}
	if err := service.remember(ctx, legacy); err != nil {
		t.Fatal(err)
	}
	ticket := func(address EndpointAddress) string {
		t.Helper()
		encoded, err := json.Marshal(pairingTicket{Version: 1, Address: address, Token: pairingToken{1}, ExpiresAtMS: 1})
		if err != nil {
			t.Fatal(err)
		}
		return string(encoded)
	}
	_, inspectErr := service.Inspect(ctx, legacy)
	_, pairErr := service.Pair(ctx, ticket(legacy))
	for _, err := range []error{inspectErr, pairErr, service.UpdatePeer(ctx, legacy)} {
		if fault := faultValue(err); fault.Code != protocol.ErrorCodeUnavailable || !strings.Contains(fault.Message, "Iroh") {
			t.Fatalf("legacy-only route did not fail explicitly: %v", err)
		}
	}
	peers, err := service.Peers(ctx)
	if err != nil || len(peers) != 1 || !addressIncludes(peers[0], legacy) || !service.Direction(legacy.ID).AllowsOutbound() {
		t.Fatalf("failed legacy dial changed trust or routes: %#v, %v", peers, err)
	}

	modern := EndpointAddress{ID: legacy.ID, Addrs: []json.RawMessage{json.RawMessage(`{"Libp2p":"` + fixtureRelayQUIC + `/p2p-circuit"}`)}}
	var response bytes.Buffer
	hasNode := true
	if err := wire.Write(&response, pairingResult{HasNode: &hasNode}, false); err != nil {
		t.Fatal(err)
	}
	stream := newFixtureStream(response.Bytes())
	stream.stopOnce.Do(func() { close(stream.stopped) })
	endpoint := &fixturePairEndpoint{
		local:      EndpointAddress{ID: service.nodeID, Addrs: []json.RawMessage{json.RawMessage(`{"Libp2p":"/ip4/127.0.0.1/tcp/4003"}`)}},
		connection: &fixtureConnection{info: peertransport.ConnectionInfo{RemoteID: modern.ID, ALPN: wire.PairALPN}, stream: stream},
		onlineErr:  peertransport.ErrClosed,
	}
	service.endpoint = endpoint
	paired, err := service.Pair(ctx, ticket(modern))
	if err != nil || !addressIncludes(paired, modern) || !addressIncludes(endpoint.dialed, modern) || endpoint.alpn != wire.PairALPN || !stream.finished {
		t.Fatalf("re-pair on the Host's circuit = %#v, dialed %#v, %v", paired, endpoint.dialed, err)
	}
	peers, err = service.Peers(ctx)
	if err != nil || len(peers) != 1 || !addressIncludes(peers[0], modern) || addressIncludes(peers[0], legacy) || !service.Direction(legacy.ID).AllowsOutbound() {
		t.Fatalf("re-pair did not update the same trusted peer: %#v, %v", peers, err)
	}
	relays, err := NetworkRelayURLs(ctx, *service.NetworkSelection())
	if err != nil || len(relays) != 1 || relays[0] != fixtureRelayTCP {
		t.Fatalf("raw pairing changed the claimant's selection: %v, %v", relays, err)
	}
}

func TestPairingHasOneCertifiedClaimant(t *testing.T) {
	t.Parallel()
	link := newFixtureLink(t)
	offer := &pairingOffer{
		token: pairingToken{7}, deadline: link.now().Add(invitationLifetime),
		stop: make(chan struct{}), paired: make(chan struct{}),
	}
	link.offer = offer
	first, second := protocol.NodeID{1}, protocol.NodeID{2}
	type result struct {
		peer protocol.NodeID
		err  error
	}
	start := make(chan struct{})
	finished := make(chan result, 2)
	for _, peer := range []protocol.NodeID{first, second} {
		go func() {
			<-start
			finished <- result{peer, acceptLegacyPairing(t, link, offer.token, peer)}
		}()
	}
	close(start)
	var winner protocol.NodeID
	accepted := 0
	for range 2 {
		result := <-finished
		if result.err == nil {
			accepted++
			winner = result.peer
		}
	}
	if accepted != 1 || !channelClosed(offer.paired) {
		t.Fatalf("accepted %d pairing claimants", accepted)
	}
	peers, err := link.Peers(t.Context())
	if err != nil || len(peers) != 1 || peers[0].ID != winner {
		t.Fatalf("persisted pairing: %#v, %v", peers, err)
	}
	if err := acceptLegacyPairing(t, link, offer.token, winner); err != nil {
		t.Fatalf("certified winner could not retry its valid ticket: %v", err)
	}
	if link.Direction(winner) != DirectionController {
		t.Fatal("accepting a claimant granted reverse execution access")
	}
	if err := link.SetTrust(t.Context(), winner, false); err != nil {
		t.Fatal(err)
	}
	if err := acceptLegacyPairing(t, link, offer.token, winner); err == nil {
		t.Fatal("revoked peer reused its completed invitation")
	}
	t.Run("connections_need_an_open_invitation_with_a_free_place", func(t *testing.T) {
		link := newFixtureLink(t)
		if link.admitPairing() != nil {
			t.Fatal("pairing connection admitted without an invitation")
		}
		offer := &pairingOffer{
			token: pairingToken{7}, deadline: link.now().Add(invitationLifetime),
			stop: make(chan struct{}), paired: make(chan struct{}),
		}
		link.offer = offer
		for index := range maxPairings {
			if link.admitPairing() != offer {
				t.Fatalf("open invitation refused claimant %d", index)
			}
		}
		if link.admitPairing() != nil {
			t.Fatal("one invitation occupied more than its accept workers")
		}
		offer.claims--
		if link.admitPairing() != offer {
			t.Fatal("released place was not reusable")
		}
		offer.claims = 0
		link.now = func() time.Time { return offer.deadline }
		if link.admitPairing() != nil {
			t.Fatal("expired invitation admitted a pairing connection")
		}
		link.now = func() time.Time { return offer.deadline.Add(-time.Second) }
		offer.cancel()
		if link.admitPairing() != nil || offer.claims != 0 {
			t.Fatal("closed invitation admitted a pairing connection")
		}
	})
	t.Run("claimant_without_trust_frees_the_invitation", func(t *testing.T) {
		link := newFixtureLink(t)
		store := link.store.(*fixturePeerStore)
		offer := &pairingOffer{
			token: pairingToken{7}, deadline: link.now().Add(invitationLifetime),
			stop: make(chan struct{}), paired: make(chan struct{}),
		}
		link.offer = offer
		failed, uncertain, other := protocol.NodeID{3}, protocol.NodeID{4}, protocol.NodeID{5}
		store.trustOutcome = &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture directory is unavailable"}
		if err := acceptLegacyPairing(t, link, offer.token, failed); err == nil || offer.peer != nil || channelClosed(offer.paired) || link.isTrusted(failed) {
			t.Fatalf("a claimant that gained no trust kept the invitation: %v", err)
		}
		// A commit of unknown outcome is reconciled to trust, so its claim stays.
		store.trustOutcome = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "fixture directory sync is uncertain"}
		if err := acceptLegacyPairing(t, link, offer.token, uncertain); err == nil || offer.peer == nil || *offer.peer != uncertain || !link.isTrusted(uncertain) {
			t.Fatalf("a trusted claimant lost its invitation: %v", err)
		}
		store.trustOutcome = nil
		if err := acceptLegacyPairing(t, link, offer.token, other); err == nil || link.isTrusted(other) {
			t.Fatal("a second device claimed an invitation that already trusted one")
		}
		if err := acceptLegacyPairing(t, link, offer.token, uncertain); err != nil || !channelClosed(offer.paired) {
			t.Fatalf("the trusted claimant could not finish its pairing: %v", err)
		}
	})
	t.Run("uncertain_persistence_reconciles_live_peer", func(t *testing.T) {
		link := newFixtureLink(t)
		store := link.store.(*fixturePeerStore)
		peer := protocol.NodeID{9}
		unknown := &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "fixture directory sync is uncertain"}
		store.trustOutcome = unknown
		if err := link.SetTrust(t.Context(), peer, true); faultValue(err).Code != protocol.ErrorCodeOutcomeUnknown || !link.isTrusted(peer) {
			t.Fatalf("uncertain trust was not reconciled: %v", err)
		}
		store.trustOutcome = nil
		first := EndpointAddress{ID: peer, Addrs: []json.RawMessage{json.RawMessage(`{"Ip":"127.0.0.1:1"}`)}}
		if err := link.remember(t.Context(), first); err != nil {
			t.Fatal(err)
		}
		second := EndpointAddress{ID: peer, Addrs: []json.RawMessage{json.RawMessage(`{"Ip":"127.0.0.1:2"}`)}}
		store.rememberOutcome = unknown
		if err := link.remember(t.Context(), second); faultValue(err).Code != protocol.ErrorCodeOutcomeUnknown {
			t.Fatalf("uncertain remembered address: %v", err)
		}
		if current := link.currentAddress(EndpointAddress{ID: peer}); !addressIncludes(current, second) || addressIncludes(current, first) {
			t.Fatal("uncertain committed address was not reconciled")
		}
		slotContext, cancel := context.WithCancel(link.ctx)
		slot := &outgoingSlot{ctx: slotContext, cancel: cancel}
		link.outgoing[peer] = slot
		store.trustOutcome = unknown
		if err := link.SetTrust(t.Context(), peer, false); faultValue(err).Code != protocol.ErrorCodeOutcomeUnknown {
			t.Fatalf("uncertain revocation: %v", err)
		}
		if link.isTrusted(peer) || slotContext.Err() == nil || link.outgoing[peer] != nil {
			t.Fatal("uncertain committed revocation retained live access")
		}
		if peers, err := link.Peers(t.Context()); err != nil || len(peers) != 0 {
			t.Fatalf("uncertain revoked address remained: %#v, %v", peers, err)
		}
	})
}

func TestPairingResultRetainsRustUnit(t *testing.T) {
	t.Parallel()
	encoded, err := json.Marshal(pairingResult{})
	if err != nil || string(encoded) != `{"Ok":null}` {
		t.Fatalf("pairing result: %s, %v", encoded, err)
	}
	for _, value := range []string{`{"Ok":true}`, `{"Err":null}`, `{"Ok":null,"Err":null}`, `{"Ok":null,"Ok":null}`, `null`, `"Ok"`} {
		var result pairingResult
		if err := json.Unmarshal([]byte(value), &result); err == nil {
			t.Fatalf("invalid pairing result was accepted: %s", value)
		}
	}
}

// acceptLegacyPairing accepts a claim that does not say whether the claimant
// has a Node, as claimants before protocol version 2 sent it.
func acceptLegacyPairing(t *testing.T, link *Link, token pairingToken, peer protocol.NodeID) error {
	t.Helper()
	_, offer, err := link.acceptPairingIdentity(t.Context(), token, peer, EndpointAddress{ID: peer}, nil)
	if err == nil {
		link.confirmPairing(offer)
	}
	return err
}

func TestPairingKeepsClaimAvailableUntilConfirmed(t *testing.T) {
	t.Parallel()
	link := newFixtureLink(t)
	store := link.store.(*fixturePeerStore)
	offer := &pairingOffer{scope: InvitationScope{Kind: protocol.InvitationOtherUser}, token: pairingToken{7},
		deadline: link.now().Add(invitationLifetime), stop: make(chan struct{}), paired: make(chan struct{})}
	link.offer = offer
	peer := EndpointAddress{ID: protocol.NodeID{2}}
	hasNode := true
	store.rememberOutcome = &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture address storage unavailable"}
	if _, _, err := link.acceptPairingIdentity(t.Context(), offer.token, peer.ID, peer, &hasNode); err == nil {
		t.Fatal("address storage failure did not fail pairing")
	}
	if store.trusted[peer.ID] || link.isTrusted(peer.ID) || offer.peer != nil {
		t.Fatal("failed address storage left an incomplete pairing")
	}
	store.rememberOutcome = nil
	for range 2 {
		_, saved, err := link.acceptPairingIdentity(t.Context(), offer.token, peer.ID, peer, &hasNode)
		if err != nil || saved != offer || channelClosed(offer.paired) {
			t.Fatalf("claim retry = %p, %v", saved, err)
		}
		peers, err := link.Peers(t.Context())
		if err != nil || len(peers) != 1 || peers[0].ID != peer.ID {
			t.Fatalf("saved address = %v, %v", peers, err)
		}
	}
	link.confirmPairing(offer)
	if !channelClosed(offer.paired) {
		t.Fatal("acknowledged pairing did not finish")
	}
}
