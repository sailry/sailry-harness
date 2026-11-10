//go:build !windows

package process

type outputDecoder struct{}

func newOutputDecoder(_, _ int, _ bool) outputDecoder { return outputDecoder{} }
func (*outputDecoder) append([]byte)                  {}
func (*outputDecoder) finish()                        {}
func (*outputDecoder) split(uint64) bool              { return false }
func (*outputDecoder) native() bool                   { return false }
func (*outputDecoder) transcoded() bool               { return false }
func (*outputDecoder) bounds(value []byte, offset uint64, complete bool) (int, int) {
	start, end := 0, len(value)
	if offset > 0 {
		start = utf8Start(value)
	}
	if !complete {
		end = utf8End(value)
	}
	return min(start, end), end
}
func (*outputDecoder) page(value []byte, offset uint64, limit int, complete bool) (int, error) {
	if offset > 0 && len(value) > 0 && value[0]&0xc0 == 0x80 {
		return 0, invalid("command output offset splits a UTF-8 character")
	}
	return pageEnd(value, limit, complete), nil
}
func (*outputDecoder) decode(value []byte, _ uint64) (string, bool) { return decodeOutput(value) }
