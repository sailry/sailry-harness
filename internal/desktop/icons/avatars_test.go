package icons

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// An avatar's seed always draws the same picture, a random one included;
// an icon that names no seed draws none.
func TestAvatar(t *testing.T) {
	for range 8 {
		avatar := RandomAvatar()
		if _, ok := protocol.RoleAvatarSeed(avatar); !ok || Avatar(avatar) == nil {
			t.Fatalf("random avatar %q: no picture", avatar)
		}
	}
	fox := Avatar("avatar-fox").Image(64, 64, nil)
	avatars.Lock()
	avatars.pictures = nil
	avatars.Unlock()
	again := Avatar("avatar-fox").Image(64, 64, nil)
	if string(fox.Pix) != string(again.Pix) {
		t.Error("avatar-fox draws differently the second time")
	}
	if string(fox.Pix) == string(Avatar("avatar-owl").Image(64, 64, nil).Pix) {
		t.Error("two seeds draw the same picture")
	}
	for _, icon := range []string{"avatar-", "avatar-Fox", "fox", "code", ""} {
		if Avatar(icon) != nil {
			t.Errorf("%q has a picture", icon)
		}
	}
}
