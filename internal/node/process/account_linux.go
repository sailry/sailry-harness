package process

type nativePasswd struct {
	name, password     *byte
	uid, gid           uint32
	gecos, home, shell *byte
}
