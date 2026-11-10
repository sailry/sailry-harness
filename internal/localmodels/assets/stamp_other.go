//go:build !darwin && !linux

package assets

import (
	"encoding/binary"
	"io"
)

// native is the creation time that identifies one file where the platform
// reports it. A directory carries no native identity.
type native struct {
	created int64
}

func (value native) write(output io.Writer, directory bool) {
	if !directory {
		_ = binary.Write(output, binary.LittleEndian, value.created)
	}
}
