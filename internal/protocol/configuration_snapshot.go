package protocol

import (
	"encoding/binary"

	"lukechampine.com/blake3"
)

// ConfigurationPageBytes leaves room for base64 encoding and the command
// envelope within a normal frame. The snapshot bound accommodates all 4,096
// one-MiB record bodies, their metadata and execution credentials.
const ConfigurationPageBytes = MAX_FRAME_BYTES / 16
const MaxConfigurationSnapshotBytes = uint64(2 * 4096 * ConfigurationPageBytes)

// ConfigurationSnapshotPage binds every chunk to one frozen delivery. Data is
// encoded as bytes so a page can end inside any JSON string or UTF-8 sequence.
type ConfigurationSnapshotPage struct {
	Root     RequestID `json:"root"`
	Source   NodeID    `json:"source"`
	Category string    `json:"category"`
	Revision uint64    `json:"revision"`
	Bytes    uint64    `json:"bytes"`
	Digest   string    `json:"digest"`
	Index    uint64    `json:"index"`
	Data     []byte    `json:"data,omitempty"`
}

// ConfigurationSnapshotProgress is also the persistent receipt. Complete is
// published in the same transaction as the replacement cache and its events.
type ConfigurationSnapshotProgress struct {
	Root      RequestID `json:"root"`
	NextIndex uint64    `json:"next_index"`
	Complete  bool      `json:"complete"`
}

// ConfigurationPageRequestID preserves the identity of a page across retries
// and process restarts without creating a second effect ledger for cache data.
func ConfigurationPageRequestID(root RequestID, index uint64) RequestID {
	body := append([]byte("sailry.configuration.page.v1\x00"), root[:]...)
	body = binary.BigEndian.AppendUint64(body, index)
	digest := blake3.Sum256(body)
	var id RequestID
	copy(id[:], digest[:])
	id[6] = id[6]&0x0f | 0x80
	id[8] = id[8]&0x3f | 0x80
	return id
}
