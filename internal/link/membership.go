package link

import (
	"context"
	"encoding/json"
	"sort"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// InvitationScope is captured by the endpoint sharing the invitation. User is
// meaningful only on a Node; it is never accepted from a pairing claimant.
type InvitationScope struct {
	Kind protocol.InvitationKind
	User *protocol.UserID
}

type PeerRelationship struct {
	ID                protocol.NodeID   `json:"id"`
	Name              string            `json:"name,omitempty"`
	Direct            bool              `json:"direct"`
	DirectDirection   PeerDirection     `json:"direct_direction,omitempty"`
	MyDevice          bool              `json:"my_device"`
	Removed           bool              `json:"removed"`
	Sources           []protocol.NodeID `json:"sources"`
	DirectoryRevision uint64            `json:"directory_revision"`
}

// MembershipStore extends persistent peer stores. The base interface remains
// usable by transports that do not support identity invitations or directories.
type MembershipStore interface {
	PairMember(context.Context, protocol.NodeID, PeerDirection, InvitationScope) error
	Relationships(context.Context) ([]PeerRelationship, error)
	ApplyDirectory(context.Context, protocol.NodeID, uint64, []EndpointAddress, ...map[protocol.NodeID]string) error
}

// PeerNameStore retains display metadata learned from authenticated endpoints.
// Names never establish trust or change an address's certified identity.
type PeerNameStore interface {
	RememberPeerName(context.Context, protocol.NodeID, string) error
}

func validPeerName(name string) bool {
	return len([]rune(name)) <= 128 && !strings.ContainsFunc(name, unicode.IsControl)
}

func (link *Link) Name() string {
	link.mu.RLock()
	defer link.mu.RUnlock()
	if link.name == nil {
		return ""
	}
	return *link.name
}

func (link *Link) RememberPeerName(ctx context.Context, peer protocol.NodeID, name string) error {
	link.trustUpdate.Lock()
	defer link.trustUpdate.Unlock()
	return link.rememberPeerName(ctx, peer, name)
}

func (link *Link) rememberPeerName(ctx context.Context, peer protocol.NodeID, name string) error {
	name = strings.TrimSpace(name)
	if !validPeerName(name) {
		return invalidLink("invalid device name")
	}
	if !link.isTrusted(peer) {
		return pairingRejected()
	}
	store, ok := link.store.(PeerNameStore)
	if !ok {
		return nil
	}
	values, err := link.Relationships(ctx)
	if err != nil {
		return err
	}
	for _, value := range values {
		if value.ID == peer && value.Name == name {
			return nil
		}
	}
	if err := store.RememberPeerName(ctx, peer, name); err != nil {
		return err
	}
	link.notifyPeers()
	return nil
}

func (link *Link) pairMember(ctx context.Context, peer protocol.NodeID, direction PeerDirection, scope InvitationScope) error {
	return link.updateTrust(ctx, peer, func() error {
		if store, ok := link.store.(MembershipStore); ok {
			return store.PairMember(ctx, peer, direction, scope)
		}
		if scope.Kind == protocol.InvitationMyDevice {
			return invalidLink("peer store does not support my-device invitations")
		}
		return link.store.PairPeer(ctx, peer, direction)
	})
}

func (link *Link) Relationships(ctx context.Context) ([]PeerRelationship, error) {
	if store, ok := link.store.(MembershipStore); ok {
		values, err := store.Relationships(ctx)
		for index := range values {
			if values[index].Direct && values[index].DirectDirection == "" && !values[index].Removed {
				values[index].DirectDirection = link.Direction(values[index].ID)
			}
		}
		return values, err
	}
	peers, err := link.store.Peers(ctx)
	values := make([]PeerRelationship, 0, len(peers))
	for _, peer := range peers {
		values = append(values, PeerRelationship{ID: peer, Direct: true, DirectDirection: link.Direction(peer), Sources: []protocol.NodeID{}})
	}
	return values, err
}

func (link *Link) MyDevice(ctx context.Context, peer protocol.NodeID) (bool, error) {
	values, err := link.Relationships(ctx)
	for _, value := range values {
		if value.ID == peer {
			return value.MyDevice && !value.Removed && link.Direction(peer).Valid(), err
		}
	}
	return false, err
}

// ApplyDirectory imports only outbound hosts from an authenticated my-device
// source. Store provenance preserves direct pairs and explicit local removals.
func (link *Link) ApplyDirectory(ctx context.Context, directory protocol.HostDirectory) error {
	known, err := link.MyDevice(ctx, directory.Source)
	if err != nil {
		return err
	}
	if !known {
		return pairingRejected()
	}
	relationships, err := link.Relationships(ctx)
	if err != nil {
		return err
	}
	for _, relationship := range relationships {
		if relationship.ID == directory.Source && directory.Revision <= relationship.DirectoryRevision {
			return nil
		}
	}
	if len(directory.Hosts) > maxControllerPeers {
		return invalidLink("host directory exceeds limits")
	}
	self, err := link.Address(ctx)
	if err != nil {
		return err
	}
	byID := map[protocol.NodeID]EndpointAddress{}
	names := map[protocol.NodeID]string{}
	for _, host := range directory.Hosts {
		var address EndpointAddress
		if len(host.Address) > 8192 || json.Unmarshal([]byte(host.Address), &address) != nil || address.ID != host.Node || len(address.Addrs) > 16 {
			return invalidLink("invalid directory host address")
		}
		if !validPeerName(host.Name) {
			return invalidLink("invalid directory host name")
		}
		if address.ID != self.ID {
			byID[address.ID] = address
			names[address.ID] = strings.TrimSpace(host.Name)
		}
	}
	addresses := make([]EndpointAddress, 0, len(byID))
	for _, address := range byID {
		addresses = append(addresses, address)
	}
	sort.Slice(addresses, func(i, j int) bool { return addresses[i].ID.String() < addresses[j].ID.String() })
	store, ok := link.store.(MembershipStore)
	if !ok {
		return invalidLink("peer store does not support host directories")
	}
	if err := store.ApplyDirectory(ctx, directory.Source, directory.Revision, addresses, names); err != nil {
		return err
	}
	return link.RefreshPeers(ctx)
}

// RefreshPeers reconciles trust changed by a Node command transaction. Removed
// inbound access closes existing connections before another request can use it.
func (link *Link) RefreshPeers(ctx context.Context) error {
	link.trustUpdate.Lock()
	defer link.trustUpdate.Unlock()
	stored, err := link.store.Directions(ctx)
	if err != nil {
		return err
	}
	ids := map[protocol.NodeID]bool{}
	link.mu.RLock()
	for id := range link.trusted {
		ids[id] = true
	}
	link.mu.RUnlock()
	for _, value := range stored {
		ids[value.ID] = true
	}
	for id := range ids {
		if err := link.reconcilePeer(ctx, id); err != nil {
			link.applyTrust(id, "")
			return err
		}
	}
	return nil
}
