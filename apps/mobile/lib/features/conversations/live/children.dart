import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import 'connection_status.dart';
import 'presentation.dart';
import 'turn_frame.dart';
import 'activity.dart';

/// Match Desktop's active, then claimed, then newest child presentation order.
Map<String, dynamic>? latestChild(Iterable<Map<String, dynamic>> children) {
  int priority(Map<String, dynamic> child) =>
      switch (object(child['run'])['status']) {
        'running' || 'stopping' => 2,
        'queued' => 0,
        _ => 1,
      };
  Map<String, dynamic>? selected;
  for (final child in children) {
    if (selected == null ||
        priority(child) > priority(selected) ||
        priority(child) == priority(selected) &&
            (object(child['run'])['sequence'] as int) >
                (object(selected['run'])['sequence'] as int)) {
      selected = child;
    }
  }
  return selected;
}

Map<String, dynamic>? childSession(
  Map<String, dynamic> snapshot,
  Map<String, dynamic> parent,
  Map<String, dynamic> child,
) {
  final run = object(child['run']);
  final origin = object(child['origin']);
  return objects(snapshot['sessions']).where((session) {
    final delegation = object(session['delegation']);
    return session['id'] == run['session'] &&
        session['project'] == parent['project'] &&
        session['worktree'] == run['worktree'] &&
        delegation.isNotEmpty &&
        delegation['session'] == origin['session'] &&
        delegation['role'] == origin['role'] &&
        objects(snapshot['worktrees']).any(
          (tree) =>
              tree['id'] == session['worktree'] &&
              tree['project'] == session['project'],
        );
  }).firstOrNull;
}

class ChildTask extends StatelessWidget {
  const ChildTask({
    super.key,
    required this.child,
    required this.title,
    required this.connected,
    this.continuing = false,
    this.onOpen,
  });

  final Map<String, dynamic> child;
  final String title;
  final bool connected;
  final bool continuing;
  final VoidCallback? onOpen;

  @override
  Widget build(BuildContext context) {
    final run = object(child['run']);
    final status = activeRun(run) && !connected
        ? context.tr('conversationUnsynced')
        : runLabel(run['status'] as String?, translate: context.tr);
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ListTile(
          key: ValueKey('child-${run['session']}'),
          contentPadding: EdgeInsets.zero,
          leading: connected && (activeRun(run) || continuing)
              ? SizedBox.square(
                  dimension: 20,
                  child: CircularProgressIndicator.adaptive(
                    value: MediaQuery.disableAnimationsOf(context) ? .75 : null,
                    strokeWidth: 2,
                  ),
                )
              : const AppIcon('spark', size: 20),
          title: ActivityText(
            title,
            active: connected && (activeRun(run) || continuing),
          ),
          subtitle: Text(status),
          trailing: onOpen == null ? null : const AppIcon('chevron'),
          onTap: onOpen,
        ),
        if (run['error'] != null)
          ErrorDetails(
            key: PageStorageKey('child-error-${run['session']}'),
            reason: text(object(run['error'])['message']),
          ),
      ],
    );
  }
}
