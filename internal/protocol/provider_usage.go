package protocol

// CommandReadProviderAccountUsagePayload requests decoded account usage for one
// configured OAuth model provider on the target Node.
type CommandReadProviderAccountUsagePayload struct {
	Provider         ProviderID `json:"provider"`
	ExpectedRevision uint64     `json:"expected_revision"`
}

// ProviderAccountUsageWindow describes one rate-limit or quota window returned
// by a provider account usage endpoint.
type ProviderAccountUsageWindow struct {
	Key     string   `json:"key"`
	Used    *float64 `json:"used"`
	ResetMs *int64   `json:"reset_ms"`
}

// ProviderAccountUsageCredits describes prepaid credit availability on accounts
// that report credits alongside rate-limit windows.
type ProviderAccountUsageCredits struct {
	Balance   *float64 `json:"balance"`
	Unlimited *bool    `json:"unlimited"`
	Available *bool    `json:"available"`
}

// ProviderAccountUsageExtra describes optional pay-as-you-go extra usage.
type ProviderAccountUsageExtra struct {
	Used     *float64 `json:"used"`
	Limit    *float64 `json:"limit"`
	Currency *string  `json:"currency"`
	Enabled  *bool    `json:"enabled"`
}

// ProviderAccountUsage is the normalized account usage payload returned by the
// native provider-status Node module.
type ProviderAccountUsage struct {
	Windows []ProviderAccountUsageWindow `json:"windows"`
	Credits *ProviderAccountUsageCredits `json:"credits"`
	Extra   *ProviderAccountUsageExtra   `json:"extra"`
}
