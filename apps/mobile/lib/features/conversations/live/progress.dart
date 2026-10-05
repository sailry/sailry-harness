import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import 'activity.dart';

/// Steps are the shared Client's validated progress, not inferred tool outcomes.
class TaskProgress extends StatelessWidget {
  const TaskProgress({
    super.key,
    required this.progress,
    this.continuing = false,
  });

  final Map<String, dynamic> progress;
  final bool continuing;

  @override
  Widget build(BuildContext context) {
    final steps = objects(progress['steps']);
    final running = steps.indexWhere((step) => step['state'] == 'in_progress');
    final active = continuing ? (running < 0 ? steps.length - 1 : running) : -1;
    final colors = Theme.of(context).colorScheme;
    return Card.outlined(
      margin: EdgeInsets.zero,
      color: colors.onSurface.withValues(alpha: .03),
      clipBehavior: Clip.antiAlias,
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          mainAxisSize: MainAxisSize.min,
          children: [
            ConstrainedBox(
              constraints: const BoxConstraints(maxHeight: 240),
              child: ListView.builder(
                key: const PageStorageKey('progress-scroll'),
                primary: false,
                shrinkWrap: true,
                padding: EdgeInsets.zero,
                itemCount: steps.length,
                itemBuilder: (context, index) {
                  final step = steps[index];
                  final done = ['completed', 'skipped'].contains(step['state']);
                  final label = context.tr(switch (step['state']) {
                    'completed' => 'completed',
                    'in_progress' => 'running',
                    'skipped' => 'conversationStepSkipped',
                    _ => 'conversationToolWaiting',
                  });
                  return ListTile(
                    contentPadding: EdgeInsets.zero,
                    dense: true,
                    leading: step['state'] == 'in_progress'
                        ? SizedBox.square(
                            dimension: 18,
                            child: CircularProgressIndicator(
                              value: MediaQuery.disableAnimationsOf(context)
                                  ? .75
                                  : null,
                              strokeWidth: 2,
                              color: colors.onSurfaceVariant,
                            ),
                          )
                        : AppIcon(done ? 'check' : 'clock', size: 18),
                    title: DefaultTextStyle.merge(
                      style: TextStyle(
                        decoration: done ? TextDecoration.lineThrough : null,
                        color: done
                            ? colors.onSurfaceVariant
                            : colors.onSurface,
                      ),
                      child: ActivityText(
                        text(step['description']),
                        active: index == active,
                        overflow: TextOverflow.visible,
                        color: done
                            ? colors.onSurfaceVariant
                            : colors.onSurface,
                      ),
                    ),
                    subtitle: Text(label),
                  );
                },
              ),
            ),
          ],
        ),
      ),
    );
  }
}
