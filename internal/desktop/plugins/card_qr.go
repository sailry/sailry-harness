package plugins

import (
	"encoding/json"
	"errors"
	"image"
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/kaptinlin/jsonpointer"
	"rsc.io/qr"
)

const CardMaxQrBytes = 2048

type cardQrCache struct {
	payload string
	image   *ui.Bitmap
	width   int
}

func (host *CardHost) renderQr(view *ui.Context, _ NativeElement, _ func(), _ func(CallbackRef, json.RawMessage)) (ui.Element, error) {
	declaration := host.binding.Declaration.Capabilities.Qr
	if host.ctx.Err() != nil || declaration == nil {
		return ui.Box(view).Size(0, 0), nil
	}
	state := host.State()
	var props any
	if json.Unmarshal(state.Props, &props) != nil {
		return ui.Box(view).Size(0, 0), nil
	}
	if declaration.ExpiresAt != nil {
		if expiry, err := jsonpointer.Value(props, *declaration.ExpiresAt); err == nil {
			if milliseconds, valid := expiry.(float64); valid && milliseconds <= float64(host.binding.Clock.Now().UnixMilli()) {
				return ui.Box(view).Size(0, 0), nil
			}
		}
	}
	value, err := jsonpointer.Value(props, declaration.Payload)
	payload, valid := value.(string)
	if err != nil || !valid || len(payload) == 0 || len(payload) > CardMaxQrBytes {
		return ui.Box(view).Size(0, 0), nil
	}
	host.mu.Lock()
	defer host.mu.Unlock()
	if host.qr.image == nil || host.qr.payload != payload {
		cached, err := encodeCardQr(payload)
		if err != nil {
			return ui.Box(view).Size(0, 0), nil
		}
		host.qr = cached
	}
	return ui.Image(view.Key(strconv.Itoa(host.qr.width)), host.qr.image).Size(float32(host.qr.width), float32(host.qr.width)).MaxWidthPercent(100).Fit(ui.Contain), nil
}

func encodeCardQr(payload string) (cardQrCache, error) {
	if len(payload) == 0 || len(payload) > CardMaxQrBytes {
		return cardQrCache{}, errors.New("QR payload exceeds the size limit")
	}
	code, err := qr.Encode(payload, qr.M)
	if err != nil {
		return cardQrCache{}, errors.New("QR payload cannot be encoded")
	}
	width := (code.Size + 8) * 4
	if width > 768 {
		return cardQrCache{}, errors.New("QR image exceeds the size limit")
	}
	pixels := image.NewGray(image.Rect(0, 0, width, width))
	for row := range width {
		for column := range width {
			if !code.Black(column/4-4, row/4-4) {
				pixels.Pix[row*pixels.Stride+column] = 255
			}
		}
	}
	return cardQrCache{payload: payload, image: ui.NewBitmap(pixels).Pixelated(), width: width}, nil
}
