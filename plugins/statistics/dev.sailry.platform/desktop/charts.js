// Date, category and tooltip policy adapted from Sailry ef5e82ea,
// plugins/builtin/statistics/desktop/workspace/charts{,/trend}.rs.
import {compact, integer, total} from './format.js';
export {compact};

const DAY = 86400000;
const pad = value => String(value).padStart(2, '0');
const localDay = date => Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / DAY;
const localDate = date => `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
const utcDate = day => new Date(day * DAY).toISOString().slice(0, 10);
const timestamp = value => new Date(Number(value));
const valueOf = point => total(point.metrics.tokens) ?? (integer(point.metrics.responses) === 0n ? 0n : null);

export function heatmap(points, text) {
  const first = points.length ? timestamp(points[0].start_ms) : null;
  const start = first && Number.isFinite(first.getTime())
    ? localDay(first) - (first.getDay() + 6) % 7 : null;
  const days = [];
  for (const point of points) {
    const date = timestamp(point.start_ms);
    if (start === null || !Number.isFinite(date.getTime())) continue;
    const offset = localDay(date) - start;
    if (offset < 0) continue;
    const models = Object.entries(point.models).map(([name, tokens]) => [name, total(tokens)])
      .filter(([, value]) => value > 0n)
      .sort(([a, av], [b, bv]) => av !== bv ? (av > bv ? -1 : 1) : a < b ? -1 : a > b ? 1 : 0);
    if (models.length > 8) {
      const other = models.splice(8).reduce((sum, [, value]) => sum + value, 0n);
      models.push([text('usage_other_models'), other]);
    }
    days.push({date, offset, models, tokens:valueOf(point)});
  }
  const rows = days.length <= 31 ? 1 : 7;
  const columns = rows === 1 ? Math.max(days.length, 1) : Math.floor((days.at(-1)?.offset ?? 0) / 7) + 1;
  const maximum = days.reduce((maximum, day) => day.tokens > maximum ? day.tokens : maximum, 0n);
  const cells = days.map(day => {
    const level = day.tokens > 0n ? Math.ceil(Math.sqrt(Number(day.tokens) / Number(maximum)) * 4) : 0;
    const date = day.date;
    return {
      column:rows === 1 ? day.offset - days[0].offset : Math.floor(day.offset / 7),
      row:rows === 1 ? 0 : day.offset % 7,
      intensity:[0, 0.25, 0.45, 0.7, 1][Math.min(level, 4)],
      label:rows === 1 ? `${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
        : text('usage_month').replace('%{month}', String(date.getMonth() + 1)),
      ...(rows === 1 ? {} : {label_group:`${date.getFullYear()}-${date.getMonth() + 1}`}),
      tooltip:{
        heading:`${localDate(date)} ${pad(date.getHours())}:${pad(date.getMinutes())}`,
        title:`${compact(day.tokens) ?? text('composer_metric_unknown')} ${text('composer_tokens')}`,
        rows:day.models.map(([label, value]) => ({label, value:compact(value)})),
      },
    };
  });
  return {rows, columns, cells};
}

function buckets(points, period) {
  const grouped = new Map();
  for (const point of points) {
    const date = timestamp(point.start_ms);
    if (!Number.isFinite(date.getTime())) continue;
    // The Node report already groups UTC days; local time must not move a week boundary.
    const day = Math.floor(date.getTime() / DAY);
    const start = period === 'weekly' ? day - (date.getUTCDay() + 6) % 7 : day;
    let bucket = grouped.get(start);
    if (!bucket) {
      bucket = {start, first:day, last:day, values:[0n, 0n, 0n]};
      grouped.set(start, bucket);
    }
    bucket.first = Math.min(bucket.first, day);
    bucket.last = Math.max(bucket.last, day);
    const tokens = point.metrics.tokens;
    if (tokens && bucket.values) {
      const input = integer(tokens.input), cached = integer(tokens.cached_input);
      const cache = cached < input ? cached : input;
      bucket.values[0] += cache;
      bucket.values[1] += input - cache;
      bucket.values[2] += integer(tokens.output);
    } else if (!tokens && integer(point.metrics.responses) > 0n) {
      bucket.values = null;
    }
  }
  return [...grouped.values()].sort((a, b) => a.start - b.start);
}

export function trend(points, period, text) {
  const grouped = buckets(points, period);
  const sum = values => values.reduce((sum, value) => sum + value, 0n);
  const maximum = grouped.reduce((maximum, bucket) => {
    const value = bucket.values ? sum(bucket.values) : 0n;
    return value > maximum ? value : maximum;
  }, 0n);
  const labels = ['usage_column_cached', 'usage_uncached_input', 'usage_column_output'];
  const unknown = text('composer_metric_unknown');
  const divisions = Number(maximum < 1n ? 1n : maximum > 4n ? 4n : maximum);
  const scale = Number(maximum > 0n ? maximum : 1n);
  return {
    series:[{id:'cache', color:'chart_3'}, {id:'input', color:'chart_2'}, {id:'output', color:'chart_4'}],
    maximum:Number(maximum),
    ticks:Array.from({length:divisions + 1}, (_, index) => {
      const value = scale * index / divisions;
      return {value, label:compact(BigInt(Math.floor(value)))};
    }),
    buckets:grouped.map(bucket => ({
      label:utcDate(bucket.start).slice(5),
      values:bucket.values?.map(Number) ?? null,
      tooltip:{
        heading:`${utcDate(bucket.first)}${bucket.first === bucket.last ? '' : ` – ${utcDate(bucket.last)}`} · ${text('usage_utc_time')}`,
        rows:labels.map((label, index) => ({label:text(label), value:bucket.values?.[index].toString() ?? unknown})),
        footer:{label:text('composer_tokens'), value:bucket.values ? sum(bucket.values).toString() : unknown},
      },
    })),
  };
}
