package link

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const maxControllerPeers = 128
const maxControllerBytes = 1024 * 1024

type PeerStore interface {
	Peers(context.Context) ([]protocol.NodeID, error)
	Addresses(context.Context) ([]EndpointAddress, error)
	Remember(context.Context, EndpointAddress) error
	SetTrust(context.Context, protocol.NodeID, bool) error
	Directions(context.Context) ([]PeerTrust, error)
	PairPeer(context.Context, protocol.NodeID, PeerDirection) error
	SetDirection(context.Context, protocol.NodeID, PeerDirection) error
}

type controllerPeer struct {
	ID                protocol.NodeID      `json:"id"`
	Name              string               `json:"name,omitempty"`
	Removals          []HostRemovalAttempt `json:"removals,omitempty"`
	Address           *EndpointAddress     `json:"address"`
	Direction         PeerDirection        `json:"direction"`
	Direct            bool                 `json:"direct"`
	DirectDirection   PeerDirection        `json:"direct_direction,omitempty"`
	MyDevice          bool                 `json:"my_device"`
	Removed           bool                 `json:"removed"`
	Sources           []protocol.NodeID    `json:"sources,omitempty"`
	DirectoryRevision uint64               `json:"directory_revision"`
	HasNode           *bool                `json:"has_node,omitempty"`
	DirectoryOnly     bool                 `json:"directory_only,omitempty"`
}

func (peer *controllerPeer) UnmarshalJSON(encoded []byte) error {
	type plain controllerPeer
	decoded := plain{Direction: DirectionHost, Direct: true}
	if err := protocol.DecodeObject(encoded, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	if decoded.Direct && !decoded.Removed && decoded.DirectDirection == "" {
		decoded.DirectDirection = decoded.Direction
	}
	*peer = controllerPeer(decoded)
	return nil
}

type controllerRecord struct {
	Version uint32           `json:"version"`
	Peers   []controllerPeer `json:"peers"`
}

func (record *controllerRecord) UnmarshalJSON(encoded []byte) error {
	type plain controllerRecord
	var decoded plain
	if err := protocol.DecodeObject(encoded, &decoded, []string{"version", "peers"}, false); err != nil {
		return err
	}
	*record = controllerRecord(decoded)
	return nil
}

type controllerStore struct {
	path  string
	mu    sync.Mutex
	peers []controllerPeer
	lock  *os.File
}

func openControllerStore(path string) (Identity, *controllerStore, error) {
	if !filepath.IsAbs(path) {
		return Identity{}, nil, controllerStorage(errors.New("absolute controller directory required"))
	}
	directory, lock, err := prepareControllerFiles(path)
	if err != nil {
		return Identity{}, nil, controllerStorage(err)
	}
	ready := false
	defer func() {
		if !ready {
			_ = lock.Close()
		}
	}()
	for _, name := range []string{"storage/node.sqlite", "storage/node.turso", "storage/node.sqlite3"} {
		if _, err := os.Stat(filepath.Join(directory, name)); err == nil {
			return Identity{}, nil, controllerStorage(errors.New("a Node profile cannot be opened as a controller"))
		} else if !errors.Is(err, os.ErrNotExist) {
			return Identity{}, nil, controllerStorage(err)
		}
	}
	identity, err := OpenIdentity(directory)
	if err != nil {
		return Identity{}, nil, controllerStorage(err)
	}
	if err := validateControllerIdentity(directory); err != nil {
		return Identity{}, nil, controllerStorage(err)
	}
	store := &controllerStore{path: directory, lock: lock, peers: []controllerPeer{}}
	file, err := readControllerFile(filepath.Join(directory, "peers.json"))
	switch {
	case err == nil:
		encoded, readErr := io.ReadAll(io.LimitReader(file, maxControllerBytes+1))
		readErr = errors.Join(readErr, file.Close())
		if readErr != nil {
			return Identity{}, nil, controllerStorage(readErr)
		}
		record, err := decodeControllerRecord(encoded)
		if err != nil {
			return Identity{}, nil, err
		}
		store.peers = record.Peers
		if retireControllerImportedRoutes(store.peers) {
			if err := store.update(context.Background(), func(*[]controllerPeer) error { return nil }); err != nil {
				return Identity{}, nil, err
			}
		}
	case errors.Is(err, os.ErrNotExist):
		if err := store.update(context.Background(), func(*[]controllerPeer) error { return nil }); err != nil {
			return Identity{}, nil, err
		}
	default:
		return Identity{}, nil, controllerStorage(err)
	}
	ready = true
	return identity, store, nil
}

func decodeControllerRecord(encoded []byte) (controllerRecord, error) {
	var record controllerRecord
	if len(encoded) > maxControllerBytes || json.Unmarshal(encoded, &record) != nil || record.Version != 1 {
		return controllerRecord{}, controllerStorage(errors.New("unsupported peer record"))
	}
	unique := make(map[protocol.NodeID]struct{}, len(record.Peers))
	active := 0
	for _, peer := range record.Peers {
		if !peer.Removed && !peer.DirectoryOnly {
			active++
		}
		_, duplicate := unique[peer.ID]
		if duplicate || !validPeerName(peer.Name) || !peer.Direction.Valid() || peer.DirectDirection != "" && !peer.DirectDirection.Valid() || peer.Address != nil && peer.Address.ID != peer.ID {
			return controllerRecord{}, controllerStorage(errors.New("invalid peer identity binding"))
		}
		unique[peer.ID] = struct{}{}
	}
	if active > maxControllerPeers {
		return controllerRecord{}, controllerStorage(errors.New("paired device capacity reached"))
	}
	return record, nil
}

func (store *controllerStore) Peers(ctx context.Context) ([]protocol.NodeID, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	store.mu.Lock()
	defer store.mu.Unlock()
	peers := make([]protocol.NodeID, 0, len(store.peers))
	for _, peer := range store.peers {
		if !peer.Removed && !peer.DirectoryOnly {
			peers = append(peers, peer.ID)
		}
	}
	return peers, nil
}

func (store *controllerStore) Addresses(ctx context.Context) ([]EndpointAddress, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	store.mu.Lock()
	defer store.mu.Unlock()
	addresses := make([]EndpointAddress, 0, len(store.peers))
	for _, peer := range store.peers {
		if !peer.Removed && !peer.DirectoryOnly && peer.Address != nil {
			addresses = append(addresses, cloneAddress(*peer.Address))
		}
	}
	return addresses, nil
}

func (store *controllerStore) Remember(ctx context.Context, address EndpointAddress) error {
	return store.update(ctx, func(peers *[]controllerPeer) error {
		for index := range *peers {
			if (*peers)[index].ID == address.ID {
				copy := cloneAddress(address)
				(*peers)[index].Address = &copy
				return nil
			}
		}
		return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "peer is not paired"}
	})
}

