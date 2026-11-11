package reader

import (
	"bytes"
	"context"
	"io"
	"sync"

	"github.com/egoist/mygo/audio"
	// The speech command returns MP3.
	_ "github.com/egoist/mygo/audio/mp3"
)

// playClip plays one synthesized clip on the system's audio output, to its
// end or until ctx ends. Without an output the clip would pass unheard, so
// that is an error; so is a clip that stops decoding before its end, which
// the player ends like one that played through.
func playClip(ctx context.Context, clip []byte) error {
	decoder, _, err := audio.NewDecoder(bytes.NewReader(clip))
	if err != nil {
		return err
	}
	decoded := &decoding{Decoder: decoder}
	player := audio.NewPlayer(decoded)
	defer player.Close()
	ended := make(chan struct{}, 1)
	off := player.OnEnd(func() {
		select {
		case ended <- struct{}{}:
		default:
		}
	})
	defer off()
	player.Play()
	select {
	case <-ctx.Done():
		player.Stop()
		return ctx.Err()
	case <-ended:
		if err := decoded.failure(); err != nil {
			return err
		}
		return audio.Err()
	}
}

// decoding keeps the error that ended a clip's decoding. The player decodes
// on its own goroutine and reports nothing of it. A frame the MP3 decoder
// itself skips, because part of the read it failed in was decoded, is not
// an error here either: the clip plays on past it.
type decoding struct {
	audio.Decoder
	mu  sync.Mutex
	err error
}

func (d *decoding) Read(buf []float32) (int, error) {
	n, err := d.Decoder.Read(buf)
	if err != nil && err != io.EOF {
		d.mu.Lock()
		if d.err == nil {
			d.err = err
		}
		d.mu.Unlock()
	}
	return n, err
}

func (d *decoding) failure() error {
	d.mu.Lock()
	defer d.mu.Unlock()
	return d.err
}
