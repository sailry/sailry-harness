import 'package:flutter/material.dart';

import '../l10n/strings.dart';
import '../runtime/session.dart';
import 'connection_status.dart';
import 'failure_state.dart';

/// Shared unavailable presentation; connection state remains owned by Client.
class HostState extends StatelessWidget {
  const HostState({
    super.key,
    required this.added,
    this.failure,
    this.showFailureReason = false,
    this.action,
  });

  final bool added;
  // The host's last fault; a protocol mismatch names the side to update.
  final Object? failure;
  final bool showFailureReason;
  final Widget? action;

  @override
  Widget build(BuildContext context) {
    final version = versionMismatchKey(failure);
    final reason = showFailureReason && failure != null && version == null
        ? connectionFailureKey(failure) ?? 'connectionFailureUnknown'
        : null;
    return FailureState(
      message: context.tr(
        added ? version ?? 'hostDisconnected' : 'hostConnectPrompt',
      ),
      description: added && reason != null ? context.tr(reason) : null,
      action: action,
    );
  }
}