func (store *controllerStore) SetTrust(ctx context.Context, id protocol.NodeID, trusted bool) error {
	return store.update(ctx, func(peers *[]controllerPeer) error {
		for index, peer := range *peers {
			if peer.ID == id {
				if !trusted {
					(*peers)[index].Removed = true
					(*peers)[index].Direct = false
					(*peers)[index].DirectDirection = ""
					(*peers)[index].MyDevice = false
					removeControllerSource(peers, id)
				} else {
					if (*peers)[index].Removed {
						(*peers)[index].Direction = DirectionHost
						(*peers)[index].DirectoryRevision = 0
					}
					(*peers)[index].Removed = false
					(*peers)[index].Direct = true
					(*peers)[index].DirectoryOnly = false
					(*peers)[index].DirectDirection = (*peers)[index].Direction
				}
				return nil
			}
		}
		if trusted {
			*peers = append(*peers, controllerPeer{ID: id, Direction: DirectionHost, Direct: true, DirectDirection: DirectionHost})
		}
		return nil
	})
}

func (store *controllerStore) Directions(ctx context.Context) ([]PeerTrust, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	store.mu.Lock()
	defer store.mu.Unlock()
	values := make([]PeerTrust, 0, len(store.peers))
	for _, peer := range store.peers {
		if !peer.Removed && !peer.DirectoryOnly {
			values = append(values, PeerTrust{ID: peer.ID, Direction: peer.Direction})
		}
	}
	return values, nil
}

func (store *controllerStore) PairPeer(ctx context.Context, id protocol.NodeID, direction PeerDirection) error {
	return store.PairMember(ctx, id, direction, InvitationScope{Kind: protocol.InvitationOtherUser})
}

