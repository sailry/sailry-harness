package link

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/subtle"
	"encoding/json"
	"errors"
	"io"
	"slices"
	"sync"
	"time"

	ma "github.com/multiformats/go-multiaddr"

	"github.com/sailry/sailry-harness/internal/link/peertransport"
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
	scope       InvitationScope
	token       pairingToken
	deadline    time.Time
	stop        chan struct{}
	paired      chan struct{}
	peer        *protocol.NodeID
	peerHasNode *bool
	claims      int // connections being served for this offer; guarded by offerMu
	close       sync.Once
	accept      sync.Once
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
	if err := link.Online(ctx); err != nil {
		return nil, err
	}
	address, err := link.Address(ctx)
	if err != nil {
		return nil, err
	}
	if len(address.Addrs) == 0 {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Link endpoint has no dialable address to advertise"}
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
	ticket, err := decodePairingTicket(encoded, link.nodeID)
	if err != nil {
		return EndpointAddress{}, err
	}
	var selected []libp2pRelaySpec
	if relays != nil {
		selected, err = parseLibp2pRelays(relays)
		if err != nil {
			return EndpointAddress{}, err
		}
		ticket.Address, err = pairingAddressOnNetwork(ticket.Address, selected)
		if err != nil {
			return EndpointAddress{}, err
		}
	}
	dialAddress, err := libp2pDialAddress(ticket.Address)
	if err != nil {
		return EndpointAddress{}, err
	}
	address, err := link.Address(ctx)
	if err != nil {
		return EndpointAddress{}, err
	}
	scoped, cancel := link.operation(ctx, true)
	var connection peertransport.Connection
	if endpoint, ok := link.endpoint.(*libp2pEndpoint); ok && relays != nil {
		connection, err = endpoint.connect(scoped, dialAddress, wire.PairALPN, selected)
	} else {
		connection, err = link.endpoint.Connect(scoped, dialAddress, wire.PairALPN)
	}
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
	if relays != nil {
		// Check the actual stream before sending the invitation token.
		opened, ok := stream.(*libp2pStream)
		if !ok || !matchesPairingNetwork(opened.raw.Conn().RemoteMultiaddr(), selected) {
			return EndpointAddress{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "the opened pairing path does not match the selected network"}
		}
	}
	link.cancelHostRemoval(ticket.Address.ID)
	hasNode := link.handler != nil
	if err := link.writeFrame(ctx, stream, pairingClaim{Token: ticket.Token, Address: address, HasNode: &hasNode}); err != nil {
		return EndpointAddress{}, err
	}
	var accepted pairingResult
	if err := link.readFrame(ctx, stream, &accepted, false); err != nil {
		return ticket.Address, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "the pairing response was not received; retry the same invitation"}
	}
	if accepted.Fault != nil {
		if accepted.Fault.Code == protocol.ErrorCodeOutcomeUnknown {
			return ticket.Address, accepted.Fault
		}
		return EndpointAddress{}, accepted.Fault
	}
	scope := InvitationScope{Kind: protocol.InvitationOtherUser}
	if accepted.HasNode == nil && accepted.MyDevice {
		scope.Kind = protocol.InvitationMyDevice
	}
	save, release := link.operation(context.WithoutCancel(ctx), true)
	err = link.savePairing(save, ticket.Address, DirectionHost, scope, accepted.HasNode)
	release()
	if err != nil {
		// Closing an unfinished send stream resets it. The sharer keeps the
		// invitation available to this authenticated claimant until it expires.
		return ticket.Address, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "the host accepted the pairing but the local pairing could not be confirmed; retry the same invitation"}
	}
	// The write half closes only after both ends have confirmed their
	// records. Older sharers still accept the original claim frame.
	if stream.CloseWrite() == nil {
		scoped, cancel := link.operation(ctx, true)
		_, _ = stream.WaitStopped(scoped)
		cancel()
	}
	if link.paired != nil {
		link.paired(ticket.Address.ID)
	}
	return ticket.Address, nil
}

func pairingAddressOnNetwork(address EndpointAddress, selected []libp2pRelaySpec) (EndpointAddress, error) {
	address, err := libp2pDialAddress(address)
	if err != nil {
		return EndpointAddress{}, err
	}
	info, err := libp2pAddressInfo(address)
	if err != nil {
		return EndpointAddress{}, err
	}
	filtered := EndpointAddress{ID: address.ID}
	for index, route := range info.Addrs {
		if matchesPairingNetwork(route, selected) {
			filtered.Addrs = append(filtered.Addrs, address.Addrs[index])
		}
	}
	if len(filtered.Addrs) == 0 {
		if len(selected) == 0 {
			return EndpointAddress{}, invalidLink("pairing invitation does not advertise a direct Libp2p address")
		}
		return EndpointAddress{}, invalidLink("pairing invitation does not advertise a selected Libp2p relay")
	}
	return filtered, nil
}

