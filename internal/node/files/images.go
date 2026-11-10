package files

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/binary"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
	_ "golang.org/x/image/bmp"
	xdraw "golang.org/x/image/draw"
	_ "golang.org/x/image/webp"
	"image"
	"image/color"
	"image/draw"
	_ "image/gif"
	"image/jpeg"
	"image/png"
)

func ReadImage(ctx context.Context, f *core.Files, path string) (protocol.FileImage, error) {
	out := protocol.DefaultFileImage()
	out.Path = path
	handle, e := f.Open(path)
	if e != nil {
		return out, e
	}
	defer handle.Close()
	before, e := handle.Stat()
	if e != nil {
		return out, e
	}
	data, e := readBounded(ctx, handle, protocol.MAX_IMAGE_BYTES)
	if e != nil {
		return out, e
	}
	after, e := handle.Stat()
	if e != nil {
		return out, e
	}
	if before.Size() != after.Size() || before.ModTime() != after.ModTime() || int64(len(data)) != before.Size() {
		return out, conflict("file changed during inspection")
	}
	out.Revision = revision(data)
	source, sent, encoded, e := PrepareImage(data)
	if e != nil {
		return out, e
	}
	out.Source, out.Sent = source, sent
	mime := source.MimeType
	if sent != nil {
		mime = sent.MimeType
	}
	out.InlineData = []protocol.InlineImage{{MimeType: mime, Data: base64.StdEncoding.EncodeToString(encoded)}}
	return out, nil
}
func PrepareImage(data []byte) (protocol.ImageFacts, *protocol.ImageFacts, []byte, error) {
	var facts protocol.ImageFacts
	if len(data) > protocol.MAX_IMAGE_BYTES {
		return facts, nil, nil, invalid("image exceeds the inspection limit")
	}
	config, format, e := image.DecodeConfig(bytes.NewReader(data))
	if e != nil {
		return facts, nil, nil, invalid("image could not be decoded")
	}
	if format != "png" && format != "jpeg" && format != "gif" && format != "webp" && format != "bmp" {
		return facts, nil, nil, invalid("image format is not supported")
	}
	if config.Width <= 0 || config.Height <= 0 || uint64(config.Width)*uint64(config.Height) > 256*1024*1024/4 {
		return facts, nil, nil, invalid("image dimensions exceed the decoding limit")
	}
	mime := "image/" + format
	if format == "jpeg" {
		mime = "image/jpeg"
	}
	facts = protocol.ImageFacts{MimeType: mime, Width: uint32(config.Width), Height: uint32(config.Height), Size: uint64(len(data))}
	accepted := format == "png" || format == "jpeg"
	if format == "gif" {
		animated, e := animatedGIF(data)
		if e != nil {
			return facts, nil, nil, invalid("image could not be decoded")
		}
		accepted = !animated
	}
	if format == "webp" {
		accepted = !animatedWebP(data)
	}
	orientation := jpegOrientation(data)
	if accepted && config.Width <= protocol.MAX_IMAGE_SIDE && config.Height <= protocol.MAX_IMAGE_SIDE && len(data) <= 3*1024*1024 {
		return facts, nil, data, nil
	}
	decoded, _, e := image.Decode(bytes.NewReader(data))
	if e != nil {
		return facts, nil, nil, invalid("image could not be decoded")
	}
	decoded = orientImage(decoded, orientation)
	bounds := decoded.Bounds()
	w, h := bounds.Dx(), bounds.Dy()
	if max(w, h) > protocol.MAX_IMAGE_SIDE {
		scale := float64(protocol.MAX_IMAGE_SIDE) / float64(max(w, h))
		w, h = max(1, int(float64(w)*scale)), max(1, int(float64(h)*scale))
	}
	resized := image.NewNRGBA(image.Rect(0, 0, w, h))
	xdraw.BiLinear.Scale(resized, resized.Bounds(), decoded, decoded.Bounds(), draw.Src, nil)
	var encoded bytes.Buffer
	mime = "image/png"
	if format != "jpeg" {
		e = png.Encode(&encoded, resized)
	}
	if format == "jpeg" || encoded.Len() > 3*1024*1024 {
		encoded.Reset()
		opaque := image.NewRGBA(resized.Bounds())
		draw.Draw(opaque, opaque.Bounds(), image.NewUniform(color.White), image.Point{}, draw.Src)
		draw.Draw(opaque, opaque.Bounds(), resized, image.Point{}, draw.Over)
		e = jpeg.Encode(&encoded, opaque, &jpeg.Options{Quality: 85})
		mime = "image/jpeg"
	}
	if e != nil {
		return facts, nil, nil, invalid("image could not be encoded")
	}
	if encoded.Len() > 3*1024*1024 {
		return facts, nil, nil, invalid("image cannot be reduced below the model input limit")
	}
	sent := &protocol.ImageFacts{MimeType: mime, Width: uint32(w), Height: uint32(h), Size: uint64(encoded.Len())}
	return facts, sent, encoded.Bytes(), nil
}

