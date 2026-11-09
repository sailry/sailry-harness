package wire

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"errors"
	"io"
	"math"
	"reflect"
	"strings"
	"testing"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestFramePlain(t *testing.T) {
	t.Run("wire shapes", func(t *testing.T) {
		fixtures := []struct {
			name   string
			input  string
			output string
			value  func() any
		}{
			{"legacy inspect", `{"kind":"inspect"}`, `{"software_version":null,"protocol":null,"zstd":false,"kind":"inspect"}`, func() any { return new(Envelope) }},
			{"inspect", `{"software_version":"0.1.0","protocol":1,"zstd":true,"kind":"inspect"}`, "", func() any { return new(Envelope) }},
			{"open stream", `{"kind":"open_stream","data":"01890f00-1234-7000-8000-000000000001"}`, "", func() any { return new(Call) }},
			{"subscribe", `{"kind":"subscribe","data":{"kind":"node"}}`, "", func() any { return new(Call) }},
			{"legacy endpoint", `{"kind":"endpoint","data":{"execution":true,"name":null,"platform":"linux"}}`, `{"kind":"endpoint","data":{"software_version":null,"protocol":null,"zstd":false,"execution":true,"name":null,"platform":"linux"}}`, func() any { return new(Reply) }},
			{"stream opened", `{"kind":"stream_opened"}`, "", func() any { return new(Reply) }},
			{"subscribed", `{"kind":"subscribed"}`, "", func() any { return new(Reply) }},
			{"receipt", `{"kind":"receipt","data":{"id":"01890f00-1234-7000-8000-000000000001","durable":true}}`, "", func() any { return new(Reply) }},
			{"rejected", `{"kind":"rejected","data":{"code":"permission_denied","message":"fixture denied"}}`, "", func() any { return new(Reply) }},
			{"completed failure", `{"kind":"completed","data":{"Err":{"code":"permission_denied","message":"fixture denied"}}}`, "", func() any { return new(Reply) }},
			{"completed success", `{"kind":"completed","data":{"Ok":{"kind":"notifications_updated"}}}`, "", func() any { return new(Reply) }},
			{"update", `{"kind":"update","data":{"kind":"reset_required"}}`, "", func() any { return new(Reply) }},
		}
		for _, fixture := range fixtures {
			t.Run(fixture.name, func(t *testing.T) {
				value := fixture.value()
				if err := json.Unmarshal([]byte(fixture.input), value); err != nil {
					t.Fatal(err)
				}
				var stream bytes.Buffer
				if err := Write(&stream, value, false); err != nil {
					t.Fatal(err)
				}
				frame := append([]byte(nil), stream.Bytes()...)
				if got := binary.BigEndian.Uint32(frame[:4]); int(got) != len(frame)-4 {
					t.Fatalf("plain prefix = %d, payload = %d", got, len(frame)-4)
				}
				want := fixture.output
				if want == "" {
					want = fixture.input
				}
				assertJSONEqual(t, frame[4:], []byte(want))
				decoded := fixture.value()
				if err := Read(&stream, decoded, false); err != nil {
					t.Fatal(err)
				}
				if !reflect.DeepEqual(value, decoded) {
					t.Fatalf("round trip changed value: %#v", decoded)
				}
			})
		}
	})

	t.Run("partial prefix and payload", func(t *testing.T) {
		for _, compressed := range []bool{false, true} {
			value := strings.Repeat("payload", 4096)
			var stream bytes.Buffer
			if err := Write(&stream, value, compressed); err != nil {
				t.Fatal(err)
			}
			frame := stream.Bytes()
			for _, split := range []int{2, 8} {
				reader := NewReader(compressed)
				var decoded string
				assertFault(t, reader.Read(bytes.NewReader(frame[:split]), &decoded), "unavailable")
				if split == 2 && reader.prefixRead != split {
					t.Fatalf("partial prefix retained %d bytes", reader.prefixRead)
				}
				if split > 4 && reader.payloadRead != split-4 {
					t.Fatalf("partial payload retained %d bytes", reader.payloadRead)
				}
				if err := reader.Read(bytes.NewReader(frame[split:]), &decoded); err != nil {
					t.Fatal(err)
				}
				if decoded != value || reader.prefixRead != 0 || reader.payloadRead != 0 {
					t.Fatal("resuming the interrupted frame changed its value or retained read offsets")
				}
				if !bytes.Equal(reader.payload, make([]byte, len(reader.payload))) {
					t.Fatal("completed payload was not cleared")
				}
			}
		}
	})

	t.Run("sequential frames", func(t *testing.T) {
		var stream bytes.Buffer
		for _, value := range []string{"first", "second"} {
			if err := Write(&stream, value, false); err != nil {
				t.Fatal(err)
			}
		}
		reader := NewReader(false)
		for _, want := range []string{"first", "second"} {
			var got string
			if err := reader.Read(&stream, &got); err != nil || got != want {
				t.Fatalf("read = %q, %v", got, err)
			}
		}
	})

	t.Run("invalid wire objects", func(t *testing.T) {
		for _, input := range []string{`{}`, `{"kind":"unknown"}`, `{"kind":"command"}`, `{"kind":"open_stream","data":null}`} {
			var call Call
			if err := json.Unmarshal([]byte(input), &call); err == nil {
				t.Fatalf("accepted invalid call %s", input)
			}
		}
		for _, input := range []string{`{"Ok":{"kind":"notifications_updated"},"Err":{"code":"busy","message":"busy"}}`, `{}`, `{"Other":{}}`} {
			var response Response
			if err := json.Unmarshal([]byte(input), &response); err == nil {
				t.Fatalf("accepted invalid response %s", input)
			}
		}
		if _, err := json.Marshal(Response{}); err == nil {
			t.Fatal("accepted a response without a result")
		}
		var stream bytes.Buffer
		if err := Write(&stream, json.RawMessage(`{"unfinished":`), false); err == nil {
			t.Fatal("accepted invalid JSON for writing")
		}
		if stream.Len() != 0 {
			t.Fatal("wrote bytes before validating JSON")
		}
		var value any
		assertFault(t, Read(bytes.NewReader([]byte{0, 0, 0, 1, '{'}), &value, false), "invalid_request")
		assertFault(t, Write(failingWriter{}, "value", false), "unavailable")
	})

	t.Run("snapshot paging", func(t *testing.T) {
		snapshot := snapshotFixture(t, "")
		data, err := json.Marshal(snapshot)
		if err != nil {
			t.Fatal(err)
		}
		for _, delivery := range []Delivery{DeliveryCompleted, DeliveryUpdate} {
			header := SnapshotHeader{delivery, snapshot.Node, snapshot.Cursor, uint64(len(data))}
			assembly := Assembly{}
			if reply, err := assembly.Accept(Reply{Kind: ReplySnapshotStart, SnapshotStart: &header}); err != nil || reply != nil {
				t.Fatalf("start = %#v, %v", reply, err)
			}
			if reply, err := assembly.Accept(snapshotPage(0, string(data[:100]))); err != nil || reply != nil {
				t.Fatalf("partial snapshot = %#v, %v", reply, err)
			}
			reply, err := assembly.Accept(snapshotPage(1, string(data[100:])))
			if err != nil || reply == nil || assembly.pending != nil {
				t.Fatalf("complete snapshot = %#v, %v", reply, err)
			}
			actual, actualDelivery := replySnapshot(*reply)
			if !reflect.DeepEqual(actual, &snapshot) || actualDelivery != delivery {
				t.Fatal("snapshot completion changed its captured data or delivery")
			}

			var first, second bytes.Buffer
			if err := Write(&first, Reply{Kind: ReplySnapshotStart, SnapshotStart: &header}, false); err != nil {
				t.Fatal(err)
			}
			if err := Write(&first, snapshotPage(0, string(data[:100])), false); err != nil {
				t.Fatal(err)
			}
			if err := Write(&second, snapshotPage(1, string(data[100:])), false); err != nil {
				t.Fatal(err)
			}
			reader := NewReader(false)
			_, err = reader.Reply(&first)
			assertFault(t, err, "unavailable")
			resumed, err := reader.Reply(&second)
			if err != nil {
				t.Fatal(err)
			}
			actual, actualDelivery = replySnapshot(resumed)
			if !reflect.DeepEqual(actual, &snapshot) || actualDelivery != delivery {
				t.Fatal("resuming paged delivery exposed a different snapshot")
			}
		}
	})

	t.Run("bounded UTF-8 pages", func(t *testing.T) {
		snapshot := snapshotFixture(t, strings.Repeat("界", PageBytes/3+1))
		for _, compressed := range []bool{false, true} {
			output := protocol.Output{Kind: "snapshot", Data: &snapshot}
			var stream bytes.Buffer
			if err := WriteReply(&stream, Reply{Kind: ReplyCompleted, Completed: &Response{Output: &output}}, compressed); err != nil {
				t.Fatal(err)
			}
			reader := NewReader(compressed)
			var header Reply
			if err := reader.Read(&stream, &header); err != nil || header.Kind != ReplySnapshotStart {
				t.Fatalf("snapshot header = %#v, %v", header, err)
			}
			assembly := Assembly{}
			if _, err := assembly.Accept(header); err != nil {
				t.Fatal(err)
			}
			var completed *Reply
			var pages uint64
			for stream.Len() != 0 {
				var page Reply
				if err := reader.Read(&stream, &page); err != nil {
					t.Fatal(err)
				}
				if page.Kind != ReplySnapshotPage || page.SnapshotPage.Index != pages || !utf8.ValidString(page.SnapshotPage.Data) || len(page.SnapshotPage.Data) > PageBytes {
					t.Fatalf("invalid UTF-8 snapshot page %#v", page)
				}
				pages++
				var err error
				completed, err = assembly.Accept(page)
				if err != nil {
					t.Fatal(err)
				}
			}
			if pages < 2 || completed == nil {
				t.Fatalf("snapshot completed after %d pages: %#v", pages, completed)
			}
			actual, _ := replySnapshot(*completed)
			if !reflect.DeepEqual(actual, &snapshot) {
				t.Fatal("paged writer changed the snapshot")
			}
		}
	})

	t.Run("snapshot sequence and identity", func(t *testing.T) {
		snapshot := snapshotFixture(t, "")
		data, err := json.Marshal(snapshot)
		if err != nil {
			t.Fatal(err)
		}
		header := SnapshotHeader{DeliveryUpdate, snapshot.Node, snapshot.Cursor, uint64(len(data))}
		assembly := Assembly{}
		_, err = assembly.Accept(snapshotPage(0, string(data)))
		assertFault(t, err, "invalid_request")
		for _, reply := range []Reply{
			{Kind: ReplySnapshotStart, SnapshotStart: &header},
			snapshotPage(0, ""),
			snapshotPage(0, strings.Repeat("x", PageBytes+1)),
			snapshotPage(0, string(data)+"x"),
			snapshotPage(1, string(data)),
			{Kind: ReplySubscribed},
		} {
			assembly := Assembly{}
			if _, err := assembly.Accept(Reply{Kind: ReplySnapshotStart, SnapshotStart: &header}); err != nil {
				t.Fatal(err)
			}
			_, err := assembly.Accept(reply)
			assertFault(t, err, "invalid_request")
		}
		for _, changeNode := range []bool{false, true} {
			changed := header
			if changeNode {
				changed.Node[0]++
			} else {
				changed.Cursor++
			}
			assembly := Assembly{}
			if _, err := assembly.Accept(Reply{Kind: ReplySnapshotStart, SnapshotStart: &changed}); err != nil {
				t.Fatal(err)
			}
			_, err := assembly.Accept(snapshotPage(0, string(data)))
			assertFault(t, err, "invalid_request")
		}
	})
}

