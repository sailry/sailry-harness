import {compact,total,integer,cost,cache,speed,context,severity,amount} from './format.js';

export function metrics(state,text) {
  const statistics = state.statistics, usage = statistics?.usage, generation = statistics?.generation;
  const unknown = text.composer_metric_unknown;
  const number = value => compact(value) ?? unknown;
  const charge = value => value == null ? unknown : '$'+amount(value);
  const row = (key,value) => ({label:{label:text[key]},value:value ?? unknown});
  const input = usage ? integer(usage.input) : null, cached = usage ? integer(usage.cached_input) : null;
  const uncached = input == null ? null : input > cached ? input-cached : 0n;
  const breakdown = statistics?.cost?.breakdown;
  const values = [
    ['composer_tokens',number(total(usage)),[
      row('composer_context_details',text.composer_context_usage.replace('%{used}',number(statistics?.context_tokens)).replace('%{limit}',number(state.context_limit))),
      row('composer_input_tokens',number(input)),row('composer_uncached_input',number(uncached)),
      row('composer_cached_input',number(cached)),row('composer_token_output',number(usage?.output)),
      row('composer_reasoning_tokens',number(usage?.reasoning))]],
    ['composer_speed',speed(generation),[
      row('composer_timed_responses',number(generation?.responses)),row('composer_timed_output',number(generation?.output_tokens))]],
    ['composer_cost',cost(statistics) == null ? null : '$'+cost(statistics),[
      row('composer_cost_input',charge(breakdown?.input)),row('composer_cost_output',charge(breakdown?.output)),
      row('composer_cost_cache_read',charge(breakdown?.cache_read)),row('composer_cost_cache_write',charge(breakdown?.cache_write)),
      row('composer_priced_responses',number(statistics?.cost?.responses))]],
    ['composer_cache',cache(usage),[
      row('composer_cached_input',number(cached)),row('composer_uncached_input',number(uncached)),row('composer_input_tokens',number(input))]],
    ['composer_turns',number(statistics?.turns),[row('composer_responses',number(statistics?.responses))]],
  ];
  const percent = context(statistics?.context_tokens,state.context_limit);
  const hint = state.compacting ? text.chat_compacting : percent == null ? text.composer_context_unknown
    : text.composer_context_usage.replace('%{used}',number(statistics.context_tokens)).replace('%{limit}',number(state.context_limit));
  return [...values.map(([id,value,details])=>({id,value:value ?? unknown,visible:state.has_messages,details})),
    {id:'context',value:{percent,loading:state.compacting,tone:severity(percent),hint},visible:!state.sidebar}];
}