// EXIF orientation is a single TIFF tag. Other metadata is left untouched.
func jpegOrientation(data []byte) int {
	if len(data) < 4 || data[0] != 0xff || data[1] != 0xd8 {
		return 1
	}
	for offset := 2; offset+4 <= len(data); {
		if data[offset] != 0xff {
			return 1
		}
		marker := data[offset+1]
		offset += 2
		if marker == 0xda || marker == 0xd9 {
			return 1
		}
		length := int(binary.BigEndian.Uint16(data[offset : offset+2]))
		if length < 2 || offset+length > len(data) {
			return 1
		}
		segment := data[offset+2 : offset+length]
		offset += length
		if marker != 0xe1 || !bytes.HasPrefix(segment, []byte("Exif\x00\x00")) {
			continue
		}
		tiff := segment[6:]
		if len(tiff) < 8 {
			return 1
		}
		var order binary.ByteOrder = binary.LittleEndian
		if string(tiff[:2]) == "MM" {
			order = binary.BigEndian
		} else if string(tiff[:2]) != "II" {
			return 1
		}
		index := uint64(order.Uint32(tiff[4:8]))
		if index+2 > uint64(len(tiff)) {
			return 1
		}
		count := uint64(order.Uint16(tiff[index : index+2]))
		for i := uint64(0); i < count; i++ {
			at := index + 2 + i*12
			if at+12 > uint64(len(tiff)) {
				return 1
			}
			entry := tiff[at : at+12]
			if order.Uint16(entry[:2]) == 0x112 && order.Uint16(entry[2:4]) == 3 && order.Uint32(entry[4:8]) == 1 {
				value := int(order.Uint16(entry[8:10]))
				if value >= 1 && value <= 8 {
					return value
				}
			}
		}
	}
	return 1
}
func orientImage(source image.Image, orientation int) image.Image {
	if orientation <= 1 || orientation > 8 {
		return source
	}
	b := source.Bounds()
	w, h := b.Dx(), b.Dy()
	nw, nh := w, h
	if orientation >= 5 {
		nw, nh = h, w
	}
	target := image.NewNRGBA(image.Rect(0, 0, nw, nh))
	for y := 0; y < nh; y++ {
		for x := 0; x < nw; x++ {
			sx, sy := x, y
			switch orientation {
			case 2:
				sx = w - 1 - x
			case 3:
				sx, sy = w-1-x, h-1-y
			case 4:
				sy = h - 1 - y
			case 5:
				sx, sy = y, x
			case 6:
				sx, sy = y, h-1-x
			case 7:
				sx, sy = w-1-y, h-1-x
			case 8:
				sx, sy = w-1-y, x
			}
			target.Set(x, y, source.At(b.Min.X+sx, b.Min.Y+sy))
		}
	}
	return target
}
