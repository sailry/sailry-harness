package files

import (
	"bytes"
	"encoding/binary"
	"io"
)

// Inspect only container blocks; decoding every animation frame would exceed the
// single-image allocation budget.
func animatedGIF(data []byte) (bool, error) {
	if len(data) < 13 || !bytes.HasPrefix(data, []byte("GIF")) {
		return false, io.ErrUnexpectedEOF
	}
	offset := 13
	if data[10]&0x80 != 0 {
		offset += 3 * (1 << ((data[10] & 7) + 1))
	}
	skipBlocks := func() error {
		for {
			if offset >= len(data) {
				return io.ErrUnexpectedEOF
			}
			n := int(data[offset])
			offset++
			if n == 0 {
				return nil
			}
			if n > len(data)-offset {
				return io.ErrUnexpectedEOF
			}
			offset += n
		}
	}
	frames := 0
	for offset < len(data) {
		kind := data[offset]
		offset++
		switch kind {
		case 0x3b:
			return false, nil
		case 0x21:
			if offset >= len(data) {
				return false, io.ErrUnexpectedEOF
			}
			offset++
			if err := skipBlocks(); err != nil {
				return false, err
			}
		case 0x2c:
			if offset+9 > len(data) {
				return false, io.ErrUnexpectedEOF
			}
			flags := data[offset+8]
			offset += 9
			if flags&0x80 != 0 {
				offset += 3 * (1 << ((flags & 7) + 1))
			}
			if offset >= len(data) {
				return false, io.ErrUnexpectedEOF
			}
			offset++
			if err := skipBlocks(); err != nil {
				return false, err
			}
			frames++
			if frames >= 2 {
				return true, nil
			}
		default:
			return false, io.ErrUnexpectedEOF
		}
	}
	return false, io.ErrUnexpectedEOF
}
func animatedWebP(data []byte) bool {
	if len(data) < 12 || string(data[:4]) != "RIFF" || string(data[8:12]) != "WEBP" {
		return false
	}
	for offset := 12; offset+8 <= len(data); {
		kind := string(data[offset : offset+4])
		size := uint64(binary.LittleEndian.Uint32(data[offset+4 : offset+8]))
		offset += 8
		if size > uint64(len(data)-offset) {
			return false
		}
		if kind == "ANIM" || kind == "ANMF" {
			return true
		}
		offset += int(size + (size & 1))
	}
	return false
}
