import 'package:flutter/material.dart';

import '../../../runtime/json.dart';

Map<String, dynamic>? runningRecipient(Map<String, dynamic> page) =>
    objects(page['runs'])
        .where((run) => run['kind'] == 'task' && run['status'] == 'running')
        .firstOrNull;

/// Input waiting to join the running turn is marked by color, not copy.
class GuidanceStatus extends StatelessWidget {
  const GuidanceStatus({super.key, required this.state});

  final Object? state;

  @override
  Widget build(BuildContext context) => state == 'pending'
      ? Icon(Icons.redo, size: 14, color: Theme.of(context).colorScheme.primary)
      : const SizedBox.shrink();
}
