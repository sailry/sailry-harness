package theme

import (
	"bytes"
	"errors"
	"fmt"
	"image"
	"image/color"
	"io"
	"io/fs"
	"path"
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
)

const (
	themeManifestLimit = 256 * 1024
	themeFileLimit     = 8 * 1024 * 1024
	themePackageLimit  = 32 * 1024 * 1024
	themeDecodeLimit   = 32 * 1024 * 1024
	themePreviewEdge   = 768
)

func (value *Package) loadPictures(manifest []byte, files fs.FS, embedded bool) error {
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
			if !embedded {
				if _, _, err := image.Decode(bytes.NewReader(data)); err != nil {
					return fmt.Errorf("%s: %w", asset.Path, err)
				}
			}
			size := len(data)
			source := func() []byte { return data }
			if embedded {
				// The built-in themes cannot change: a picture reads its
				// bytes again when it is first drawn instead of keeping them.
				name := asset.Path
				data = nil
				source = func() []byte {
					body, _ := readThemeResource(files, name)
					return body
				}
			}
			picture := &picture{bitmap: sync.OnceValue(func() *ui.Bitmap {
				bitmap, _ := ui.DecodeBitmap(source())
				return bitmap
			})}
			if max(configuration.Width, configuration.Height) > themePreviewEdge {
				picture.preview = sync.OnceValue(func() *ui.Bitmap {
					bitmap, _ := ui.DecodeBitmapScaled(source(), themePreviewEdge)
					return bitmap
				})
			}
			seen[asset.Path], asset.pic = picture, picture
			encoded += size
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
