package theme

import (
	"bytes"
	"crypto/sha256"
	"errors"
	"fmt"
	"image"
	"image/color"
	"io"
	"io/fs"
	"os"
	"path"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
)

const (
	themeManifestLimit = 256 * 1024
	themeFileLimit     = 8 * 1024 * 1024
	themePackageLimit  = 32 * 1024 * 1024
	themeDecodeLimit   = 32 * 1024 * 1024
	themePreviewEdge   = 768
)

func (value *Package) loadPictures(manifest []byte, files fs.FS) error {
	seen := make(map[string]*picture)
	encoded, decoded := len(manifest), uint64(0)
	for _, variant := range []*Variant{value.Light, value.Dark} {
		for _, asset := range variant.Assets {
			if picture, exists := seen[asset.Path]; exists {
				asset.pic = picture
				continue
			}
			if files == nil {
				return fmt.Errorf("%s: theme resource is unavailable", asset.Path)
			}
			prefix := "."
			for _, component := range append([]string{"."}, strings.Split(asset.Path, "/")...) {
				prefix = path.Join(prefix, component)
				info, err := fs.Lstat(files, prefix)
				if err != nil {
					return fmt.Errorf("%s: %w", asset.Path, err)
				}
				if info.Mode()&fs.ModeSymlink != 0 {
					return errors.New("theme resources cannot use symbolic links")
				}
				if prefix == asset.Path && (!info.Mode().IsRegular() || info.Size() > themeFileLimit) {
					return errors.New("theme resource exceeds file limit")
				}
			}
			// A snapshot keeps its resources' bytes, and its pictures share
			// them rather than hold a second copy.
			snapshot, _ := files.(memoryFS)
			data, kept := snapshot[asset.Path]
			if !kept {
				var err error
				if data, err = readThemeResource(files, asset.Path); err != nil {
					return err
				}
			}
			if len(data) > themeFileLimit || encoded+len(data) > themePackageLimit || len(seen) >= 32 {
				return errors.New("theme package exceeds resource limit")
			}
			configuration, format, err := image.DecodeConfig(bytes.NewReader(data))
			if err != nil {
				return fmt.Errorf("%s: %w", asset.Path, err)
			}
			ext := strings.ToLower(path.Ext(asset.Path))
			expected := map[string]string{".png": "png", ".jpg": "jpeg", ".jpeg": "jpeg", ".webp": "webp", ".gif": "gif"}[ext]
			if format != expected {
				return fmt.Errorf("%s: theme image format does not match its extension", asset.Path)
			}
			allocation := uint64(configuration.Width) * uint64(configuration.Height) * 4
			decoderBytes := allocation
			switch configuration.ColorModel.Convert(color.NRGBA64{}).(type) {
			case color.RGBA64, color.NRGBA64:
				decoderBytes = uint64(configuration.Width) * uint64(configuration.Height) * 8
			}
			if configuration.Width < 1 || configuration.Height < 1 || configuration.Width > 4096 || configuration.Height > 4096 || max(allocation, decoderBytes) > themeDecodeLimit-decoded {
				return errors.New("theme image exceeds decoded resource limit")
			}
			// Validate installed resources at admission without retaining their
			// pixels. The renderer decodes only the pictures it actually uses.
			if _, _, err := image.Decode(bytes.NewReader(data)); err != nil {
				return fmt.Errorf("%s: %w", asset.Path, err)
			}
			picture := lazyPicture(func() []byte { return data }, max(configuration.Width, configuration.Height) > themePreviewEdge)
			seen[asset.Path], asset.pic = picture, picture
			encoded += len(data)
			decoded += allocation
		}
	}
	return nil
}

