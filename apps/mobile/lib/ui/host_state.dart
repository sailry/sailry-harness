import 'package:flutter/material.dart';

import '../l10n/strings.dart';
import '../runtime/session.dart';
import 'failure_state.dart';

/// Shared unavailable presentation; connection state remains owned by Client.
class HostState extends StatelessWidget {
  const HostState({super.key, required this.added, this.failure, this.action});

  final bool added;
  // The host's last fault; a protocol mismatch names the side to update.
  final Object? failure;
  final Widget? action;

  @override
  Widget build(BuildContext context) => FailureState(
    message: context.tr(
      added
          ? versionMismatchKey(failure) ?? 'hostDisconnected'
          : 'hostConnectPrompt',
    ),
    action: action,
  );
}
