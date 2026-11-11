package link

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
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
	decoded, err := decodePairingTicket(string(encoded), local, now)
	if err != nil || decoded.Token != ticket.Token || decoded.Address.ID != ticket.Address.ID {
		t.Fatalf("valid ticket: %#v, %v", decoded, err)
	}
	for _, test := range []struct {
		name  string
		field string
		value any
	}{
		{"expired", "expires_at_ms", now.UnixMilli()},
		{"too_far_future", "expires_at_ms", now.Add(invitationLifetime + time.Millisecond).UnixMilli()},
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
			if _, err := decodePairingTicket(string(modified), local, now); err == nil {
				t.Fatal("invalid pairing ticket was accepted")
			}
		})
	}
	t.Run("self_pairing", func(t *testing.T) {
		if _, err := decodePairingTicket(string(encoded), ticket.Address.ID, now); err == nil {
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
		if _, err := decodePairingTicket(string(modified), local, now); err == nil {
			t.Fatal("oversized address set was accepted")
		}
	})
	t.Run("ticket_size", func(t *testing.T) {
		if _, err := decodePairingTicket(strings.Repeat(" ", 8193), local, now); err == nil {
			t.Fatal("oversized ticket was accepted")
		}
	})
	t.Run("duplicate_fields", func(t *testing.T) {
		modified := strings.Replace(string(encoded), `"version":1`, `"version":1,"version":1`, 1)
		if _, err := decodePairingTicket(modified, local, now); err == nil {
			t.Fatal("duplicate pairing ticket field was accepted")
		}
	})
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
			finished <- result{peer, link.acceptPairing(t.Context(), offer.token, peer, EndpointAddress{ID: peer})}
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
	if err := link.acceptPairing(t.Context(), offer.token, winner, EndpointAddress{ID: winner}); err != nil {
		t.Fatalf("certified winner could not retry its valid ticket: %v", err)
	}
	if err := link.SetTrust(t.Context(), winner, false); err != nil {
		t.Fatal(err)
	}
	if err := link.acceptPairing(t.Context(), offer.token, winner, EndpointAddress{ID: winner}); err == nil {
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
		if err := link.acceptPairing(t.Context(), offer.token, failed, EndpointAddress{ID: failed}); err == nil || offer.peer != nil || channelClosed(offer.paired) || link.isTrusted(failed) {
			t.Fatalf("a claimant that gained no trust kept the invitation: %v", err)
		}
		// A commit of unknown outcome is reconciled to trust, so its claim stays.
		store.trustOutcome = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "fixture directory sync is uncertain"}
		if err := link.acceptPairing(t.Context(), offer.token, uncertain, EndpointAddress{ID: uncertain}); err == nil || offer.peer == nil || *offer.peer != uncertain || !link.isTrusted(uncertain) {
			t.Fatalf("a trusted claimant lost its invitation: %v", err)
		}
		store.trustOutcome = nil
		if err := link.acceptPairing(t.Context(), offer.token, other, EndpointAddress{ID: other}); err == nil || link.isTrusted(other) {
			t.Fatal("a second device claimed an invitation that already trusted one")
		}
		if err := link.acceptPairing(t.Context(), offer.token, uncertain, EndpointAddress{ID: uncertain}); err != nil || !channelClosed(offer.paired) {
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
