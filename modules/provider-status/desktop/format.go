package providerstatusdesktop

import (
	"fmt"
	"math"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	packageName       = "provider-status"
	footerControlID   = "usage"
	providerControlID = "provider_usage"
	metricOrder       = 10
)

type translateFunc func(key string) string

type formattedRow struct {
	row        conversation.Row
	isCredits  bool
	isProvider bool
}

// formatAmount formats a number with up to 2 decimal places, stripping
// trailing ".00" or a trailing zero after the decimal point.
func formatAmount(value float64) string {
	if math.IsNaN(value) || math.IsInf(value, 0) {
		return "0"
	}
	rounded := math.Round(value*100) / 100
	if rounded == 0 {
		rounded = 0
	}
	s := fmt.Sprintf("%.2f", rounded)
	if strings.HasSuffix(s, ".00") {
		return s[:len(s)-3]
	}
	if strings.Contains(s, ".") && strings.HasSuffix(s, "0") {
		return s[:len(s)-1]
	}
	return s
}

func formatAmountPtr(value *float64) string {
	if value == nil {
		return formatAmount(0)
	}
	return formatAmount(*value)
}

// remainingPercent converts raw used percentage to remaining percentage clamped
// to [0, 100]. Raw used values above 100 render as 0%.
func remainingPercent(used *float64) (float64, bool) {
	if used == nil || math.IsNaN(*used) || math.IsInf(*used, 0) {
		return 0, false
	}
	rem := 100 - *used
	if rem < 0 {
		rem = 0
	} else if rem > 100 {
		rem = 100
	}
	return rem, true
}

// formatResetTime formats an epoch-millisecond timestamp in local time as
// YYYY-MM-DD HH:MM.
func formatResetTime(ms int64, loc *time.Location) string {
	if loc == nil {
		loc = time.Local
	}
	at := time.UnixMilli(ms).In(loc)
	return fmt.Sprintf("%04d-%02d-%02d %02d:%02d", at.Year(), int(at.Month()), at.Day(), at.Hour(), at.Minute())
}

func windowLabel(key string, t translateFunc) string {
	switch key {
	case "five_hour":
		return t("provider_status_five_hour")
	case "seven_day":
		return t("provider_status_seven_day")
	case "seven_day_sonnet":
		return t("provider_status_seven_day_sonnet")
	case "seven_day_opus":
		return t("provider_status_seven_day_opus")
	case "seven_day_oauth_apps":
		return t("provider_status_seven_day_oauth_apps")
	case "seven_day_cowork":
		return t("provider_status_seven_day_cowork")
	case "primary":
		return t("provider_status_primary")
	case "secondary":
		return t("provider_status_secondary")
	default:
		return key
	}
}

func buildDetails(snapshot *protocol.ProviderAccountUsage, t translateFunc, loc *time.Location) (value string, rows []formattedRow, ok bool) {
	if snapshot == nil || (len(snapshot.Windows) == 0 && snapshot.Credits == nil && snapshot.Extra == nil) {
		return "", nil, false
	}
	var (
		hasMeasured  bool
		minRemaining float64
	)
	for _, win := range snapshot.Windows {
		if rem, measured := remainingPercent(win.Used); measured {
			if !hasMeasured || rem < minRemaining {
				minRemaining = rem
				hasMeasured = true
			}
			rows = append(rows, formattedRow{
				row: conversation.Row{
					Label:    windowLabel(win.Key, t),
					Value:    formatAmount(rem) + "%",
					Meter:    true,
					Progress: rem,
				},
				isProvider: true,
			})
		}
		if win.ResetMs != nil {
			rows = append(rows, formattedRow{
				row: conversation.Row{
					Label: t("provider_status_resets"),
					Value: formatResetTime(*win.ResetMs, loc),
				},
			})
		}
	}

	var (
		creditsText string
		hasCredits  bool
	)
	if snapshot.Credits != nil {
		switch {
		case snapshot.Credits.Unlimited != nil && *snapshot.Credits.Unlimited:
			creditsText = t("provider_status_unlimited")
			hasCredits = true
		case snapshot.Credits.Balance != nil:
			creditsText = formatAmount(*snapshot.Credits.Balance)
			hasCredits = true
		}
	}
	if hasCredits {
		rows = append(rows, formattedRow{
			row: conversation.Row{
				Label: t("provider_status_credits"),
				Value: creditsText,
			},
			isCredits:  true,
			isProvider: true,
		})
	}

	if snapshot.Extra != nil {
		// Claude's extra_usage amounts use minor currency units, unlike dollar windows.
		var usedPtr, limitPtr *float64
		if snapshot.Extra.Used != nil {
			v := *snapshot.Extra.Used / 100
			usedPtr = &v
		}
		if snapshot.Extra.Limit != nil {
			v := *snapshot.Extra.Limit / 100
			limitPtr = &v
		}
		extraVal := formatAmountPtr(usedPtr) + " / " + formatAmountPtr(limitPtr)
		if snapshot.Extra.Currency != nil && *snapshot.Extra.Currency != "" {
			extraVal += " " + *snapshot.Extra.Currency
		}
		rows = append(rows, formattedRow{
			row: conversation.Row{
				Label: t("provider_status_extra"),
				Value: extraVal,
			},
		})
	}

	switch {
	case hasMeasured:
		return formatAmount(minRemaining) + "%", rows, true
	case hasCredits:
		return creditsText, rows, true
	default:
		return "", rows, false
	}
}

