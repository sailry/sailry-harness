import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';

Map<String, dynamic>? runningRecipient(Map<String, dynamic> page) =>
    objects(page['runs'])
        .where((run) => run['kind'] == 'task' && run['status'] == 'running')
        .firstOrNull;

String? statusKey(Object? state) => switch (state) {
  'pending' => 'guidancePending',
  'delivered' => 'guidanceDelivered',
  'next_turn' => 'guidanceNextTurn',
  _ => null,
};

class GuidanceStatus extends StatelessWidget {
  const GuidanceStatus({super.key, required this.state});

  final Object? state;

  @override
  Widget build(BuildContext context) {
    final key = statusKey(state);
    return key == null
        ? const SizedBox.shrink()
        : Text(
            context.tr(key),
            style: Theme.of(context).textTheme.labelSmall?.copyWith(
              color: Theme.of(context).colorScheme.onSurfaceVariant,
            ),
          );
  }
}
