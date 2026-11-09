package wire

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"io"

	"github.com/sailry/sailry-harness/internal/protocol"
)

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
	var header [4]byte
	binary.BigEndian.PutUint32(header[:], prefix)
	if _, err := io.Copy(stream, bytes.NewReader(header[:])); err != nil {
		return network()
	}
	if _, err := io.Copy(stream, bytes.NewReader(payload)); err != nil {
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
	if len(reader.payload) != size {
		reader.payload = make([]byte, size)
	}
	if reader.payloadRead < size {
		count, err := io.ReadFull(stream, reader.payload[reader.payloadRead:])
		reader.payloadRead += count
		if err != nil {
			return network()
		}
	}
	data, err := decode(prefix, reader.payload, reader.compressed, maximum)
	if err != nil {
		return err
	}
	err = json.Unmarshal(data, value)
	clear(data)
	reader.prefixRead = 0
	reader.payloadRead = 0
	clear(reader.payload)
	if err != nil {
		return invalid()
	}
	return nil
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
