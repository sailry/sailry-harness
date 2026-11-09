package kit

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// Preferences are presentation choices shared by controls in one window.
type Preferences struct {
	ToastSeconds    uint64
	MessageTextSize uint8
}

type preferencesKey struct{}

func preferencesOf(c *ui.Context) *Preferences {
	return ui.Local(c.Root(), preferencesKey{}, func() Preferences { return Preferences{ToastSeconds: 5, MessageTextSize: 14} })
}

// SetPreferences keeps preferences on the window root, including its overlays.
// It must run before rendering controls or draining asynchronous UI outcomes.
func SetPreferences(c *ui.Context, p Preferences) {
	if p.ToastSeconds == 0 {
		p.ToastSeconds = 5
	}
	if p.MessageTextSize == 0 {
		p.MessageTextSize = 14
	}
	*preferencesOf(c) = p
}

// ToastDuration is the selected timeout for an ordinary expiring toast.
func ToastDuration(c *ui.Context) time.Duration {
	return time.Duration(preferencesOf(c).ToastSeconds) * time.Second
}

// MessageTextSize is the user and assistant body size in DIPs.
func MessageTextSize(c *ui.Context) float32 { return float32(preferencesOf(c).MessageTextSize) }
