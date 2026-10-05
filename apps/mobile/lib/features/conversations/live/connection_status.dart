import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';

/// Disclosure state is presentation only; Rust owns subscription recovery.
class ErrorDetails extends StatelessWidget {
  const ErrorDetails({super.key, required this.reason});

  final String reason;

  @override
  Widget build(BuildContext context) => ExpansionTile(
    tilePadding: EdgeInsets.zero,
    dense: true,
    title: Text(context.tr('conversationErrorDetails')),
    children: [
      ConstrainedBox(
        constraints: const BoxConstraints(maxHeight: 180),
        child: SingleChildScrollView(
          key: const PageStorageKey('error-scroll'),
          child: Align(
            alignment: Alignment.centerLeft,
            child: SelectableText(
              reason,
              key: const PageStorageKey('error-text'),
            ),
          ),
        ),
      ),
    ],
  );
}

class ConnectionStatus extends StatelessWidget {
  const ConnectionStatus({
    super.key,
    required this.reconnecting,
    this.reason,
    this.onRetry,
  });

  final bool reconnecting;
  final String? reason;
  final VoidCallback? onRetry;

  @override
  Widget build(BuildContext context) => Semantics(
    liveRegion: true,
    child: Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          context.tr(
            reconnecting
                ? 'conversationReconnecting'
                : 'conversationUnavailable',
          ),
          textAlign: TextAlign.center,
          style: TextStyle(
            color: Theme.of(context).colorScheme.onSurfaceVariant,
          ),
        ),
        if (reason != null && reason!.isNotEmpty)
          ErrorDetails(
            key: PageStorageKey('connection-error-$reason'),
            reason: reason!,
          ),
        if (onRetry != null)
          Center(
            child: FilledButton(
              onPressed: onRetry,
              child: Text(context.tr('retry')),
            ),
          ),
      ],
    ),
  );
}
