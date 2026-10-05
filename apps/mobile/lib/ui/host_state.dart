import 'package:flutter/material.dart';

import '../l10n/strings.dart';
import 'failure_state.dart';

/// Shared unavailable presentation; connection state remains owned by Client.
class HostState extends StatelessWidget {
  const HostState({super.key, required this.added, this.action});

  final bool added;
  final Widget? action;

  @override
  Widget build(BuildContext context) => FailureState(
    message: context.tr(added ? 'hostDisconnected' : 'hostConnectPrompt'),
    action: action,
  );
}