// buildFooterMetric projects a provider account usage snapshot into the
// conversation context bar's lead metric.
func buildFooterMetric(snapshot *protocol.ProviderAccountUsage, t translateFunc, loc *time.Location) (conversation.Metric, bool) {
	value, details, ok := buildDetails(snapshot, t, loc)
	if !ok {
		return conversation.Metric{}, false
	}
	rows := make([]conversation.Row, len(details))
	for i, d := range details {
		rows[i] = d.row
	}
	return conversation.Metric{
		ID:    packageName + ":" + footerControlID,
		Order: metricOrder,
		Label: t("provider_status_usage"),
		Icon:  icons.Gauge,
		Value: value,
		Rows:  rows,
	}, true
}

// buildProviderMetric projects a provider account usage snapshot for the
// Settings > Providers row, retaining only meter and credit rows.
func buildProviderMetric(snapshot *protocol.ProviderAccountUsage, t translateFunc, loc *time.Location) (conversation.Metric, bool) {
	value, details, ok := buildDetails(snapshot, t, loc)
	if !ok {
		return conversation.Metric{}, false
	}
	rows := make([]conversation.Row, 0, len(details))
	for _, d := range details {
		if d.isProvider {
			rows = append(rows, d.row)
		}
	}
	return conversation.Metric{
		ID:    packageName + ":" + providerControlID,
		Order: metricOrder,
		Label: t("provider_status_usage"),
		Icon:  icons.Gauge,
		Value: value,
		Rows:  rows,
	}, true
}

// providerUsageLines selects up to two inline meters, or a single credits/value
// line when no measured windows exist.
func providerUsageLines(figure conversation.Metric) []settings.UsageLine {
	var lines []settings.UsageLine
	for _, row := range figure.Rows {
		if row.Meter && len(lines) < 2 {
			lines = append(lines, settings.UsageLine{
				Label:    strings.TrimSpace(row.Label),
				Value:    row.Value,
				Meter:    true,
				Progress: row.Progress,
			})
		}
	}
	if len(lines) != 0 {
		return lines
	}
	if len(figure.Rows) != 0 {
		return []settings.UsageLine{{Value: figure.Rows[0].Value}}
	}
	return []settings.UsageLine{{Value: figure.Value}}
}

// buildChannelUsage builds the settings.ChannelUsage entry rendered in the
// provider row along with its hover UsageCard.
func buildChannelUsage(snapshot *protocol.ProviderAccountUsage, t translateFunc, loc *time.Location) *settings.ChannelUsage {
	figure, ok := buildProviderMetric(snapshot, t, loc)
	if !ok {
		return nil
	}
	return &settings.ChannelUsage{
		Lines: providerUsageLines(figure),
		Icon:  figure.Icon,
		Card: func(c *ui.Context, over ui.Element) {
			conversation.UsageCard(c, over, figure)
		},
	}
}