func TestFrameSmallNotCompressed(t *testing.T) {
	for _, size := range []int{1, minimumCompressedBytes - 1} {
		data := bytes.Repeat([]byte{'x'}, size)
		prefix, payload, err := encode(data, true)
		if err != nil || prefix != uint32(size) || !bytes.Equal(payload, data) {
			t.Fatalf("small frame = %d, %d bytes, %v", prefix, len(payload), err)
		}
	}
	data := make([]byte, 4096)
	state := uint32(1)
	for index := range data {
		state ^= state << 13
		state ^= state >> 17
		state ^= state << 5
		data[index] = byte(state)
	}
	prefix, payload, err := encode(data, true)
	if err != nil || prefix != uint32(len(data)) || !bytes.Equal(payload, data) {
		t.Fatalf("nonbeneficial compression = %d, %d bytes, %v", prefix, len(payload), err)
	}
}

func TestFrameCompressed(t *testing.T) {
	value := strings.Repeat("compressible payload", 4096)
	var stream bytes.Buffer
	if err := Write(&stream, value, true); err != nil {
		t.Fatal(err)
	}
	frame := stream.Bytes()
	prefix := binary.BigEndian.Uint32(frame[:4])
	if prefix&compressedFlag == 0 || int(prefix&^compressedFlag) != len(frame)-4 || len(frame) >= len(value) {
		t.Fatalf("compressed frame prefix = %d, bytes = %d", prefix, len(frame))
	}
	var decoded string
	if err := Read(&stream, &decoded, true); err != nil || decoded != value {
		t.Fatalf("decompressed value changed: %v", err)
	}
	payload := frame[4:]
	declared := binary.BigEndian.Uint32(payload[:4])
	for _, wrong := range []uint32{0, declared - 1, declared + 1, protocol.MAX_FRAME_BYTES + 1} {
		changed := append([]byte(nil), payload...)
		binary.BigEndian.PutUint32(changed, wrong)
		_, err := decode(prefix, changed, true, protocol.MAX_FRAME_BYTES)
		assertFault(t, err, "invalid_request")
	}
	for _, malformed := range [][]byte{{0, 0, 0}, {0, 0, 4, 0, 1, 2, 3}} {
		_, err := decode(compressedFlag|uint32(len(malformed)), malformed, true, protocol.MAX_FRAME_BYTES)
		assertFault(t, err, "invalid_request")
	}
	_, err := decode(prefix, payload, true, int(declared)-1)
	assertFault(t, err, "invalid_request")
	_, err = decode(prefix+1, payload, true, protocol.MAX_FRAME_BYTES)
	assertFault(t, err, "invalid_request")
}

