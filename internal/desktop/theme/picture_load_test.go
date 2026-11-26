package theme

import (
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

// settlePictures waits for the background loads the decorations started.
func settlePictures(decorations ...*Decoration) {
	for _, d := range decorations {
		if d != nil && d.pic != nil {
			for _, load := range d.pic.loads {
				load.wait()
			}
		}
	}
}

// loadedBitmap is d's bitmap once a load its first request started ends.
func loadedBitmap(d *Decoration) *ui.Bitmap {
	d.Bitmap()
	settlePictures(d)
	return d.Bitmap()
}

// An installed picture loads off the frame that first asks for it and wakes
// the window; a failed read is asked again rather than kept for the
// revision, and a closed window's watcher is not called.
func TestInstalledPictureLoadsInBackgroundAndRecovers(t *testing.T) {
	profile := NewProfile(t.TempDir())
	snapshot := profileFixture(t, "coast", "0.1.0")
	if err := profile.Install(t.Context(), snapshot, nil); err != nil {
		t.Fatal(err)
	}
	installed, err := profile.Installed(t.Context())
	if err != nil || len(installed) != 1 {
		t.Fatalf("installed themes = %d, %v", len(installed), err)
	}
	art := installed[0].Package.Light.Asset("background.content")
	woken := make(chan struct{}, 8)
	release := WatchPictures(func() {
		select {
		case woken <- struct{}{}:
		default:
		}
	})
	t.Cleanup(release)

	dir := filepath.Join(profile.Root, "coast")
	moved := dir + ".away"
	if err := os.Rename(dir, moved); err != nil {
		t.Fatal(err)
	}
	if art.Bitmap() != nil {
		t.Fatal("the first request returned a bitmap read on the calling thread")
	}
	settlePictures(art)
	if art.Bitmap() != nil {
		t.Fatal("an unreadable picture produced a bitmap")
	}
	settlePictures(art)
	select {
	case <-woken:
		t.Fatal("a failed load woke the window")
	default:
	}
	if err := os.Rename(moved, dir); err != nil {
		t.Fatal(err)
	}
	// End this picture's pause instead of waiting it out.
	for _, load := range art.pic.loads {
		load.mu.Lock()
		load.retry = time.Time{}
		load.mu.Unlock()
	}
	if loadedBitmap(art) == nil {
		t.Fatal("a transient read failure stayed cached for this revision")
	}
	<-woken

	release()
	other := installed[0].Package.Dark.Asset("background.content")
	if loadedBitmap(other) == nil {
		t.Fatal("the dark picture did not load")
	}
	select {
	case <-woken:
		t.Fatal("a released watcher was woken")
	default:
	}
}
