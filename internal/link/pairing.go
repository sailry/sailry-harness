package link

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/subtle"
	"encoding/json"
	"errors"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/link/iroh"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

const invitationLifetime = time.Minute

type pairingToken [32]byte

func (token *pairingToken) UnmarshalJSON(encoded []byte) error {
	var values []json.RawMessage
	if json.Unmarshal(encoded, &values) != nil || len(values) != len(token) {
		return pairingRejected()
	}
	var decoded pairingToken
	for index, value := range values {
		var number *uint8
		if json.Unmarshal(value, &number) != nil || number == nil {
			return pairingRejected()
		}
		decoded[index] = *number
	}
	*token = decoded
	return nil
}

type pairingTicket struct {
	Version     uint8           `json:"version"`
	Address     EndpointAddress `json:"address"`
	Token       pairingToken    `json:"token"`
	ExpiresAtMS uint64          `json:"expires_at_ms"`
}

func (ticket *pairingTicket) UnmarshalJSON(encoded []byte) error {
	type plain pairingTicket
	var decoded plain
	if err := protocol.DecodeObject(encoded, &decoded, []string{"version", "address", "token", "expires_at_ms"}, true); err != nil {
		return pairingRejected()
	}
	*ticket = pairingTicket(decoded)
	return nil
}

type pairingClaim struct {
	Token   pairingToken    `json:"token"`
	Address EndpointAddress `json:"address"`
	HasNode *bool           `json:"has_node,omitempty"`
}

func (claim *pairingClaim) UnmarshalJSON(encoded []byte) error {
	type plain pairingClaim
	var decoded plain
	if err := protocol.DecodeObject(encoded, &decoded, []string{"token", "address"}, true); err != nil {
		return pairingRejected()
	}
	*claim = pairingClaim(decoded)
	return nil
}

type pairingResult struct {
	Fault    *protocol.Fault
	MyDevice bool
	HasNode  *bool
}

func (result pairingResult) MarshalJSON() ([]byte, error) {
	if result.Fault == nil {
		if result.HasNode != nil {
			return json.Marshal(struct {
				OK       any   `json:"Ok"`
				MyDevice bool  `json:"my_device,omitempty"`
				HasNode  *bool `json:"has_node"`
			}{nil, result.MyDevice, result.HasNode})
		}
		if result.MyDevice {
			return []byte(`{"Ok":null,"my_device":true}`), nil
		}
		return []byte(`{"Ok":null}`), nil
	}
	return json.Marshal(struct {
		Fault *protocol.Fault `json:"Err"`
	}{result.Fault})
}

func (result *pairingResult) UnmarshalJSON(encoded []byte) error {
	var fields struct {
		OK       json.RawMessage `json:"Ok"`
		Err      json.RawMessage `json:"Err"`
		MyDevice bool            `json:"my_device"`
		HasNode  *bool           `json:"has_node"`
	}
	if protocol.DecodeObject(encoded, &fields, nil, true) != nil || (len(fields.OK) == 0) == (len(fields.Err) == 0) {
		return pairingRejected()
	}
	result.Fault, result.MyDevice, result.HasNode = nil, fields.MyDevice, fields.HasNode
	if len(fields.OK) != 0 {
		if !bytes.Equal(bytes.TrimSpace(fields.OK), []byte("null")) {
			return pairingRejected()
		}
	} else {
		if result.MyDevice || bytes.Equal(bytes.TrimSpace(fields.Err), []byte("null")) {
			return pairingRejected()
		}
		var failure protocol.Fault
		if json.Unmarshal(fields.Err, &failure) != nil {
			return pairingRejected()
		}
		result.Fault = &failure
	}
	return nil
}

type pairingOffer struct {
	scope    InvitationScope
	token    pairingToken
	deadline time.Time
	stop     chan struct{}
	paired   chan struct{}
	peer     *protocol.NodeID
	claims   int // connections being served for this offer; guarded by offerMu
	close    sync.Once
	accept   sync.Once
}

func (offer *pairingOffer) cancel() {
	offer.close.Do(func() { close(offer.stop) })
}

