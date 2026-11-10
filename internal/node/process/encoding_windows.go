package process

import (
	"unicode/utf16"
	"unicode/utf8"
	"unsafe"

	"golang.org/x/sys/windows"
)

type codePage struct {
	number uint32
	leads  [12]byte
	single bool
}

var encodingKernel = windows.NewLazySystemDLL("kernel32.dll")
var getCPInfo = encodingKernel.NewProc("GetCPInfo")
var getOEMCP = encodingKernel.NewProc("GetOEMCP")

const mbErrInvalidChars = 0x00000008

func currentCodePage() *codePage {
	number, _ := windows.GetConsoleOutputCP()
	if number == 0 {
		result, _, _ := getOEMCP.Call()
		number = uint32(result)
	}
	return newCodePage(number)
}

func newCodePage(number uint32) *codePage {
	if number == 65001 {
		return nil
	}
	var info struct {
		maximum     uint32
		defaultChar [2]byte
		leads       [12]byte
		_           uint16
	}
	result, _, _ := getCPInfo.Call(uintptr(number), uintptr(unsafe.Pointer(&info)))
	if result == 0 || (info.maximum != 1 && info.maximum != 2) {
		return nil
	}
	return &codePage{number, info.leads, info.maximum == 1}
}

func (page *codePage) lead(value byte) bool {
	for index := 0; index < len(page.leads) && page.leads[index] != 0; index += 2 {
		if value >= page.leads[index] && value <= page.leads[index+1] {
			return true
		}
	}
	return false
}
func (page *codePage) decode(value []byte) (string, bool) {
	if len(value) == 0 {
		return "", true
	}
	length, err := windows.MultiByteToWideChar(page.number, mbErrInvalidChars, &value[0], int32(len(value)), nil, 0)
	if err != nil || length <= 0 {
		return "", false
	}
	wide := make([]uint16, length)
	written, err := windows.MultiByteToWideChar(page.number, mbErrInvalidChars, &value[0], int32(len(value)), &wide[0], length)
	if err != nil || written != length {
		return "", false
	}
	for index, value := range wide {
		if value >= 0xd800 && value <= 0xdbff {
			if index+1 >= len(wide) || wide[index+1] < 0xdc00 || wide[index+1] > 0xdfff {
				return "", false
			}
		} else if value >= 0xdc00 && value <= 0xdfff && (index == 0 || wide[index-1] < 0xd800 || wide[index-1] > 0xdbff) {
			return "", false
		}
	}
	return string(utf16.Decode(wide)), true
}

type outputDecoder struct {
	pageCode        *codePage
	command         bool
	mode            byte // 0 detect, 1 UTF-8, 2 legacy, 3 UTF-16
	bom, big        bool
	probe           []byte
	total           uint64
	lead            bool
	unit            *byte
	surrogate, utf8 bool
	prefix, tail    int
	trails          []byte
}

