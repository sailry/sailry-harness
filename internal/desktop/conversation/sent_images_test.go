package conversation_test

import (
	"image"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

const sentWidth, sentHeight = 1024, 760

func sentBitmap(w, h int) *ui.Bitmap { return ui.NewBitmap(image.NewRGBA(image.Rect(0, 0, w, h))) }

// sentTester draws the user turns *turns holds through the transcript, as the
// live window does, with the resolvers sess gives.
func sentTester(turns *[]conversation.Turn, sess *conversation.Session) *ui.Tester {
	s := conversation.New()
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() { conversation.View(c, s, "session", *turns, sess, noActions{}) })
	}, sentWidth, sentHeight)
	tt.SetLocale("en")
	tt.Frame()
	tt.Frame()
	return tt
}

func sentTile(t *testing.T, tt *ui.Tester, name string) ui.Rect {
	t.Helper()
	tile, ok := tt.Access(name)
	if !ok {
		t.Fatalf("no picture %q: %q", name, tt.Texts())
	}
	return tile.Bounds
}

func sentSize(t *testing.T, tt *ui.Tester, name string, w, h float32) ui.Rect {
	t.Helper()
	b := sentTile(t, tt, name)
	if d := max(b.W-w, w-b.W, b.H-h, h-b.H); d > 0.5 {
		t.Fatalf("picture %q is %vx%v, want %vx%v", name, b.W, b.H, w, h)
	}
	return b
}

// sentPixels is what the window shows inside b.
func sentPixels(tt *ui.Tester, b ui.Rect) []byte {
	img := tt.Image()
	scale := float32(img.Bounds().Dx()) / sentWidth
	area := image.Rect(int(b.X*scale), int(b.Y*scale), int((b.X+b.W)*scale), int((b.Y+b.H)*scale)).Intersect(img.Bounds())
	var pixels []byte
	for y := area.Min.Y; y < area.Max.Y; y++ {
		start := img.PixOffset(area.Min.X, y)
		pixels = append(pixels, img.Pix[start:start+area.Dx()*4]...)
	}
	return pixels
}

// sentSession resolves layout facts as the live resolver does and pixels only for a
// tile in view: source returns them, nil while they load.
func sentSession(facts map[string]conversation.Image, source func(conversation.Image) ui.ImageSource) *conversation.Session {
	return &conversation.Session{ResolveImage: func(_ string, im conversation.Image) conversation.Image {
		if im.Source != nil {
			return im
		}
		if known, ok := facts[im.ID]; ok {
			im.Width, im.Height, im.OriginalRevision = known.Width, known.Height, known.OriginalRevision
		}
		if !im.ThumbnailFactsOnly() && source != nil {
			im.Source = source(im)
		}
		return im
	}}
}

// The user's picture of unknown size waits in a still 48-DIP placeholder,
// where a reply's picture of unknown size keeps the full tile.
func TestSentPictureOfUnknownSize(t *testing.T) {
	turns := []conversation.Turn{
		{ID: "user", User: "Look", Sent: time.Unix(1, 0), Images: []conversation.Image{{ID: "sent", Name: "sent.png"}}},
	}
	tt := sentTester(&turns, sentSession(nil, nil))
	b := sentSize(t, tt, "sent.png", 48, 48)
	if b.X+b.W < sentWidth/2 {
		t.Fatalf("the user's picture at %+v is not on the user's side", b)
	}
	before := sentPixels(tt, b)
	for range 4 {
		tt.Advance(170 * time.Millisecond)
		if after := sentPixels(tt, sentTile(t, tt, "sent.png")); !slices.Equal(before, after) {
			t.Fatal("the user's placeholder animates")
		}
	}

	replies := []conversation.Turn{{ID: "reply", Reply: conversation.Reply{Blocks: []conversation.Block{
		{Kind: conversation.ImagesBlock, Images: []conversation.Image{{ID: "reply", Name: "reply.png"}}},
	}}}}
	sentSize(t, sentTester(&replies, sentSession(nil, nil)), "reply.png", 240, 240)
}

