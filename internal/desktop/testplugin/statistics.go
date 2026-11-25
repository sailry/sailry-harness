package testplugin

const StatisticsName = "statistics"
const StatisticsTurnsID = "composer_turns"

func Statistics() Package {
	return Package{Name: StatisticsName, Platform: `{
  "api_version":"v1","scope":"host","actions":["conversation.read","usage.read"],
  "desktop":{"ui_entry":"dev.sailry.platform/statistics.js","resources":["dev.sailry.platform/statistics.js"]},
  "ui":[
    {"id":"tokens","slot":"statistics","kind":"metric","label":{"label":"Tokens"},"order":1},
    {"id":"speed","slot":"statistics","kind":"metric","label":{"label":"Speed"},"order":2},
    {"id":"cost","slot":"statistics","kind":"metric","label":{"label":"Cost"},"order":3},
    {"id":"cache","slot":"statistics","kind":"metric","label":{"label":"Cache"},"order":4},
    {"id":"` + StatisticsTurnsID + `","slot":"statistics","kind":"metric","label":{"label":"Turns"},"order":5}
  ]
}`, Files: map[string]string{"dev.sailry.platform/statistics.js": `
import {View,div} from 'gpui-kit';
import {publishContributions} from 'sailry/sdk';
import {readComposer,nextComposerChange} from 'sailry/ui';

const count = n => n >= 1000000 ? (n/1000000).toFixed(1)+'M' : n >= 1000 ? (n/1000).toFixed(1)+'k' : String(n);
const tokens = usage => Number(usage?.input ?? 0) + Number(usage?.output ?? 0);
const price = micros => (Math.floor(micros/10000)/100).toFixed(2);
const rate = (output, elapsed) => Number(elapsed) > 0 ? (Number(output)*1000000/Number(elapsed)).toFixed(1)+' tok/s' : '—';
const duration = us => us == null ? '—' : (Number(us)/1000).toFixed(1)+' ms';
const row = (label,value) => ({label:{label},value});
const metric = (id,value,details) => ({id,value,details,visible:true,enabled:true});
const usageIDs = ['tokens','speed','cost','cache'];
const turnsID = '` + StatisticsTurnsID + `';

export default class StatisticsFixture extends View {
  init(_props,cx) {
    const first = readComposer();
    this.publish(first.statistics);
    cx.spawn(async () => {
      let current = first;
      while (true) {
        current = await nextComposerChange(current.cursor);
        this.publish(current.statistics);
      }
    });
  }
  publish(stats) {
    const turns = String(stats?.turns ?? 0);
    const turnMetric = Number(turns) > 0 ? metric(turnsID,turns,[row('Turns',turns)]) : {id:turnsID,visible:false};
    if (!stats?.usage) {
      publishContributions([...usageIDs.map(id => ({id,visible:false})),turnMetric]);
      return;
    }
    const own = tokens(stats.usage), delegated = tokens(stats.delegated?.tokens);
    const cost = Number(stats.cost?.usd_micros ?? 0), extra = Number(stats.delegated?.cost?.usd_micros ?? 0);
    const input = Number(stats.usage.input), cached = Number(stats.usage.cached_input);
    const speed = rate(stats.throughput?.output_tokens,stats.throughput?.elapsed_us);
    const recent = stats.observations.recent, attempts = recent[recent.length-1]?.attempts ?? [];
    const last = attempts[attempts.length-1];
    publishContributions([
      metric('tokens',count(own+delegated),[row('This session',count(own)),row('Subagents',count(delegated))]),
      metric('speed',speed,[row('End-to-end TPS',speed),row('Average TTFT',duration(stats.first_token_us)),
        row('Latest end-to-end TPS',rate(last?.usage?.output,last?.elapsed_us)),row('Latest request TTFT',duration(last?.first_token_us))]),
      metric('cost',price(cost+extra),[row('This session','$'+price(cost)),row('Subagents','$'+price(extra))]),
      metric('cache',(input > 0 ? (100*cached/input).toFixed(0) : '0')+'%',
        [row('Cached input',count(cached)),row('Uncached input',count(input-cached)),row('Input tokens',count(input))]),
      turnMetric
    ]);
  }
  render() { return div(); }
}`}}
}