func newOutputDecoder(prefix, tail int, command bool) outputDecoder {
	decoder := outputDecoder{prefix: prefix, tail: tail, command: command}
	if command {
		decoder.pageCode = currentCodePage()
	}
	return decoder
}
func (decoder *outputDecoder) slot(offset uint64) (uint64, bool) {
	if offset <= uint64(decoder.prefix) {
		return offset, true
	}
	if decoder.tail > 0 {
		return uint64(decoder.prefix) + 1 + (offset-uint64(decoder.prefix)-1)%uint64(decoder.tail), true
	}
	return 0, false
}
func (decoder *outputDecoder) record(offset uint64, trail bool) {
	slot, ok := decoder.slot(offset)
	if !ok {
		return
	}
	if len(decoder.trails) == 0 {
		if !trail {
			return
		}
		decoder.trails = make([]byte, (decoder.prefix+decoder.tail+8)/8)
	}
	bit := byte(1 << (slot % 8))
	if trail {
		decoder.trails[slot/8] |= bit
	} else {
		decoder.trails[slot/8] &^= bit
	}
}
func (decoder *outputDecoder) append(value []byte) {
	for _, value := range value {
		offset := decoder.total
		decoder.total++
		if !decoder.command {
			continue
		}
		if decoder.mode == 3 {
			if decoder.unit != nil {
				unit := uint16(*decoder.unit) | uint16(value)<<8
				if decoder.big {
					unit = uint16(*decoder.unit)<<8 | uint16(value)
				}
				decoder.record(offset-1, decoder.surrogate && unit >= 0xdc00 && unit <= 0xdfff)
				decoder.record(offset, true)
				decoder.surrogate = unit >= 0xd800 && unit <= 0xdbff
				decoder.unit = nil
			} else {
				v := value
				decoder.unit = &v
				decoder.record(offset, decoder.surrogate)
			}
			continue
		}
		if !decoder.utf8 && decoder.pageCode != nil {
			decoder.record(offset, decoder.lead)
			decoder.lead = !decoder.lead && decoder.pageCode.lead(value)
		}
		if decoder.mode == 0 {
			decoder.probe = append(decoder.probe, value)
			p := decoder.probe
			switch {
			case len(p) == 2 && (p[0] == 0xff && p[1] == 0xfe || p[0] == 0xfe && p[1] == 0xff):
				decoder.mode = 3
				decoder.big = value == 0xff
				decoder.record(0, false)
				decoder.record(1, true)
				decoder.probe = nil
			case len(p) == 3 && p[0] == 0xef && p[1] == 0xbb && p[2] == 0xbf:
				decoder.mode = 1
				decoder.bom = true
				decoder.utf8 = true
				decoder.trails = nil
				decoder.probe = nil
			case len(p) == 1 && (p[0] == 0xef || p[0] == 0xff || p[0] == 0xfe) || len(p) == 2 && p[0] == 0xef && p[1] == 0xbb:
			default:
				decoder.mode = 1
				decoder.checkUTF8()
			}
		} else if decoder.mode == 1 && !decoder.bom && !decoder.utf8 && decoder.pageCode != nil && (value >= 128 || len(decoder.probe) > 0) {
			decoder.probe = append(decoder.probe, value)
			decoder.checkUTF8()
		}
	}
}
func (decoder *outputDecoder) checkUTF8() {
	if utf8.Valid(decoder.probe) {
		for _, value := range decoder.probe {
			if value >= 128 {
				decoder.utf8 = true
				decoder.trails = nil
				break
			}
		}
		decoder.probe = nil
		return
	}
	value := decoder.probe
	for len(value) > 0 {
		if !utf8.FullRune(value) {
			return
		}
		r, size := utf8.DecodeRune(value)
		if r == utf8.RuneError && size == 1 {
			if decoder.pageCode != nil {
				decoder.mode = 2
			}
			decoder.probe = nil
			return
		}
		value = value[size:]
	}
}
func (decoder *outputDecoder) finish() {
	if !decoder.utf8 && (decoder.mode == 0 || decoder.mode == 1 && !decoder.bom) && len(decoder.probe) > 0 && decoder.pageCode != nil && decoder.pageCode.single {
		if _, ok := decoder.pageCode.decode(decoder.probe); ok {
			decoder.mode = 2
			decoder.probe = nil
		}
	}
}
func (decoder *outputDecoder) native() bool {
	return decoder.command && (decoder.mode == 2 || decoder.mode == 3)
}
func (decoder *outputDecoder) transcoded() bool {
	return decoder.native() || decoder.command && decoder.bom
}
func (decoder *outputDecoder) pending() int {
	if decoder.mode == 2 {
		if decoder.lead {
			return 1
		}
	}
	if decoder.mode == 3 {
		count := 0
		if decoder.unit != nil {
			count++
		}
		if decoder.surrogate {
			count += 2
		}
		return count
	}
	return 0
}
func (decoder *outputDecoder) split(offset uint64) bool {
	if !decoder.native() || offset > decoder.total {
		return false
	}
	if offset == decoder.total {
		return decoder.pending() > 0
	}
	slot, ok := decoder.slot(offset)
	return ok && int(slot/8) < len(decoder.trails) && decoder.trails[slot/8]&(1<<(slot%8)) != 0
}
func (decoder *outputDecoder) bounds(value []byte, offset uint64, complete bool) (int, int) {
	if decoder.command && decoder.mode == 0 && !complete {
		return 0, 0
	}
	start, end := 0, len(value)
	if !decoder.native() {
		if offset > 0 {
			start = utf8Start(value)
		}
		if !complete {
			end = utf8End(value)
		}
		return min(start, end), end
	}
	for start < len(value) && decoder.split(offset+uint64(start)) {
		start++
	}
	if !complete {
		if offset+uint64(end) == decoder.total {
			end = max(0, end-decoder.pending())
		} else {
			for end > start && decoder.split(offset+uint64(end)) {
				end--
			}
		}
	}
	return min(start, end), end
}
func (decoder *outputDecoder) page(value []byte, offset uint64, limit int, complete bool) (int, error) {
	if decoder.command && decoder.mode == 0 && !complete {
		return 0, nil
	}
	if !decoder.native() {
		if offset > 0 && len(value) > 0 && value[0]&0xc0 == 0x80 {
			return 0, invalid("command output offset splits a UTF-8 character")
		}
		return pageEnd(value, limit, complete), nil
	}
	if decoder.split(offset) && offset < decoder.total {
		return 0, invalid("command output offset splits an encoded character")
	}
	available := len(value)
	if !complete && offset+uint64(available) == decoder.total {
		available = max(0, available-decoder.pending())
	}
	end := min(available, limit)
	for end > 0 && end < available && decoder.split(offset+uint64(end)) {
		end--
	}
	if end == 0 && available > 0 {
		end = 1
		for end < available && decoder.split(offset+uint64(end)) {
			end++
		}
	}
	return end, nil
}
func (decoder *outputDecoder) decode(value []byte, offset uint64) (string, bool) {
	if decoder.native() {
		if decoder.mode == 2 {
			if text, ok := decoder.pageCode.decode(value); ok {
				return text, false
			}
			text, _ := decodeOutput(value)
			return text, true
		}
		if offset == 0 && len(value) >= 2 {
			value = value[2:]
		}
		wide := make([]uint16, 0, len(value)/2)
		for index := 0; index+1 < len(value); index += 2 {
			unit := uint16(value[index]) | uint16(value[index+1])<<8
			if decoder.big {
				unit = uint16(value[index])<<8 | uint16(value[index+1])
			}
			wide = append(wide, unit)
		}
		bad := len(value)%2 != 0
		for index, unit := range wide {
			if unit >= 0xd800 && unit <= 0xdbff {
				bad = bad || index+1 >= len(wide) || wide[index+1] < 0xdc00 || wide[index+1] > 0xdfff
			} else if unit >= 0xdc00 && unit <= 0xdfff {
				bad = bad || index == 0 || wide[index-1] < 0xd800 || wide[index-1] > 0xdbff
			}
		}
		text := string(utf16.Decode(wide))
		if len(value)%2 != 0 {
			text += "\ufffd"
		}
		return text, bad
	}
	if decoder.command && decoder.bom && offset == 0 && len(value) >= 3 {
		value = value[3:]
	}
	return decodeOutput(value)
}
