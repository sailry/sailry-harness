import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import 'activity.dart';

/// The latest validated progress of a turn.
Map<String, dynamic>? turnProgress(
  List<Map<String, dynamic>> calls,
  Object? turn,
) => calls
    .where((call) => call['turn'] == turn && call['progress'] != null)
    .map((call) => object(call['progress']))
    .lastOrNull;

/// Settled and total steps; skipped steps count as settled.
(int, int) progressCounts(Map<String, dynamic> progress) {
  final steps = objects(progress['steps']);
  final done = steps
      .where((step) => ['completed', 'skipped'].contains(step['state']))
      .length;
  return (done, steps.length);
}

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
                // One line per step: the icon carries its state, which
                // remains spoken with the step.
                itemBuilder: (context, index) {
                  final step = steps[index];
                  final state = step['state'];
                  final done = ['completed', 'skipped'].contains(state);
                  final description = text(step['description']);
                  final label = context.tr(switch (state) {
                    'completed' => 'completed',
                    'in_progress' => 'running',
                    'skipped' => 'conversationStepSkipped',
                    _ => 'conversationToolWaiting',
                  });
                  final muted = done
                      ? colors.onSurfaceVariant
                      : colors.onSurface;
                  return Semantics(
                    key: ValueKey('progress-step-$index'),
                    label: '$description, $label',
                    excludeSemantics: true,
                    child: Padding(
                      padding: const EdgeInsets.symmetric(vertical: 7),
                      child: Row(
                        spacing: 10,
                        children: [
                          SizedBox.square(
                            dimension: 16,
                            child: state == 'in_progress'
                                ? Padding(
                                    padding: const EdgeInsets.all(1),
                                    child: CircularProgressIndicator(
                                      value:
                                          MediaQuery.disableAnimationsOf(
                                            context,
                                          )
                                          ? .75
                                          : null,
                                      strokeWidth: 2,
                                      color: colors.onSurfaceVariant,
                                    ),
                                  )
                                : AppIcon(
                                    switch (state) {
                                      'completed' => 'check',
                                      'skipped' => 'arrow',
                                      _ => 'clock',
                                    },
                                    size: 16,
                                    color: colors.onSurfaceVariant,
                                  ),
                          ),
                          Expanded(
                            child: DefaultTextStyle.merge(
                              maxLines: 1,
                              softWrap: false,
                              style: TextStyle(
                                decoration: done
                                    ? TextDecoration.lineThrough
                                    : null,
                                color: muted,
                              ),
                              child: ActivityText(
                                description,
                                active: index == active,
                                color: muted,
                              ),
                            ),
                          ),
                        ],
                      ),
                    ),
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
