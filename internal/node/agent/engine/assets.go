package engine

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
)

func (r *Runtime) modelAssetsWith(ctx context.Context, records []canonical.Record, assets map[string][]byte) (map[string][]byte, error) {
	err := r.db.Read(ctx, func(db *sql.Conn) error {
		for i := range records {
			if message := records[i].MutableMessage(); message != nil {
				for _, block := range message.Content {
					if value, ok := block.Data.(*canonical.AssetContent); ok && value.Asset.Digest != "" {
						if _, exists := assets[value.Asset.Digest]; !exists {
							data, err := store.GetBlob(ctx, db, value.Asset.Digest)
							if err != nil {
								return err
							}
							assets[value.Asset.Digest] = data
						}
					}
				}
			}
		}
		return nil
	})
	return assets, err
}