type Invitation struct {
	ticket      string
	expiresAtMS uint64
	deadline    time.Time
	offer       *pairingOffer
	link        *Link
}

func (invitation *Invitation) Ticket() string      { return invitation.ticket }
func (invitation *Invitation) ExpiresAtMS() uint64 { return invitation.expiresAtMS }
func (*Invitation) String() string                 { return "Invitation([REDACTED])" }
func (*Invitation) GoString() string               { return "Invitation([REDACTED])" }
func (invitation *Invitation) Close() error        { invitation.offer.cancel(); return nil }
func (invitation *Invitation) IsPaired() bool      { return channelClosed(invitation.offer.paired) }

func (invitation *Invitation) Paired(ctx context.Context) error {
	if invitation.IsPaired() {
		return nil
	}
	scoped, cancel := invitation.link.operation(ctx, false)
	defer cancel()
	deadline, expire := invitation.link.timeoutContext(scoped, max(0, invitation.deadline.Sub(invitation.link.now())))
	defer expire()
	select {
	case <-invitation.offer.paired:
		return nil
	case <-invitation.offer.stop:
		return pairingRejected()
	case <-deadline.Done():
		if invitation.link.ctx.Err() != nil {
			return linkNetwork(invitation.link.ctx.Err())
		}
		if ctx.Err() != nil {
			return ctx.Err()
		}
		return pairingRejected()
	}
}

func (link *Link) Invite(ctx context.Context) (*Invitation, error) {
	return link.InviteFor(ctx, InvitationScope{Kind: protocol.InvitationOtherUser})
}

func (link *Link) InviteFor(ctx context.Context, scope InvitationScope) (*Invitation, error) {
	if !scope.Kind.Valid() {
		return nil, invalidLink("invalid invitation kind")
	}
	if scope.User != nil {
		user := *scope.User
		scope.User = &user
	}
	if link.ctx.Err() != nil {
		return nil, linkNetwork(link.ctx.Err())
	}
	address, err := link.Address(ctx)
	if err != nil {
		return nil, err
	}
	var token pairingToken
	if _, err := rand.Read(token[:]); err != nil {
		return nil, linkNetwork(err)
	}
	now := link.now()
	if now.UnixMilli() < 0 {
		return nil, linkNetwork(errors.New("system clock is before the Unix epoch"))
	}
	deadline := now.Add(invitationLifetime)
	expires := uint64(deadline.UnixMilli())
	encoded, err := json.Marshal(pairingTicket{Version: 1, Address: address, Token: token, ExpiresAtMS: expires})
	if err != nil {
		return nil, linkNetwork(err)
	}
	offer := &pairingOffer{scope: scope, token: token, deadline: deadline, stop: make(chan struct{}), paired: make(chan struct{})}
	link.offerMu.Lock()
	if link.offer != nil {
		link.offer.cancel()
	}
	link.offer = offer
	link.offerMu.Unlock()
	return &Invitation{ticket: string(encoded), expiresAtMS: expires, deadline: deadline, offer: offer, link: link}, nil
}

// decodePairingTicket checks what the claimant can check without a clock. The
// sharer alone decides whether a ticket is still valid: it holds the deadline
// and accepts the token once. The claimant does not compare the expiry with
// its own clock, because the two machines' clocks differ and a correct ticket
// would be refused (a Host a few seconds ahead was enough).
func decodePairingTicket(encoded string, local protocol.NodeID) (pairingTicket, error) {
	var ticket pairingTicket
	if len(encoded) > 8192 || json.Unmarshal([]byte(encoded), &ticket) != nil {
		return pairingTicket{}, pairingRejected()
	}
	if ticket.Version != 1 || ticket.ExpiresAtMS == 0 || ticket.Address.ID == local || len(ticket.Address.Addrs) > 16 {
		return pairingTicket{}, pairingRejected()
	}
	return ticket, nil
}

func (link *Link) Pair(ctx context.Context, encoded string) (EndpointAddress, error) {
	return link.pairSelected(ctx, encoded, nil)
}