func matchesPairingNetwork(route ma.Multiaddr, selected []libp2pRelaySpec) bool {
	if len(selected) == 0 {
		return libp2pDirectAddr(route, false)
	}
	id, circuit := libp2pCircuitRelay(route)
	return circuit && slices.ContainsFunc(selected, func(relay libp2pRelaySpec) bool { return id == relay.info.ID })
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

func (link *Link) servePairing(connection peertransport.Connection, peer protocol.NodeID) {
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
	err = wire.ReadLimited(&scopedReader{ctx: scoped, stream: stream}, &claim, 8192)
	cancel()
	if err != nil {
		return
	}
	var accepted error
	var scope InvitationScope
	var saved *pairingOffer
	if protocol.VERSION >= 2 && claim.HasNode == nil {
		accepted = pairingRejected()
	} else {
		scope, saved, accepted = link.acceptPairingIdentity(link.ctx, claim.Token, peer, claim.Address, claim.HasNode)
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
	if saved != nil {
		// Claimants that can recover a local write failure acknowledge only
		// after saving. Legacy claimants already closed their write half.
		scoped, cancel := link.operation(link.ctx, true)
		var extra [1]byte
		count, err := stream.ReadContext(scoped, extra[:])
		cancel()
		if count == 0 && errors.Is(err, io.EOF) {
			link.confirmPairing(saved)
		}
	}
	scoped, cancel = link.operation(link.ctx, true)
	_, _ = stream.WaitStopped(scoped)
	cancel()
}

func (link *Link) acceptPairingIdentity(ctx context.Context, token pairingToken, peer protocol.NodeID, address EndpointAddress, hasNode *bool) (InvitationScope, *pairingOffer, error) {
	if address.ID != peer || len(address.Addrs) > 16 {
		return InvitationScope{}, nil, pairingRejected()
	}
	// Serialize claiming and saving with other claims and Link trust updates.
	// A retained claim may resume only its already granted inbound pairing.
	link.trustUpdate.Lock()
	defer link.trustUpdate.Unlock()
	link.offerMu.Lock()
	offer := link.offer
	if offer == nil || subtle.ConstantTimeCompare(offer.token[:], token[:]) != 1 ||
		channelClosed(offer.stop) || !link.now().Before(offer.deadline) ||
		offer.peer != nil && *offer.peer != peer {
		link.offerMu.Unlock()
		return InvitationScope{}, nil, pairingRejected()
	}
	resume := offer.peer != nil
	if resume && ((offer.peerHasNode == nil) != (hasNode == nil) || hasNode != nil && *offer.peerHasNode != *hasNode) {
		link.offerMu.Unlock()
		return InvitationScope{}, nil, pairingRejected()
	}
	if !resume {
		offer.peer = &peer
		if hasNode != nil {
			offer.peerHasNode = new(*hasNode)
		}
	}
	link.offerMu.Unlock()
	scope := offer.scope
	if hasNode != nil {
		scope = InvitationScope{Kind: protocol.InvitationOtherUser}
	}
	if err := link.savePairingLocked(ctx, address, DirectionController, scope, hasNode, resume); err != nil {
		if !resume && faultValue(err).Code != protocol.ErrorCodeOutcomeUnknown {
			link.offerMu.Lock()
			offer.peer = nil
			offer.peerHasNode = nil
			link.offerMu.Unlock()
		}
		return InvitationScope{}, nil, err
	}
	return scope, offer, nil
}

func (link *Link) confirmPairing(offer *pairingOffer) {
	link.offerMu.Lock()
	defer link.offerMu.Unlock()
	if link.offer == offer {
		offer.accept.Do(func() { close(offer.paired) })
	}
}

func (link *Link) savePairing(ctx context.Context, address EndpointAddress, direction PeerDirection, scope InvitationScope, hasNode *bool) error {
	link.trustUpdate.Lock()
	defer link.trustUpdate.Unlock()
	return link.savePairingLocked(ctx, address, direction, scope, hasNode, false)
}

func (link *Link) savePairingLocked(ctx context.Context, address EndpointAddress, direction PeerDirection, scope InvitationScope, hasNode *bool, resume bool) error {
	if err := link.updateTrustLocked(ctx, address.ID, func() error {
		if !resume {
			link.cancelHostRemoval(address.ID)
		}
		return link.store.SavePairing(ctx, address, direction, scope, hasNode, resume)
	}); err != nil {
		return err
	}
	if hasNode != nil {
		link.peerKinds.Store(address.ID, *hasNode)
	}
	return nil
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
