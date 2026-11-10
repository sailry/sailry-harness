package iroh

import (
	"encoding/hex"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type Address struct {
	ID    protocol.NodeID
	Addrs []json.RawMessage
}

func (address Address) MarshalJSON() ([]byte, error) {
	hints := address.Addrs
	if hints == nil {
		hints = []json.RawMessage{}
	}
	return json.Marshal(struct {
		ID    string            `json:"id"`
		Addrs []json.RawMessage `json:"addrs"`
	}{ID: address.ID.String(), Addrs: hints})
}

func (address *Address) UnmarshalJSON(data []byte) error {
	var encoded struct {
		ID    string            `json:"id"`
		Addrs []json.RawMessage `json:"addrs"`
	}
	if err := json.Unmarshal(data, &encoded); err != nil {
		return err
	}
	identity, err := decodeIdentity(encoded.ID)
	if err != nil {
		return err
	}
	*address = Address{ID: identity, Addrs: encoded.Addrs}
	return nil
}

func decodeIdentity(value string) (protocol.NodeID, error) {
	var identity protocol.NodeID
	if len(value) != hex.EncodedLen(len(identity)) {
		return identity, errors.New("iroh endpoint identity must contain 32 bytes")
	}
	_, err := hex.Decode(identity[:], []byte(value))
	return identity, err
}
