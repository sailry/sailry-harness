package store

import (
	"context"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"io"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func assemblePersonalKVAcknowledgement(ctx context.Context, db *sql.Conn, user protocol.UserID, page protocol.ConfigurationSnapshotPage) (protocol.PersonalKVReceipt, error) {
	var receipt protocol.PersonalKVReceipt
	reader := &configurationSnapshotReader{ctx: ctx, db: db, user: user, root: page.Root, count: configurationPageCount(page.Bytes)}
	digest := blake3.New(32, nil)
	decoder := json.NewDecoder(io.TeeReader(reader, digest))
	if err := decoder.Decode(&receipt); err != nil {
		return receipt, err
	}
	var extra any
	if err := decoder.Decode(&extra); err != io.EOF {
		return receipt, invalid("personal data acknowledgement contains trailing data")
	}
	if receipt.Sequence != page.Revision || hex.EncodeToString(digest.Sum(nil)) != page.Digest {
		return receipt, invalid("personal data acknowledgement differs from its delivery")
	}
	return receipt, nil
}
