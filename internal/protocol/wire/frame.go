package wire

import (
	"encoding/binary"
	"encoding/json"
	"io"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const payloadStep = 64 << 10

type Reader struct {
	prefix      [4]byte
	prefixRead  int
	payload     []byte
	payloadRead int
	assembly    Assembly
	compressed  bool
}

func NewReader(compressed bool) *Reader {
	return &Reader{compressed: compressed}
}

func Write(stream io.Writer, value any, compressed bool) error {
	data, err := json.Marshal(value)
	if err != nil {
		return invalid()
	}
	defer clear(data)
	prefix, payload, err := encode(data, compressed)
	if err != nil {
		return err
	}
	defer clear(payload)
	frame := make([]byte, 4+len(payload))
	defer clear(frame)
	binary.BigEndian.PutUint32(frame, prefix)
	copy(frame[4:], payload)
	if _, err := stream.Write(frame); err != nil {
		return network()
	}
	return nil
}

func Read(stream io.Reader, value any, compressed bool) error {
	return NewReader(compressed).Read(stream, value)
}

func ReadLimited(stream io.Reader, value any, maximum int) error {
	reader := NewReader(false)
	return reader.read(stream, value, maximum)
}

func (reader *Reader) Read(stream io.Reader, value any) error {
	return reader.read(stream, value, protocol.MAX_FRAME_BYTES)
}

func (reader *Reader) read(stream io.Reader, value any, maximum int) error {
	if reader.prefixRead < len(reader.prefix) {
		count, err := io.ReadFull(stream, reader.prefix[reader.prefixRead:])
		reader.prefixRead += count
		if err != nil {
			return network()
		}
	}
	prefix := binary.BigEndian.Uint32(reader.prefix[:])
	size, err := frameSize(prefix, reader.compressed)
	if err != nil || size > maximum {
		return invalid()
	}
	// The buffer follows the bytes that arrived, so a declared length alone
	// reserves no more than one step.
	for reader.payloadRead < size {
		end := min(size, reader.payloadRead+payloadStep)
		if cap(reader.payload) < end {
			grown := make([]byte, reader.payloadRead, min(size, max(end, 2*cap(reader.payload))))
			copy(grown, reader.payload[:reader.payloadRead])
			clear(reader.payload[:cap(reader.payload)])
			reader.payload = grown
		}
		reader.payload = reader.payload[:end]
		count, err := io.ReadFull(stream, reader.payload[reader.payloadRead:])
		reader.payloadRead += count
		if err != nil {
			return network()
		}
	}
	reader.payload = reader.payload[:size]
	data, err := decode(prefix, reader.payload, reader.compressed, maximum)
	if err != nil {
		return err
	}
	err = protocol.DecodeAs(tolerance(value), data, value)
	clear(data)
	reader.prefixRead = 0
	reader.payloadRead = 0
	clear(reader.payload)
	if err != nil {
		return invalid()
	}
	return nil
}

// tolerance is what reading a frame does with input this build does not
// declare. A reply is a peer's report and an envelope is a peer's request;
// both come from a build of the same protocol version that may declare more.
// Every other frame, such as a pairing claim, is checked exactly.
func tolerance(value any) protocol.Tolerance {
	switch value.(type) {
	case *Reply:
		return protocol.PeerData
	case *Envelope:
		return protocol.PeerCommand
	}
	return protocol.Authored
}

func (reader *Reader) Reply(stream io.Reader) (Reply, error) {
	for {
		var incoming Reply
		if err := reader.Read(stream, &incoming); err != nil {
			return Reply{}, err
		}
		reply, err := reader.assembly.Accept(incoming)
		if err != nil {
			return Reply{}, err
		}
		if reply != nil {
			return *reply, nil
		}
	}
}

func validLength(size int) bool {
	return size > 0 && size <= protocol.MAX_FRAME_BYTES
}

func invalid() error {
	return &protocol.Fault{Code: "invalid_request", Message: "invalid Link frame"}
}

func network() error {
	return &protocol.Fault{Code: "unavailable", Message: "Link connection is unavailable"}
}
