import test from 'node:test';
import assert from 'node:assert/strict';
import {heatmap, trend} from '../dev.sailry.platform/desktop/charts.js';

const labels = {usage_other_models:'Other models',usage_month:'Month %{month}',composer_metric_unknown:'Unknown',composer_tokens:'Token',usage_utc_time:'UTC',usage_column_cached:'Cached input',usage_uncached_input:'Uncached input',usage_column_output:'Output'};
const text = key => labels[key] ?? key;
const usage = (input = '12', output = '3', cached_input = '4') => ({input,output,cached_input,reasoning:'2'});
function point(date, tokens = usage(), responses = tokens ? '1' : '0') {
  return {start_ms:String(Date.parse(date)),metrics:{responses,tokens},models:{}};
}
const values = chart => chart.buckets.map(bucket => bucket.tooltip.footer.value);

// The exact report wire format uses decimal strings, including timestamps.
test('keeps disjoint categories and exact tooltip totals', () => {
  const chart = trend([point('2024-01-01')], 'daily', text);
  assert.deepEqual(chart.buckets[0].values, [4,8,3]);
  assert.equal(chart.maximum, 15);
  assert.deepEqual(chart.series, [{id:'cache',color:'chart_3'},{id:'input',color:'chart_2'},{id:'output',color:'chart_4'}]);
  assert.equal(chart.buckets[0].tooltip.footer.value, '15');
  assert.deepEqual(chart.buckets[0].tooltip.rows.map(row => row.value), ['4','8','3']);
  const large = trend([point('2024-01-01', usage('18446744073709551615','18446744073709551615','18446744073709551615'))], 'daily', text);
  assert.deepEqual(large.buckets[0].tooltip.rows.map(row => row.value), ['18446744073709551615','0','18446744073709551615']);
  assert.equal(large.buckets[0].tooltip.footer.value, '36893488147419103230');
  assert.doesNotThrow(() => JSON.stringify(large));
});

test('caps cached input at input without counting reasoning twice', () => {
  const chart = trend([point('2024-01-01', usage('2','3','7'))], 'daily', text);
  assert.deepEqual(chart.buckets[0].values, [2,0,3]);
  assert.equal(chart.buckets[0].tooltip.footer.value, '5');
});

test('groups Monday UTC weeks over year and leap boundaries', () => {
  for (const start of ['2023-12-31', '2024-02-25']) {
    const points = Array.from({length:9}, (_, index) => point(new Date(Date.parse(start) + index * 86400000).toISOString(), index === 3 ? null : usage()));
    const daily = trend(points, 'daily', text), weekly = trend(points, 'weekly', text);
    assert.equal(daily.buckets.length, 9);
    assert.equal(daily.buckets[3].tooltip.footer.value, '0');
    assert.deepEqual(values(weekly), ['15','90','15']);
    assert.equal(weekly.buckets[0].tooltip.heading, `${start} · UTC`);
    const day = offset => new Date(Date.parse(start) + offset * 86400000).toISOString().slice(0,10);
    assert.equal(weekly.buckets[1].tooltip.heading, `${day(1)} – ${day(7)} · UTC`);
    assert.equal(values(daily).reduce((sum,value) => sum + BigInt(value), 0n), values(weekly).reduce((sum,value) => sum + BigInt(value), 0n));
  }
});

test('keeps empty and unknown distinct and poisons incomplete weeks', () => {
  const points = [point('2024-01-01', null), point('2024-01-02'), point('2024-01-03', null, '1')];
  const daily = trend(points, 'daily', text), weekly = trend(points, 'weekly', text);
  assert.deepEqual(values(daily), ['0','15','Unknown']);
  assert.equal(daily.buckets[2].values, null);
  assert.deepEqual(values(weekly), ['Unknown']);
  assert.equal(weekly.buckets[0].values, null);
  const empty = trend([], 'daily', text);
  assert.deepEqual(empty.buckets, []);
  assert.deepEqual(empty.ticks, [{value:0,label:'0'},{value:1,label:'1'}]);
});

test('sorts model totals and folds only beyond the first eight', () => {
  const value = point('2024-01-01', usage('9550','1000','5000'), '10');
  value.models = Object.fromEntries(Array.from({length:10}, (_,index) => [`model-${index}`,usage(String(1000 - index * 10), '100', '500')]));
  const chart = heatmap([value], text), cell = chart.cells[0];
  assert.equal(cell.tooltip.title, '10.6k Token');
  assert.equal(cell.tooltip.rows.length, 9);
  assert.deepEqual(cell.tooltip.rows[0], {label:'model-0',value:'1.1k'});
  assert.deepEqual(cell.tooltip.rows[8], {label:'Other models',value:'2.0k'});
  assert.equal(cell.intensity, 1);
  value.models = {z:usage('1','1'),a:usage('1','1'),empty:usage('0','0')};
  assert.deepEqual(heatmap([value], text).cells[0].tooltip.rows.map(row => row.label), ['a','z']);
});

test('places local calendar cells without daylight-saving drift', () => {
  const prior = process.env.TZ;
  try {
    process.env.TZ = 'America/New_York';
    const points = Array.from({length:35}, (_,index) => point(new Date(Date.UTC(2024,1,28,12) + index * 86400000).toISOString(), null));
    const chart = heatmap(points, text);
    assert.equal(chart.rows, 7);
    assert.equal(chart.columns, 6);
    assert.deepEqual([chart.cells[0].column,chart.cells[0].row], [0,2]);
    assert.match(chart.cells[1].tooltip.heading, /^2024-02-29 07:00$/);
    assert.deepEqual([chart.cells[5].column,chart.cells[5].row], [1,0]);
    assert.match(chart.cells[12].tooltip.heading, /^2024-03-11 08:00$/);
    assert.equal(chart.cells[2].label_group, '2024-3');
    assert.equal(chart.cells[2].label, 'Month 3');
    // Trend still uses UTC dates when the local zone is west of midnight.
    assert.equal(trend([point('2024-03-11')], 'weekly', text).buckets[0].label, '03-11');
  } finally { if (prior === undefined) delete process.env.TZ; else process.env.TZ = prior; }
});

test('retains one-row short ranges and square-root intensity', () => {
  const points = [point('2024-01-01',null),point('2024-01-02',null,'1'),point('2024-01-03',usage('1','0')),point('2024-01-04',usage('4','0')),point('2024-01-05',usage('9','0')),point('2024-01-06',usage('16','0'))];
  const chart = heatmap(points,text);
  assert.equal(chart.rows,1);
  assert.equal(chart.columns,6);
  assert.deepEqual(chart.cells.map(cell => cell.intensity),[0,0,0.25,0.45,0.7,1]);
  assert.deepEqual(chart.cells.map(cell => cell.column),[0,1,2,3,4,5]);
  assert.equal(chart.cells[0].tooltip.title,'0 Token');
  assert.equal(chart.cells[1].tooltip.title,'Unknown Token');
  assert.ok(chart.cells.every(cell => cell.row === 0 && cell.label_group === undefined));
  assert.doesNotThrow(() => JSON.stringify(chart));
});

test('uses compact axis labels with exact literal data kept separate', () => {
  const chart = trend([point('2024-01-01',usage('1200','300'))], 'daily', text);
  assert.deepEqual(chart.ticks,[{value:0,label:'0'},{value:375,label:'375'},{value:750,label:'750'},{value:1125,label:'1.1k'},{value:1500,label:'1.5k'}]);
  assert.equal(chart.buckets[0].tooltip.footer.value,'1500');
});
