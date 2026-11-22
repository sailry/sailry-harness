//go:build sailry_no_code_index

package codeindex

import (
	"context"

	"github.com/sailry/sailry-harness/internal/node/codeindexapi"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type Service struct{}

func NewService(string, Watch) codeindexapi.Service { return nil }

func New(string, Watch) *Service { return &Service{} }

func (*Service) Configure(bool) {}

func (*Service) Close() {}

func (*Service) Changed(protocol.WorktreeID, *string) {}

func (*Service) Observed(protocol.WorktreeID, []string) {}

func (*Service) Remove(protocol.WorktreeID) error { return nil }

func (*Service) Search(context.Context, protocol.WorktreeID, string, protocol.CodeIndexSearch) (protocol.CodeIndexSearchPage, error) {
	return protocol.CodeIndexSearchPage{}, unavailable()
}

func (*Service) SearchOnDemand(context.Context, protocol.WorktreeID, string, protocol.CodeIndexSearch) (protocol.CodeIndexSearchPage, error) {
	return protocol.CodeIndexSearchPage{}, unavailable()
}

func (*Service) OutlineOnDemand(context.Context, protocol.WorktreeID, string, string) (protocol.CodeIndexOutline, error) {
	return protocol.CodeIndexOutline{}, unavailable()
}

func (*Service) Graph(context.Context, protocol.WorktreeID, string, protocol.CodeIndexGraph) (protocol.CodeIndexGraphGraphPage, error) {
	return protocol.CodeIndexGraphGraphPage{}, unavailable()
}

func (*Service) GraphOnDemand(context.Context, protocol.WorktreeID, string, protocol.CodeIndexGraph) (protocol.CodeIndexGraphGraphPage, error) {
	return protocol.CodeIndexGraphGraphPage{}, unavailable()
}

type Parser struct{}

func (*Parser) Parse(context.Context, string, string) (Parsed, error) {
	return Parsed{}, unavailable()
}

func (*Parser) ParseIndexed(context.Context, string, string) (Parsed, error) {
	return Parsed{}, unavailable()
}

func (*Parser) ParseOutline(context.Context, string, string) (Parsed, error) {
	return Parsed{}, unavailable()
}

func Candidate(string) bool { return false }

func IndexedSource(string) bool { return false }

func Omitted(file string, status Status) Parsed { return omitted(file, status) }

func unavailable() *protocol.Fault {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "code index is disabled in this build"}
}
