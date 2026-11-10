package process

type nativePasswd struct {
	name, password            *byte
	uid, gid                  uint32
	change                    int64
	class, gecos, home, shell *byte
	expire                    int64
}
