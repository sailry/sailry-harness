package process

import (
	"net/url"
	"slices"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Service announcements are candidates from complete output lines, not port scans.
type serviceScanner struct {
	escape         byte
	scheme         string
	matched        int
	candidate      []byte
	oversized      bool
	pending, found []protocol.ProcessService
	unicode        []byte
}

func (scanner *serviceScanner) append(value []byte) {
	for _, value := range value {
		if len(scanner.found) >= 16 {
			return
		}
		switch {
		case scanner.escape == 0 && value == 0x1b:
			scanner.escape = 1
		case scanner.escape == 1 && value == '[':
			scanner.escape = 2
		case scanner.escape == 2 && value >= 0x40 && value <= 0x7e:
			scanner.escape = 0
		case scanner.escape == 2 && value >= 0x20 && value <= 0x3f:
		case scanner.escape == 0:
			scanner.text(value)
		default:
			scanner.escape = 0
			scanner.text(0x1b)
			scanner.text(value)
		}
	}
}

func (scanner *serviceScanner) text(value byte) {
	if value < 128 {
		for _, value := range scanner.unicode {
			scanner.plain(value)
		}
		scanner.unicode = nil
		scanner.plain(value)
		return
	}
	scanner.unicode = append(scanner.unicode, value)
	if !utf8.FullRune(scanner.unicode) {
		return
	}
	r, size := utf8.DecodeRune(scanner.unicode)
	if size == len(scanner.unicode) && unicode.IsSpace(r) {
		scanner.plain(' ')
	} else {
		for _, value := range scanner.unicode {
			scanner.plain(value)
		}
	}
	scanner.unicode = nil
}

func (scanner *serviceScanner) plain(value byte) {
	if scanner.candidate == nil {
		scanner.matchScheme(value)
	} else if value <= 32 || value == 127 || strings.ContainsRune("<>\"'`", rune(value)) {
		if !scanner.oversized && len(scanner.found)+len(scanner.pending) < 16 {
			if service, ok := parseService(string(scanner.candidate)); ok && !slices.ContainsFunc(scanner.found, func(found protocol.ProcessService) bool { return found.Port == service.Port }) && !slices.ContainsFunc(scanner.pending, func(found protocol.ProcessService) bool { return found.Port == service.Port }) {
				scanner.pending = append(scanner.pending, service)
			}
		}
		scanner.candidate, scanner.oversized = nil, false
	} else if len(scanner.candidate) < 2048 {
		scanner.candidate = append(scanner.candidate, value)
	} else {
		scanner.oversized = true
	}
	if value == '\n' || value == '\r' {
		scanner.found = append(scanner.found, scanner.pending...)
		scanner.pending = nil
	}
}

func (scanner *serviceScanner) matchScheme(value byte) {
	if value == 'h' {
		scanner.scheme, scanner.matched = "http://", 1
		return
	}
	if scanner.matched == 0 {
		return
	}
	if scanner.matched == len("http") && value == 's' {
		scanner.scheme = "https://"
	}
	if value != scanner.scheme[scanner.matched] {
		scanner.matched = 0
		return
	}
	scanner.matched++
	if scanner.matched == len(scanner.scheme) {
		scanner.candidate = []byte(scanner.scheme)
		scanner.matched = 0
	}
}

func parseService(candidate string) (protocol.ProcessService, bool) {
	value, err := url.Parse(strings.TrimRight(candidate, ")],.;"))
	if err != nil || value.User != nil || (value.Scheme != "http" && value.Scheme != "https") {
		return protocol.ProcessService{}, false
	}
	if host := value.Hostname(); host != "localhost" && host != "127.0.0.1" && host != "0.0.0.0" {
		return protocol.ProcessService{}, false
	}
	port := value.Port()
	if port == "" {
		if value.Scheme == "https" {
			port = "443"
		} else {
			port = "80"
		}
	}
	number, err := strconv.ParseUint(port, 10, 16)
	if err != nil || number == 0 {
		return protocol.ProcessService{}, false
	}
	value.Host = "127.0.0.1"
	if value.Scheme == "http" && number != 80 || value.Scheme == "https" && number != 443 {
		value.Host += ":" + port
	}
	if value.Path == "" {
		value.Path = "/"
	}
	return protocol.ProcessService{Port: uint16(number), Url: value.String()}, true
}