func (link *Link) pairSelected(ctx context.Context, encoded string, relays []string) (EndpointAddress, error) {
	address, err := link.Address(ctx)
	if err != nil {
		return EndpointAddress{}, err
	}
	ticket, err := decodePairingTicket(encoded, address.ID)
	if err != nil {
		return EndpointAddress{}, err
	}
	if relays != nil {
		ticket.Address, err = pairingAddressOnNetwork(ticket.Address, relays)
		if err != nil {
			return EndpointAddress{}, err
		}
	}
	link.cancelHostRemoval(ticket.Address.ID)
	if err := link.insertAddressRelays(ctx, ticket.Address); err != nil {
		return EndpointAddress{}, err
	}
	scoped, cancel := link.operation(ctx, true)
	connection, err := link.endpoint.Connect(scoped, ticket.Address, wire.PairALPN)
	cancel()
	if err != nil {
		return EndpointAddress{}, linkNetwork(err)
	}
	defer connection.Close(0, "pairing finished")
	scoped, cancel = link.operation(ctx, true)
	info, err := connection.Info(scoped)
	cancel()
	if err != nil || info.RemoteID != ticket.Address.ID || info.ALPN != wire.PairALPN {
		return EndpointAddress{}, pairingRejected()
	}
	scoped, cancel = link.operation(ctx, true)
	stream, err := connection.OpenBi(scoped)
	cancel()
	if err != nil {
		return EndpointAddress{}, linkNetwork(err)
	}
	defer stream.Close()
	hasNode := link.handler != nil
	if err := link.writeFrame(ctx, stream, pairingClaim{Token: ticket.Token, Address: address, HasNode: &hasNode}); err != nil {
		return EndpointAddress{}, err
	}
	if err := stream.CloseWrite(); err != nil {
		return EndpointAddress{}, linkNetwork(err)
	}
	var accepted pairingResult
	if err := link.readFrame(ctx, stream, &accepted, false); err != nil {
		return EndpointAddress{}, err
	}
	if accepted.Fault != nil {
		return EndpointAddress{}, accepted.Fault
	}
	scope := InvitationScope{Kind: protocol.InvitationOtherUser}
	if accepted.MyDevice {
		scope.Kind = protocol.InvitationMyDevice
	}
	if accepted.HasNode != nil {
		if err := link.pairIdentity(ctx, info.RemoteID, DirectionHost, *accepted.HasNode); err != nil {
			return EndpointAddress{}, err
		}
	} else if err := link.pairMember(ctx, info.RemoteID, DirectionHost, scope); err != nil {
		return EndpointAddress{}, err
	}
	if err := link.remember(ctx, ticket.Address); err != nil {
		return EndpointAddress{}, err
	}
	if link.paired != nil {
		link.paired(ticket.Address.ID)
	}
	return ticket.Address, nil
}

func pairingAddressOnNetwork(address EndpointAddress, relays []string) (EndpointAddress, error) {
	filtered := EndpointAddress{ID: address.ID, Addrs: []json.RawMessage{}}
	offered, selected := false, false
	for _, hint := range address.Addrs {
		if relay, ok := relayHint(hint); ok {
			offered = true
			if containsRelay(relays, relay) {
				filtered.Addrs = append(filtered.Addrs, append(json.RawMessage{}, hint...))
				selected = true
			}
			continue
		}
		var transport map[string]json.RawMessage
		if json.Unmarshal(hint, &transport) == nil && len(transport) == 1 && transport["Ip"] != nil {
			filtered.Addrs = append(filtered.Addrs, append(json.RawMessage{}, hint...))
		}
	}
	if offered && !selected {
		return EndpointAddress{}, invalidLink("pairing invitation uses another relay network")
	}
	return filtered, nil
}

// maxPairings bounds the accept workers one invitation's claimants occupy, so
// silent claimants leave the remaining workers to paired peers.
const maxPairings = 4