func readThemeResource(files fs.FS, name string) ([]byte, error) {
	file, err := files.Open(name)
	if err != nil {
		return nil, fmt.Errorf("%s: %w", name, err)
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || info.Size() > themeFileLimit {
		return nil, errors.New("theme resource exceeds file limit")
	}
	return io.ReadAll(io.LimitReader(file, themeFileLimit+1))
}

func lazyPicture(read func() []byte, hasPreview bool) *picture {
	pic := &picture{bitmap: sync.OnceValue(func() *ui.Bitmap {
		data := read()
		if data == nil {
			return nil
		}
		bitmap, _ := ui.DecodeBitmap(data)
		return bitmap
	})}
	if hasPreview {
		pic.preview = sync.OnceValue(func() *ui.Bitmap {
			data := read()
			if data == nil {
				return nil
			}
			bitmap, _ := ui.DecodeBitmapScaled(data, themePreviewEdge)
			return bitmap
		})
	}
	return pic
}

// Installed pictures retain only the digest of the bytes validated for this
// package. A replacement at the same path belongs to a different snapshot.
// They read and decode off the GUI thread (diskBitmap).
func (value *Package) bindDiskPictures(dir string, files map[string][]byte) {
	seen := make(map[string]*picture)
	for _, variant := range []*Variant{value.Light, value.Dark} {
		for _, asset := range variant.Assets {
			pic := seen[asset.Path]
			if pic == nil {
				name := asset.Path
				expected := sha256.Sum256(files[name])
				// read is false when the file cannot be read now, and final
				// when its bytes belong to another revision.
				read := func() (body []byte, final bool) {
					body, err := readInstalledPicture(dir, name)
					if err != nil {
						return nil, false
					}
					if sha256.Sum256(body) != expected {
						return nil, true
					}
					return body, false
				}
				decode := func(scaled bool) *diskBitmap {
					return &diskBitmap{load: func() (*ui.Bitmap, bool) {
						data, final := read()
						if data == nil {
							return nil, final
						}
						var bitmap *ui.Bitmap
						var err error
						if scaled {
							bitmap, err = ui.DecodeBitmapScaled(data, themePreviewEdge)
						} else {
							bitmap, err = ui.DecodeBitmap(data)
						}
						return bitmap, err != nil
					}}
				}
				full := decode(false)
				pic = &picture{bitmap: full.get, loads: []*diskBitmap{full}}
				if asset.pic.preview != nil {
					preview := decode(true)
					pic.preview, pic.loads = preview.get, append(pic.loads, preview)
				}
				seen[name] = pic
			}
			asset.pic = pic
		}
	}
}

var (
	// pictureLoads bounds the installed pictures read and decoded at once.
	pictureLoads = make(chan struct{}, 2)
	watchers     struct {
		sync.Mutex
		next  uint64
		wakes map[uint64]func()
	}
)

// pictureRetry is the pause after a read failure before a later frame asks
// for the picture again.
const pictureRetry = time.Second

// readInstalledPicture reads an installed picture. It opens only a regular
// file, checked before the open: a FIFO put at its path would otherwise
// hold a loader, and the load budget, until something wrote to it.
func readInstalledPicture(dir, name string) ([]byte, error) {
	root, err := os.OpenRoot(dir)
	if err != nil {
		return nil, err
	}
	defer root.Close()
	return readThemeFile(root, name, themeFileLimit)
}

// WatchPictures calls wake, from a loading goroutine, whenever an installed
// theme picture finishes loading, so the window draws it. Release stops it.
func WatchPictures(wake func()) (release func()) {
	watchers.Lock()
	defer watchers.Unlock()
	if watchers.wakes == nil {
		watchers.wakes = map[uint64]func(){}
	}
	watchers.next++
	id := watchers.next
	watchers.wakes[id] = wake
	return func() {
		watchers.Lock()
		delete(watchers.wakes, id)
		watchers.Unlock()
	}
}

func picturesChanged() {
	watchers.Lock()
	wakes := make([]func(), 0, len(watchers.wakes))
	for _, wake := range watchers.wakes {
		wakes = append(wakes, wake)
	}
	watchers.Unlock()
	for _, wake := range wakes {
		wake()
	}
}

// diskBitmap is an installed picture's bitmap. The first frame that asks
// starts its load and draws without it. A read failure is asked again after
// pictureRetry; bytes of another revision or undecodable ones stay absent.
type diskBitmap struct {
	load func() (bitmap *ui.Bitmap, final bool)

	mu      sync.Mutex
	value   *ui.Bitmap
	final   bool
	retry   time.Time
	loading chan struct{}
}

func (d *diskBitmap) get() *ui.Bitmap {
	d.mu.Lock()
	defer d.mu.Unlock()
	if d.value != nil || d.final || d.loading != nil || time.Now().Before(d.retry) {
		return d.value
	}
	done := make(chan struct{})
	d.loading = done
	go func() {
		pictureLoads <- struct{}{}
		bitmap, final := d.load()
		<-pictureLoads
		d.mu.Lock()
		d.value, d.final, d.loading = bitmap, bitmap == nil && final, nil
		if bitmap == nil && !final {
			d.retry = time.Now().Add(pictureRetry)
		}
		d.mu.Unlock()
		close(done)
		// A failure changes nothing on screen; a later frame asks again.
		if bitmap != nil {
			picturesChanged()
		}
	}()
	return nil
}

// wait returns once the load get started, if any, has ended.
func (d *diskBitmap) wait() {
	d.mu.Lock()
	done := d.loading
	d.mu.Unlock()
	if done != nil {
		<-done
	}
}
