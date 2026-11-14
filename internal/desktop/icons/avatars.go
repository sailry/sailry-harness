package icons

import (
	_ "embed"
	"math/rand/v2"
	"sync"

	dicebear "github.com/dicebear/dicebear-go/v10"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Role avatars are DiceBear's Voxel Bot drawn from a seed: a robot on a
// plain color that fills the square. The same seed always draws the same
// robot, here and in DiceBear's other ports. See avatars/SOURCE.md.

//go:embed avatars/voxel-bot.json
var voxelBotDefinition []byte

// voxelBot is the parsed style, read once on the first picture.
var voxelBot = sync.OnceValue(func() *dicebear.Style {
	style, err := dicebear.NewStyle(voxelBotDefinition)
	if err != nil {
		panic("icons: voxel-bot.json: " + err.Error())
	}
	return style
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
	pictures map[string]*ui.SVG
}

// Avatar is the picture of a role avatar's identifier, nil for any other
// icon.
func Avatar(id string) *ui.SVG {
	avatars.Lock()
	picture, cached := avatars.pictures[id]
	avatars.Unlock()
	if cached {
		return picture
	}
	if seed, ok := protocol.RoleAvatarSeed(id); ok {
		avatar, err := dicebear.NewAvatar(voxelBot(), map[string]any{"seed": seed})
		if err == nil {
			picture, _ = ui.ParseSVG([]byte(avatar.SVG()))
		}
	}
	avatars.Lock()
	if len(avatars.pictures) >= avatarCacheLimit || avatars.pictures == nil {
		avatars.pictures = make(map[string]*ui.SVG, avatarCacheLimit)
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