func (store *controllerStore) ClassifyPeer(ctx context.Context, id protocol.NodeID, hasNode bool) error {
	return store.update(ctx, func(peers *[]controllerPeer) error {
		for index := range *peers {
			peer := &(*peers)[index]
			if peer.ID != id || peer.Removed || !peer.Direction.Valid() {
				continue
			}
			if peer.HasNode != nil && *peer.HasNode != hasNode {
				return invalidLink("paired device type changed")
			}
			peer.HasNode = &hasNode
			return nil
		}
		return pairingRejected()
	})
}

func (store *controllerStore) PairMember(ctx context.Context, id protocol.NodeID, direction PeerDirection, scope InvitationScope) error {
	if !direction.Valid() {
		return invalidLink("invalid peer direction")
	}
	return store.update(ctx, func(peers *[]controllerPeer) error {
		for index := range *peers {
			if (*peers)[index].ID == id {
				for removal := range (*peers)[index].Removals {
					(*peers)[index].Removals[removal].Active = false
				}
				if (*peers)[index].Removed {
					(*peers)[index].DirectoryRevision = 0
					(*peers)[index].Direction = ""
					(*peers)[index].DirectDirection = ""
				}
				(*peers)[index].Direction = mergeDirection((*peers)[index].Direction, direction)
				(*peers)[index].DirectDirection = mergeDirection((*peers)[index].DirectDirection, direction)
				(*peers)[index].Direct = true
				(*peers)[index].DirectoryOnly = false
				(*peers)[index].Removed = false
				(*peers)[index].MyDevice = (*peers)[index].MyDevice || scope.Kind == protocol.InvitationMyDevice
				return nil
			}
		}
		*peers = append(*peers, controllerPeer{ID: id, Direction: direction, Direct: true, DirectDirection: direction, MyDevice: scope.Kind == protocol.InvitationMyDevice})
		return nil
	})
}

func (store *controllerStore) SetDirection(ctx context.Context, id protocol.NodeID, direction PeerDirection) error {
	if !direction.Valid() {
		return invalidLink("invalid peer direction")
	}
	return store.update(ctx, func(peers *[]controllerPeer) error {
		for index := range *peers {
			if (*peers)[index].ID == id && !(*peers)[index].Removed {
				(*peers)[index].Direction = direction
				(*peers)[index].Direct = true
				(*peers)[index].DirectoryOnly = false
				(*peers)[index].DirectDirection = direction
				return nil
			}
		}
		return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "peer is not paired"}
	})
}

func (store *controllerStore) update(ctx context.Context, update func(*[]controllerPeer) error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	store.mu.Lock()
	defer store.mu.Unlock()
	if store.lock == nil {
		return controllerStorage(errors.New("controller store is closed"))
	}
	peers := append([]controllerPeer{}, store.peers...)
	for index := range peers {
		peers[index].Sources = append([]protocol.NodeID{}, peers[index].Sources...)
		peers[index].Removals = append([]HostRemovalAttempt{}, peers[index].Removals...)
	}
	if err := update(&peers); err != nil {
		return err
	}
	active := 0
	for _, peer := range peers {
		if !peer.Removed && !peer.DirectoryOnly {
			active++
		}
	}
	if active > maxControllerPeers {
		return &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "paired device capacity reached"}
	}
	encoded, err := json.Marshal(controllerRecord{Version: 1, Peers: peers})
	if err != nil || len(encoded) > maxControllerBytes {
		return controllerStorage(errors.New("peer record size exceeded"))
	}
	if err := persistControllerRecord(store.path, encoded); err != nil {
		return controllerStorage(err)
	}
	store.peers = peers
	if err := syncControllerDirectory(store.path); err != nil {
		return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "controller peer persistence could not be confirmed"}
	}
	return nil
}

func persistControllerRecord(directory string, encoded []byte) error {
	file, err := os.CreateTemp(directory, ".peers-")
	if err != nil {
		return err
	}
	defer os.Remove(file.Name())
	_, writeErr := file.Write(encoded)
	if writeErr == nil {
		writeErr = file.Sync()
	}
	if err := errors.Join(writeErr, file.Close()); err != nil {
		return err
	}
	return replaceControllerFile(file.Name(), filepath.Join(directory, "peers.json"))
}

func (store *controllerStore) Close() error {
	store.mu.Lock()
	defer store.mu.Unlock()
	if store.lock == nil {
		return nil
	}
	err := store.lock.Close()
	store.lock = nil
	return err
}

func controllerStorage(err error) error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "controller storage: " + err.Error()}
}
