import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import 'connection_status.dart';
import 'presentation.dart';
import 'turn_frame.dart';
import 'activity.dart';

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
