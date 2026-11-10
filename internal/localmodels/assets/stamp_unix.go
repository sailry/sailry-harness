//go:build darwin || linux

package assets

import (
	"encoding/binary"
	"io"
)

// native is the device, inode and change time that identify one file. A
// directory is identified by its device and inode alone.
type native struct {
	device, inode        uint64
	seconds, nanoseconds int64
}

func (value native) write(output io.Writer, directory bool) {
	_ = binary.Write(output, binary.LittleEndian, value.device)
	_ = binary.Write(output, binary.LittleEndian, value.inode)
	if !directory {
		_ = binary.Write(output, binary.LittleEndian, value.seconds)
		_ = binary.Write(output, binary.LittleEndian, value.nanoseconds)
	}
}
