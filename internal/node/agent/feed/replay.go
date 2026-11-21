package feed

import (
	"bytes"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	maxRetained      = 512
	maxRetainedBytes = 4 * 1024 * 1024
)

// replay owns the immutable encoded changes in one contiguous suffix for all
// observers. Ordinary frames share a byte budget and at most one larger frame
// can coexist within the count bound.
type replay struct {
	frames   []retainedFrame
	first    uint64
	count    int
	bytes    int
	large    bool
	metadata map[metadataKey]uint64
}

type retainedFrame struct {
	body []byte
	key  metadataKey
}

type metadataKey struct {
	kind string
	turn protocol.TurnID
}

func (r *replay) at(sequence uint64) []byte {
	if r.count == 0 || sequence < r.first || sequence-r.first >= uint64(r.count) {
		return nil
	}
	return r.frames[sequence%maxRetained].body
}

func (r *replay) duplicate(key metadataKey, body []byte) bool {
	sequence, ok := r.metadata[key]
	return ok && bytes.Equal(r.at(sequence), body)
}

func (r *replay) append(sequence uint64, body []byte, key metadataKey) {
	if len(body) > protocol.MAX_FRAME_BYTES {
		// The change alone already exceeds Link's envelope limit. Leave a gap
		// so observers recover from canonical history instead of waiting for it.
		*r = replay{}
		return
	}
	if cap(body) > protocol.MAX_FRAME_BYTES {
		exact := make([]byte, len(body))
		copy(exact, body)
		body = exact
	}
	size := cap(body)
	large := size > maxRetainedBytes
	for r.count == maxRetained || large && r.large || !large && r.bytes+size > maxRetainedBytes {
		r.evict()
	}
	if r.frames == nil {
		r.frames = make([]retainedFrame, maxRetained)
	}
	if r.count == 0 {
		r.first = sequence
	}
	r.frames[sequence%maxRetained] = retainedFrame{body: body, key: key}
	r.count++
	if large {
		r.large = true
	} else {
		r.bytes += size
	}
	if key.kind != "" {
		if r.metadata == nil {
			r.metadata = make(map[metadataKey]uint64)
		}
		r.metadata[key] = sequence
	}
}

func (r *replay) evict() {
	index := r.first % maxRetained
	old := r.frames[index]
	if cap(old.body) > maxRetainedBytes {
		r.large = false
	} else {
		r.bytes -= cap(old.body)
	}
	if r.metadata[old.key] == r.first {
		delete(r.metadata, old.key)
	}
	r.frames[index] = retainedFrame{}
	r.first++
	r.count--
}
