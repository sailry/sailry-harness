package wire

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"strconv"
	"strings"
	"testing"

	"github.com/klauspost/compress/zstd"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestFrameCompressionCompatibility(t *testing.T) {
	const legacyWindow = 4 << 20
	const pattern = "large compressed reply payload "
	value := strings.Repeat(pattern, legacyWindow/len(pattern)+1)
	plain, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}

	t.Run("legacy writer", func(t *testing.T) {
		legacy, err := zstd.NewWriter(nil,
			zstd.WithEncoderLevel(zstd.EncoderLevelFromZstd(1)),
			zstd.WithEncoderConcurrency(1),
			zstd.WithWindowSize(legacyWindow),
		)
		if err != nil {
			t.Fatal(err)
		}
		packed := legacy.EncodeAll(plain, nil)
		var header zstd.Header
		if err := header.Decode(packed); err != nil || header.WindowSize != legacyWindow {
			t.Fatalf("legacy zstd window = %d, %v", header.WindowSize, err)
		}

		frame := binary.BigEndian.AppendUint32(nil, compressedFlag|uint32(len(packed)+4))
		frame = binary.BigEndian.AppendUint32(frame, uint32(len(plain)))
		frame = append(frame, packed...)
		got, err := NewReader(true).Raw(bytes.NewReader(frame))
		if err != nil || !bytes.Equal(got, plain) {
			t.Fatalf("reading legacy frame = %d bytes, %v", len(got), err)
		}
	})

	t.Run("legacy reader", func(t *testing.T) {
		var stream bytes.Buffer
		if err := Write(&stream, value, true); err != nil {
			t.Fatal(err)
		}
		frame := stream.Bytes()
		if len(frame) < 8 || binary.BigEndian.Uint32(frame[:4])&compressedFlag == 0 ||
			binary.BigEndian.Uint32(frame[4:8]) != uint32(len(plain)) {
			t.Fatal("missing compressed frame header")
		}
		var header zstd.Header
		if err := header.Decode(frame[8:]); err != nil || header.SingleSegment || header.WindowSize > frameEncoderWindow {
			t.Fatalf("outbound zstd header = %+v, %v", header, err)
		}

		legacy, err := zstd.NewReader(nil,
			zstd.WithDecoderConcurrency(1),
			zstd.WithDecoderMaxMemory(uint64(protocol.MAX_FRAME_BYTES)),
			zstd.WithDecoderMaxWindow(uint64(protocol.MAX_FRAME_BYTES)),
			zstd.WithDecodeAllCapLimit(true),
		)
		if err != nil {
			t.Fatal(err)
		}
		defer legacy.Close()
		got, err := legacy.DecodeAll(frame[8:], make([]byte, 0, len(plain)))
		if err != nil || !bytes.Equal(got, plain) {
			t.Fatalf("legacy reader decoded %d bytes, %v", len(got), err)
		}
	})
}

func TestConcurrentCompressedFrames(t *testing.T) {
	for i := range 14 {
		t.Run(strconv.Itoa(i), func(t *testing.T) {
			t.Parallel()
			pattern := t.Name() + "\n"
			want := strings.Repeat(pattern, (128<<10)/len(pattern))
			var stream bytes.Buffer
			if err := Write(&stream, want, true); err != nil {
				t.Fatal(err)
			}
			frame := stream.Bytes()
			if len(frame) < 4 || binary.BigEndian.Uint32(frame[:4])&compressedFlag == 0 {
				t.Fatal("expected a compressed frame")
			}
			var got string
			if err := Read(&stream, &got, true); err != nil || got != want {
				t.Fatalf("concurrent frame decoded %d bytes, %v", len(got), err)
			}
		})
	}
}