func TestFrameCompressionNotNegotiated(t *testing.T) {
	var stream bytes.Buffer
	if err := Write(&stream, strings.Repeat("x", 4096), true); err != nil {
		t.Fatal(err)
	}
	reader := NewReader(false)
	var value string
	assertFault(t, reader.Read(&stream, &value), "invalid_request")
	if reader.payload != nil || reader.payloadRead != 0 || stream.Len() == 0 {
		t.Fatal("read or allocated an unnegotiated compressed payload")
	}
	prefix, payload, err := encode(bytes.Repeat([]byte{'x'}, 4096), false)
	if err != nil || prefix&compressedFlag != 0 || len(payload) != 4096 {
		t.Fatalf("unnegotiated write = %d, %d bytes, %v", prefix, len(payload), err)
	}
}

func TestFrameOversizeBeforeAlloc(t *testing.T) {
	for _, prefix := range []uint32{0, protocol.MAX_FRAME_BYTES + 1, compressedFlag | (protocol.MAX_FRAME_BYTES + 1), math.MaxUint32} {
		var data [5]byte
		binary.BigEndian.PutUint32(data[:4], prefix)
		stream := bytes.NewReader(data[:])
		reader := NewReader(true)
		var value any
		assertFault(t, reader.Read(stream, &value), "invalid_request")
		if reader.payload != nil || stream.Len() != 1 {
			t.Fatalf("prefix %d caused a payload allocation or read", prefix)
		}
	}
	var bounded [5]byte
	binary.BigEndian.PutUint32(bounded[:4], 17)
	stream := bytes.NewReader(bounded[:])
	var value any
	assertFault(t, ReadLimited(stream, &value, 16), "invalid_request")
	if stream.Len() != 1 {
		t.Fatal("read payload before enforcing the smaller limit")
	}
	if !validLength(protocol.MAX_FRAME_BYTES) || validLength(protocol.MAX_FRAME_BYTES+1) || validLength(0) {
		t.Fatal("frame length boundary changed")
	}
	var output bytes.Buffer
	assertFault(t, Write(&output, strings.Repeat("x", protocol.MAX_FRAME_BYTES), true), "invalid_request")
	if output.Len() != 0 {
		t.Fatal("wrote an oversized frame")
	}
	header := SnapshotHeader{DeliveryUpdate, protocol.NodeID{1}, 7, math.MaxUint64}
	assembly := Assembly{}
	if _, err := assembly.Accept(Reply{Kind: ReplySnapshotStart, SnapshotStart: &header}); err != nil {
		t.Fatal(err)
	}
	if cap(assembly.pending.data) != 0 {
		t.Fatal("allocated the peer's declared snapshot size")
	}
	if reply, err := assembly.Accept(snapshotPage(0, "x")); err != nil || reply != nil || string(assembly.pending.data) != "x" {
		t.Fatalf("bounded snapshot page = %#v, %v", reply, err)
	}
	header.Bytes = 0
	assembly = Assembly{}
	_, err := assembly.Accept(Reply{Kind: ReplySnapshotStart, SnapshotStart: &header})
	assertFault(t, err, "invalid_request")
	page, err := json.Marshal(snapshotPage(0, strings.Repeat("\x00", PageBytes)))
	if err != nil || len(page) >= protocol.MAX_FRAME_BYTES {
		t.Fatalf("escaped page exceeds frame limit: %d bytes, %v", len(page), err)
	}
}