// A small picture shows at its own size, never enlarged to the tile, whether
// its size is a known fact or only its decoded source's.
func TestSentSmallPictureNotEnlarged(t *testing.T) {
	turns := []conversation.Turn{{ID: "user", User: "Small", Images: []conversation.Image{
		{ID: "known", Name: "known.png", Width: 32, Height: 24, Source: sentBitmap(32, 24)},
	}}}
	tt := sentTester(&turns, sentSession(nil, nil))
	sentSize(t, tt, "known.png", 32, 24)

	turns = []conversation.Turn{{ID: "user", User: "Small", Images: []conversation.Image{
		{ID: "decoded", Name: "decoded.png", Source: sentBitmap(40, 30)},
	}}}
	tt.Frame()
	tt.Frame()
	sentSize(t, tt, "decoded.png", 40, 30)

	turns = []conversation.Turn{{ID: "user", User: "Large", Images: []conversation.Image{
		{ID: "large", Name: "large.png", Width: 1600, Height: 1200, Source: sentBitmap(1600, 1200)},
	}}}
	tt.Frame()
	tt.Frame()
	sentSize(t, tt, "large.png", 240, 180)
}

// The sent preview becomes the user entry in place: the entry's facts keep
// the tile's size while its thumbnail loads, with no placeholder in between,
// and the entry's own thumbnail replaces the preview's pixels.
func TestSentPictureHandoff(t *testing.T) {
	preview := sentBitmap(400, 300)
	turns := []conversation.Turn{{ID: "outgoing/session", User: "Look", InputOnly: true, Images: []conversation.Image{
		{ID: "picture", Name: "picture.png", Width: 400, Height: 300, Source: preview, OriginalRevision: "original"},
	}}}
	facts := map[string]conversation.Image{"picture": {Width: 400, Height: 300, OriginalRevision: "original"}}
	var thumbnail ui.ImageSource
	requested := 0
	tt := sentTester(&turns, sentSession(facts, func(conversation.Image) ui.ImageSource { requested++; return thumbnail }))
	sentSize(t, tt, "picture.png", 240, 180)
	if requested != 0 {
		t.Fatal("a preview with its pixels requested the entry's thumbnail")
	}

	turns = []conversation.Turn{{ID: "turn", User: "Look", Images: []conversation.Image{{ID: "picture", Name: "picture.png"}}}}
	// Each frame of the handoff keeps the preview's size: no 48-DIP gap.
	tt.Frame()
	sentSize(t, tt, "picture.png", 240, 180)
	tt.Frame()
	sentSize(t, tt, "picture.png", 240, 180)
	if requested == 0 {
		t.Fatal("the user entry did not request its thumbnail in view")
	}
	thumbnail = sentBitmap(480, 360)
	tt.Frame()
	tt.Frame()
	sentSize(t, tt, "picture.png", 240, 180)
	if _, ok := tt.AccessByRole(ui.RoleButton, "picture.png"); !ok {
		t.Fatal("the handed-off picture does not open")
	}
}

// A sent picture opens the viewer on its original: the viewer resolves the
// entry's original revision, even while the thumbnail's pixels are gone.
func TestSentPictureOpensOriginal(t *testing.T) {
	turns := []conversation.Turn{{ID: "turn", User: "Look", Images: []conversation.Image{{ID: "picture", Name: "picture.png"}}}}
	sess := sentSession(map[string]conversation.Image{"picture": {Width: 400, Height: 300, OriginalRevision: "original"}}, nil)
	var opened []conversation.Image
	original := sentBitmap(1600, 1200)
	sess.ResolveViewerImage = func(_ string, im conversation.Image) ui.ImageSource {
		opened = append(opened, im)
		return original
	}
	tt := sentTester(&turns, sess)
	sentSize(t, tt, "picture.png", 240, 180)
	if err := tt.Click("picture.png"); err != nil {
		t.Fatal(err)
	}
	tt.Advance(time.Second)
	if len(opened) == 0 {
		t.Fatal("opening the picture did not resolve its original")
	}
	if last := opened[len(opened)-1]; last.ID != "picture" || last.OriginalRevision != "original" {
		t.Fatalf("the viewer resolved %+v, want the entry's original", last)
	}
	if !tt.HasText("100%") {
		t.Fatalf("the viewer did not open: %q", tt.Texts())
	}
	tt.Key(0, ui.KeyEscape)
	tt.Advance(time.Second)
	if tt.HasText("100%") {
		t.Fatal("Escape left the viewer open")
	}
}
