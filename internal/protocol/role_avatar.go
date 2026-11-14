// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

// RoleAvatarPrefix starts a role's or a team's icon that names a generated
// avatar: the prefix and the seed the Desktop draws the picture from,
// "avatar-k3f9x2a1".
// Icons stored before the pictures were generated, such as "avatar-fox",
// are seeds as well.
const RoleAvatarPrefix = "avatar-"

// RoleAvatarSeed is the seed of a role icon that names a generated avatar:
// 1 to 32 lowercase ASCII letters and digits after RoleAvatarPrefix.
func RoleAvatarSeed(icon string) (string, bool) {
	if len(icon) <= len(RoleAvatarPrefix) || len(icon) > len(RoleAvatarPrefix)+32 || icon[:len(RoleAvatarPrefix)] != RoleAvatarPrefix {
		return "", false
	}
	seed := icon[len(RoleAvatarPrefix):]
	for _, c := range []byte(seed) {
		if (c < 'a' || c > 'z') && (c < '0' || c > '9') {
			return "", false
		}
	}
	return seed, true
}
