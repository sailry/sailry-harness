import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';

/// Presentation of Node-owned totals, independent of loaded message pages.
class ConversationStatistics extends StatelessWidget {
  const ConversationStatistics({super.key, required this.data});

  final Map<String, dynamic> data;

  @override
  Widget build(BuildContext context) {
    final usage = object(data['usage']);
    final cost = object(data['cost']);
    final generation = object(data['generation']);
    final responses = number(data['responses']);
    final turns = number(data['turns']);
    final elapsed = number(generation['elapsed_us']);
    final speed = elapsed > 0
        ? tr('conversationStatsSpeedValue').replaceAll(
            '{value}',
            (number(generation['output_tokens']) * 1000000 / elapsed)
                .toStringAsFixed(1),
          )
        : null;
    final cache = number(usage['input']) > 0
        ? '${(100 * number(usage['cached_input']) / number(usage['input'])).toStringAsFixed(0)}%'
        : null;
    final costText = cost.isEmpty
        ? null
        : '${_amount(cost['usd_micros'])}${number(cost['responses']) < responses ? '+' : ''}';
    final groups = <(String, List<(String, String)>)>[
      (
        'conversationStatsOverview',
        [
          if (turns > 0) ('conversationStatsTurns', _count(turns)),
          if (responses > 0) ('conversationStatsResponses', _count(responses)),
          if (data['context_tokens'] != null)
            (
              'conversationStatsContext',
              _count(number(data['context_tokens'])),
            ),
        ],
      ),
      (
        'conversationStatsTokenGroup',
        [
          if (usage.isNotEmpty) ...[
            (
              'conversationStatsTokens',
              _count(number(usage['input']) + number(usage['output'])),
            ),
            ('conversationStatsInput', _count(number(usage['input']))),
            ('conversationStatsOutput', _count(number(usage['output']))),
            ('conversationStatsCached', _count(number(usage['cached_input']))),
            ('conversationStatsReasoning', _count(number(usage['reasoning']))),
          ],
          if (cache != null) ('conversationStatsCacheRate', cache),
        ],
      ),
      (
        'conversationStatsCostGroup',
        [
          if (costText != null) ...[
            ('conversationStatsCost', costText),
            (
              'conversationStatsCostCoverage',
              '${number(cost['responses'])} / $responses',
            ),
            for (final (field, label) in const [
              ('input', 'conversationStatsInputCost'),
              ('output', 'conversationStatsOutputCost'),
              ('cache_read', 'conversationStatsCacheReadCost'),
              ('cache_write', 'conversationStatsCacheWriteCost'),
            ])
              if (object(cost['breakdown'])[field] != null)
                (label, _amount(object(cost['breakdown'])[field])),
          ],
        ],
      ),
      (
        'conversationStatsGenerationGroup',
        [
          if (speed != null) ...[
            ('conversationStatsSpeed', speed),
            (
              'conversationStatsTimingCoverage',
              '${number(generation['responses'])} / $responses',
            ),
          ],
        ],
      ),
    ].where((group) => group.$2.isNotEmpty).toList();
    final style = Theme.of(context).textTheme.bodyMedium;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        Padding(
          padding: const EdgeInsets.only(bottom: 12),
          child: Text(
            tr('conversationStats'),
            style: Theme.of(context).textTheme.titleMedium,
          ),
        ),
        if (groups.isEmpty) Text(tr('conversationStatsEmpty'), style: style),
        for (final (index, (heading, rows)) in groups.indexed) ...[
          if (index > 0) const Divider(height: 24),
          Padding(
            padding: const EdgeInsets.only(bottom: 8),
            child: Text(
              tr(heading),
              style: Theme.of(context).textTheme.labelMedium?.copyWith(
                color: Theme.of(context).colorScheme.onSurfaceVariant,
              ),
            ),
          ),
          for (final (label, value) in rows)
            Padding(
              key: ValueKey(label),
              padding: const EdgeInsets.symmetric(vertical: 4),
              child: Row(
                children: [
                  Expanded(child: Text(tr(label), style: style)),
                  const SizedBox(width: 16),
                  Flexible(
                    child: Text(
                      value,
                      style: style,
                      textAlign: TextAlign.right,
                    ),
                  ),
                ],
              ),
            ),
        ],
      ],
    );
  }
}

String _count(num value) {
  for (final (divisor, suffix) in const [
    (1000000000, 'B'),
    (1000000, 'M'),
    (1000, 'k'),
  ]) {
    if (value >= divisor) {
      return '${(value / divisor).toStringAsFixed(1)}$suffix';
    }
  }
  return value.toString();
}

String _amount(Object? value) {
  final micros = number(value).toInt();
  final cents = (micros % 1000000 ~/ 10000).toString().padLeft(2, '0');
  return '\$${micros ~/ 1000000}.$cents';
}