type failingWriter struct{}

func (failingWriter) Write([]byte) (int, error) { return 0, io.ErrClosedPipe }

func assertFault(t *testing.T, err error, code string) {
	t.Helper()
	var fault *protocol.Fault
	if !errors.As(err, &fault) || string(fault.Code) != code {
		t.Fatalf("fault = %v, expected %s", err, code)
	}
}

func assertJSONEqual(t *testing.T, actual, expected []byte) {
	t.Helper()
	var got, want any
	if err := json.Unmarshal(actual, &got); err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(expected, &want); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("JSON = %s, expected %s", actual, expected)
	}
}

func snapshotPage(index uint64, data string) Reply {
	return Reply{Kind: ReplySnapshotPage, SnapshotPage: &SnapshotPage{index, data}}
}

func snapshotFixture(t *testing.T, batch string) protocol.Snapshot {
	t.Helper()
	encoded, err := json.Marshal(batch)
	if err != nil {
		t.Fatal(err)
	}
	data := `{"node":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],"cursor":7,"defaults":{"revision":0,"config":null},"projects":[],"worktrees":[],"sessions":[],"terminals":[],"terminal_settings_revision":0,"power_settings_revision":0,"sandbox_settings_revision":0,"search_settings_revision":0,"media_settings":{"revision":0,"bindings":{}},"providers":[],"provider_quotas":[],"model_catalog":{"revision":0,"updated_at_ms":null,"providers":0,"models":0,"bytes":0,"fault":null,"scores":{"revision":0,"updated_at_ms":null,"checked_at_ms":null,"batch":` + string(encoded) + `,"published_at":null,"models":0,"fault":null}},"roles":[],"teams":[],"team_settings_revision":0,"compaction_settings_revision":0,"ssh":[],"databases":[],"plugins":[],"notifications":[],"turns":[]}`
	var snapshot protocol.Snapshot
	if err := json.Unmarshal([]byte(data), &snapshot); err != nil {
		t.Fatal(err)
	}
	return snapshot
}
