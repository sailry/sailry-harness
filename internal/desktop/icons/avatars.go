package icons

import (
	_ "embed"
	"math/rand/v2"
	"sync"

	dicebear "github.com/dicebear/dicebear-go/v10"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Role avatars are DiceBear's Gaze drawn from a seed: a pair of eyes on a
// colored body. The same seed always draws the same face, here and in
// DiceBear's other ports. See avatars/SOURCE.md.

//go:embed avatars/gaze.json
var gazeDefinition []byte

// gaze is the parsed style, read once on the first picture.
var gaze = sync.OnceValue(func() *dicebear.Style {
	style, err := dicebear.NewStyle(gazeDefinition)
	if err != nil {
		panic("icons: gaze.json: " + err.Error())
	}
	return style
})

// faceScale leaves the body room inside its tile, which Kit fills.
const faceScale = 0.82

// Picture is a drawn avatar: the face, transparent around its body, and
// the body's color, which tints the tile under it.
type Picture struct {
	Face *ui.SVG
	Body ui.Color
}

// face draws the Gaze face of options, nil when they are invalid.
func face(options map[string]any) *Picture {
	options["scale"] = faceScale
	avatar, err := dicebear.NewAvatar(gaze(), options)
	if err != nil {
		return nil
	}
	svg, err := ui.ParseSVG([]byte(avatar.SVG()))
	if err != nil {
		return nil
	}
	picture := &Picture{Face: svg}
	if body, ok := avatar.ResolvedOptions()["bodyColor"].([]string); ok && len(body) > 0 {
		picture.Body = ui.Hex(body[0])
	}
	return picture
}

// DefaultFace is the face of a role without an avatar: a white round face
// that Kit shows on the role's color, so roles differ by color alone.
var DefaultFace = sync.OnceValue(func() *ui.SVG {
	return face(map[string]any{"seed": "sailry", "bodyColor": []string{"#ffffff"}, "shapeVariant": "circle", "eyesVariant": "dots", "spacingVariant": "normal"}).Face
})

// LeadFace is the face of a team's lead: a white smiling pentagon that Kit
// shows on the primary color.
var LeadFace = sync.OnceValue(func() *ui.SVG {
	return face(map[string]any{"seed": "lead", "bodyColor": []string{"#ffffff"}, "shapeVariant": "pentagon", "eyesVariant": "happy", "spacingVariant": "normal"}).Face
})

// AvatarPrefix starts the identifier of a role avatar (protocol.RoleAvatarSeed).
const AvatarPrefix = protocol.RoleAvatarPrefix

// avatarCacheLimit bounds the pictures kept: the role lists' and a few
// picker pages', so shuffling does not grow it without end.
const avatarCacheLimit = 256

// avatars are the pictures drawn so far by identifier, nil for an
// identifier that names none, so each is generated once.
var avatars struct {
	sync.Mutex
	pictures map[string]*Picture
}

// Avatar is the picture of a role avatar's identifier, nil for any other
// icon.
func Avatar(id string) *Picture {
	avatars.Lock()
	picture, cached := avatars.pictures[id]
	avatars.Unlock()
	if cached {
		return picture
	}
	if seed, ok := protocol.RoleAvatarSeed(id); ok {
		picture = face(map[string]any{"seed": seed})
	}
	avatars.Lock()
	if len(avatars.pictures) >= avatarCacheLimit || avatars.pictures == nil {
		avatars.pictures = make(map[string]*Picture, avatarCacheLimit)
	}
	avatars.pictures[id] = picture
	avatars.Unlock()
	return picture
}

// RandomAvatar is the identifier of a new random avatar: AvatarPrefix and
// a seed of 8 letters and digits.
func RandomAvatar() string {
	const alphabet = "abcdefghijklmnopqrstuvwxyz0123456789"
	seed := make([]byte, 8)
	for i := range seed {
		seed[i] = alphabet[rand.IntN(len(alphabet))]
	}
	return AvatarPrefix + string(seed)
}
