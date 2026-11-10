package wsl

import (
	"bytes"
	"encoding/binary"
	"slices"
	"strings"
	"unicode"
	"unicode/utf16"
	"unicode/utf8"
)

// decodeDistributions reads the output of `wsl.exe --list --quiet`: UTF-16
// with or without a byte order mark (little-endian by default), or UTF-8.
// Names keep their spaces and Unicode and are returned sorted and unique.
func decodeDistributions(body []byte) ([]string, error) {
	malformed := discoveryFailure("distribution list is malformed")
	little, big := bytes.HasPrefix(body, []byte{0xff, 0xfe}), bytes.HasPrefix(body, []byte{0xfe, 0xff})
	var text string
	if little || big || bytes.IndexByte(body, 0) >= 0 {
		if little || big {
			body = body[2:]
		}
		if len(body)%2 != 0 {
			return nil, malformed
		}
		var order binary.ByteOrder = binary.LittleEndian
		if big {
			order = binary.BigEndian
		}
		units := make([]uint16, len(body)/2)
		for index := range units {
			units[index] = order.Uint16(body[2*index:])
		}
		decoded, ok := decodeUTF16(units)
		if !ok {
			return nil, malformed
		}
		text = decoded
	} else {
		if !utf8.Valid(body) {
			return nil, malformed
		}
		text = strings.TrimLeft(string(body), "\uFEFF")
	}
	names := []string{}
	text = strings.TrimRight(text, "\x00")
	for text != "" {
		name, rest, terminated := strings.Cut(text, "\n")
		text = rest
		// Only a carriage return that belongs to a line ending is dropped.
		if terminated {
			name = strings.TrimSuffix(name, "\r")
		}
		if name == "" {
			continue
		}
		if strings.ContainsFunc(name, unicode.IsControl) {
			return nil, malformed
		}
		names = append(names, name)
	}
	slices.Sort(names)
	return slices.Compact(names), nil
}

// decodeUTF16 rejects unpaired surrogates instead of replacing them.
func decodeUTF16(units []uint16) (string, bool) {
	var text strings.Builder
	for index := 0; index < len(units); index++ {
		char := rune(units[index])
		if utf16.IsSurrogate(char) {
			if index+1 == len(units) {
				return "", false
			}
			char = utf16.DecodeRune(char, rune(units[index+1]))
			if char == unicode.ReplacementChar {
				return "", false
			}
			index++
		}
		text.WriteRune(char)
	}
	return text.String(), true
}
