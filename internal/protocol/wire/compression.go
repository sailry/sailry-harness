package wire

import (
	"encoding/binary"
	"sync"

	"github.com/klauspost/compress/zstd"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const compressedFlag uint32 = 1 << 31
const minimumCompressedBytes = 1024

// One codec serves every frame: EncodeAll and DecodeAll are safe for concurrent
// use and neither starts a goroutine, so the codecs are never closed.
var frameEncoder = sync.OnceValues(func() (*zstd.Encoder, error) {
	return zstd.NewWriter(nil, zstd.WithEncoderLevel(zstd.EncoderLevelFromZstd(1)))
})

var frameDecoder = sync.OnceValues(func() (*zstd.Decoder, error) {
	return zstd.NewReader(nil,
		zstd.WithDecoderConcurrency(0),
		zstd.WithDecoderMaxMemory(uint64(protocol.MAX_FRAME_BYTES)),
		zstd.WithDecoderMaxWindow(uint64(protocol.MAX_FRAME_BYTES)),
		zstd.WithDecodeAllCapLimit(true),
	)
})

func encode(data []byte, enabled bool) (uint32, []byte, error) {
	if !validLength(len(data)) {
		return 0, nil, invalid()
	}
	if enabled && len(data) >= minimumCompressedBytes {
		encoder, err := frameEncoder()
		if err != nil {
			return 0, nil, invalid()
		}
		packed := encoder.EncodeAll(data, nil)
		defer clear(packed)
		if len(packed)+4 < len(data) {
			payload := make([]byte, len(packed)+4)
			binary.BigEndian.PutUint32(payload, uint32(len(data)))
			copy(payload[4:], packed)
			return compressedFlag | uint32(len(payload)), payload, nil
		}
	}
	return uint32(len(data)), data, nil
}

func frameSize(prefix uint32, enabled bool) (int, error) {
	if prefix&compressedFlag != 0 && !enabled {
		return 0, invalid()
	}
	size := int(prefix &^ compressedFlag)
	if !validLength(size) {
		return 0, invalid()
	}
	return size, nil
}

func decode(prefix uint32, payload []byte, enabled bool, maximum int) ([]byte, error) {
	size, err := frameSize(prefix, enabled)
	if err != nil || size != len(payload) || size > maximum {
		return nil, invalid()
	}
	if prefix&compressedFlag == 0 {
		return payload, nil
	}
	if len(payload) < 4 {
		return nil, invalid()
	}
	declared := int(binary.BigEndian.Uint32(payload[:4]))
	if !validLength(declared) || declared > maximum {
		return nil, invalid()
	}
	decoder, err := frameDecoder()
	if err != nil {
		return nil, invalid()
	}
	decoded, err := decoder.DecodeAll(payload[4:], make([]byte, 0, declared))
	if err != nil || len(decoded) != declared {
		clear(decoded)
		return nil, invalid()
	}
	return decoded, nil
}
