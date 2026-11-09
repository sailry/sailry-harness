package core

import (
	"bytes"
	"errors"
	"unicode"
	"unicode/utf8"

	"golang.org/x/text/encoding/simplifiedchinese"
	"golang.org/x/text/transform"
)

var (
	ErrBinaryText      = errors.New("binary content cannot be decoded as text")
	ErrUnsupportedText = errors.New("text is neither valid UTF-8 nor GB18030")
)

// DecodeText preserves UTF-8 and strictly decodes the supported GB18030 fallback.
func DecodeText(data []byte) (string, error) {
	for _, value := range data {
		if value < 0x20 && value != '\t' && value != '\n' && value != '\r' && value != '\f' || value == 0x7f {
			return "", ErrBinaryText
		}
	}
	var text string
	if bytes.HasPrefix(data, []byte{0xef, 0xbb, 0xbf}) {
		data = data[3:]
		if !utf8.Valid(data) {
			return "", ErrUnsupportedText
		}
		text = string(data)
	} else if utf8.Valid(data) {
		text = string(data)
	} else {
		decoded, err := simplifiedchinese.GB18030.NewDecoder().Bytes(data)
		if err != nil {
			return "", ErrUnsupportedText
		}
		if bytes.Contains(decoded, []byte(string(utf8.RuneError))) && !strictGB18030(data) {
			return "", ErrUnsupportedText
		}
		text = string(decoded)
	}
	for _, character := range text {
		if unicode.IsControl(character) && character != '\t' && character != '\n' && character != '\r' && character != '\f' {
			return "", ErrBinaryText
		}
	}
	return text, nil
}

// The decoder substitutes malformed input. Decode one scalar at a time only
// when a replacement occurs, distinguishing an encoded U+FFFD from substitution.
func strictGB18030(data []byte) bool {
	encodedReplacement, err := simplifiedchinese.GB18030.NewEncoder().Bytes([]byte(string(utf8.RuneError)))
	if err != nil {
		return false
	}
	decoder := simplifiedchinese.GB18030.NewDecoder()
	var output [utf8.UTFMax]byte
	for len(data) != 0 {
		consumed := 0
		for size := 1; size <= len(output); size++ {
			written, read, err := decoder.Transform(output[:size], data, true)
			if read != 0 {
				if string(output[:written]) == string(utf8.RuneError) && !bytes.Equal(data[:read], encodedReplacement) {
					return false
				}
				consumed = read
				break
			}
			if !errors.Is(err, transform.ErrShortDst) {
				return false
			}
		}
		if consumed == 0 {
			return false
		}
		data = data[consumed:]
	}
	return true
}
