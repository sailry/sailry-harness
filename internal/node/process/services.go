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
	prefix         []byte
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
	if value <= 32 || value == 127 || strings.ContainsRune("<>\"'`", rune(value)) {
		if scanner.candidate != nil && !scanner.oversized && len(scanner.found)+len(scanner.pending) < 16 {
			if service, ok := parseService(string(scanner.candidate)); ok && !slices.ContainsFunc(scanner.found, func(found protocol.ProcessService) bool { return found.Port == service.Port }) && !slices.ContainsFunc(scanner.pending, func(found protocol.ProcessService) bool { return found.Port == service.Port }) {
				scanner.pending = append(scanner.pending, service)
			}
		}
		scanner.candidate, scanner.prefix, scanner.oversized = nil, nil, false
		if value == '\n' || value == '\r' {
			scanner.found = append(scanner.found, scanner.pending...)
			scanner.pending = nil
		}
		return
	}
	if scanner.candidate != nil {
		if len(scanner.candidate) < 2048 {
			scanner.candidate = append(scanner.candidate, value)
		} else {
			scanner.oversized = true
		}
		return
	}
	scanner.prefix = append(scanner.prefix, value)
	for !strings.HasPrefix("http://", string(scanner.prefix)) && !strings.HasPrefix("https://", string(scanner.prefix)) {
		scanner.prefix = scanner.prefix[1:]
	}
	if string(scanner.prefix) == "http://" || string(scanner.prefix) == "https://" {
		scanner.candidate, scanner.prefix = scanner.prefix, nil
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
