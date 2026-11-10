package ocr

import (
	"bytes"
	"errors"
	"image"
	_ "image/gif"  // GIF input
	_ "image/jpeg" // JPEG input
	_ "image/png"  // PNG input
	"path/filepath"
	"strings"

	_ "golang.org/x/image/webp" // WebP input
)

// ErrImageSize reports an encoded image whose dimensions exceed the limit.
var ErrImageSize = errors.New("OCR image dimensions exceed their limit")

// Supported reports whether a file name has the extension of an image format
// that Decode reads.
func Supported(name string) bool {
	switch strings.ToLower(strings.TrimPrefix(filepath.Ext(name), ".")) {
	case "png", "jpg", "jpeg", "webp", "gif":
		return true
	}
	return false
}

// Decode reads an encoded image, recognizing its format from the content.
// Dimensions are checked before any pixel is decoded.
func Decode(data []byte) (*RGB, error) {
	config, _, err := image.DecodeConfig(bytes.NewReader(data))
	if err != nil {
		return nil, err
	}
	if config.Width <= 0 || config.Height <= 0 || int64(config.Width)*int64(config.Height) > MaxImagePixels {
		return nil, ErrImageSize
	}
	decoded, _, err := image.Decode(bytes.NewReader(data))
	if err != nil {
		return nil, err
	}
	return FromImage(decoded), nil
}