// admitPairing reserves one of the open invitation's claimant places. It
// returns nil when no invitation is open or its places are taken.
func (link *Link) admitPairing() *pairingOffer {
	link.offerMu.Lock()
	defer link.offerMu.Unlock()
	offer := link.offer
	if offer == nil || channelClosed(offer.stop) || !link.now().Before(offer.deadline) || offer.claims >= maxPairings {
		return nil
	}
	offer.claims++
	return offer
}

func (link *Link) servePairing(connection *iroh.Connection, peer protocol.NodeID) {
	// A connection that no invitation can answer is closed before it holds an
	// accept worker for the stream and claim deadlines.
	offer := link.admitPairing()
	if offer == nil {
		_ = connection.Close(1, "pairing invitation is unavailable")
		return
	}
	defer func() {
		link.offerMu.Lock()
		offer.claims--
		link.offerMu.Unlock()
	}()
	defer connection.Close(0, "pairing finished")
	scoped, cancel := link.operation(link.ctx, true)
	stream, err := connection.AcceptBi(scoped)
	cancel()
	if err != nil {
		return
	}
	defer stream.Close()
	scoped, cancel = link.operation(link.ctx, true)
	var claim pairingClaim
	err = wire.ReadLimited(scopedReader{scoped, stream}, &claim, 8192)
	cancel()
	if err != nil {
		return
	}
	var accepted error
	var scope InvitationScope
	if claim.Address.ID != peer || len(claim.Address.Addrs) > 16 || protocol.VERSION >= 2 && claim.HasNode == nil {
		accepted = pairingRejected()
	} else {
		scope, accepted = link.acceptPairingIdentity(link.ctx, claim.Token, peer, claim.Address, claim.HasNode)
	}
	result := pairingResult{}
	hasNode := link.handler != nil
	result.HasNode = &hasNode
	result.MyDevice = accepted == nil && scope.Kind == protocol.InvitationMyDevice
	if accepted != nil {
		fault := faultValue(accepted)
		result.Fault = &fault
	}
	if err := link.writeFrame(link.ctx, stream, result); err != nil {
		return
	}
	if stream.CloseWrite() != nil {
		return
	}
	scoped, cancel = link.operation(link.ctx, true)
	_, _ = stream.WaitStopped(scoped)
	cancel()
}

func (link *Link) acceptPairingIdentity(ctx context.Context, token pairingToken, peer protocol.NodeID, address EndpointAddress, hasNode *bool) (InvitationScope, error) {
	link.offerMu.Lock()
	offer := link.offer
	if offer == nil || subtle.ConstantTimeCompare(offer.token[:], token[:]) != 1 ||
		channelClosed(offer.stop) || !link.now().Before(offer.deadline) ||
		offer.peer != nil && *offer.peer != peer ||
		channelClosed(offer.paired) && !link.isTrusted(peer) {
		link.offerMu.Unlock()
		return InvitationScope{}, pairingRejected()
	}
	claimed := peer
	offer.peer = &claimed
	link.offerMu.Unlock()
	scope := offer.scope
	if hasNode != nil {
		scope = InvitationScope{Kind: protocol.InvitationOtherUser}
	}
	var paired error
	if hasNode != nil {
		paired = link.pairIdentity(ctx, peer, DirectionController, *hasNode)
	} else {
		paired = link.pairMember(ctx, peer, DirectionController, scope)
	}
	if paired != nil {
		// A claimant that gained no trust leaves the invitation to the others.
		link.offerMu.Lock()
		if offer.peer == &claimed && !link.isTrusted(peer) {
			offer.peer = nil
		}
		link.offerMu.Unlock()
		return InvitationScope{}, paired
	}
	if err := link.remember(ctx, address); err != nil {
		return InvitationScope{}, err
	}
	link.offerMu.Lock()
	if link.offer == offer {
		offer.accept.Do(func() { close(offer.paired) })
	}
	link.offerMu.Unlock()
	return scope, nil
}

func channelClosed(channel <-chan struct{}) bool {
	select {
	case <-channel:
		return true
	default:
		return false
	}
}

func pairingRejected() error {
	return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "pairing invitation is unavailable"}
}
