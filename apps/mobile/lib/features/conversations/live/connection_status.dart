import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../ui/kit.dart';

/// Disclosure state is presentation only; Client owns subscription recovery.
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

/// A compact notice above the transcript viewport.
class ConnectionBanner extends StatelessWidget {
  const ConnectionBanner({
    super.key,
    required this.visible,
    required this.reconnecting,
    this.topInset = 0,
    this.failureKey,
    this.onRetry,
  });

  final bool visible;
  final bool reconnecting;
  final double topInset;
  final String? failureKey;
  final VoidCallback? onRetry;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final colors = theme.colorScheme;
    return AnimatedSwitcher(
      duration: MediaQuery.disableAnimationsOf(context)
          ? Duration.zero
          : const Duration(milliseconds: 220),
      switchInCurve: Curves.easeOutCubic,
      switchOutCurve: Curves.easeInCubic,
      transitionBuilder: (child, animation) => FadeTransition(
        opacity: animation,
        child: SizeTransition(
          sizeFactor: animation,
          alignment: Alignment.topCenter,
          child: child,
        ),
      ),
      child: !visible
          ? const SizedBox(width: double.infinity)
          : Padding(
              key: const ValueKey('connection-banner'),
              padding: EdgeInsets.fromLTRB(16, topInset + 4, 16, 8),
              child: Semantics(
                liveRegion: true,
                child: Surface(
                  radius: 16,
                  padding: EdgeInsets.fromLTRB(
                    14,
                    4,
                    !reconnecting && onRetry != null ? 4 : 14,
                    4,
                  ),
                  child: ConstrainedBox(
                    constraints: const BoxConstraints(minHeight: 36),
                    child: Row(
                      children: [
                        SizedBox.square(
                          dimension: 16,
                          child: reconnecting
                              ? const CircularProgressIndicator(strokeWidth: 2)
                              : AppIcon(
                                  'cloud-off',
                                  size: 16,
                                  color: colors.error,
                                ),
                        ),
                        const SizedBox(width: 10),
                        Expanded(
                          child: Column(
                            mainAxisSize: MainAxisSize.min,
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Text(
                                context.tr(
                                  reconnecting
                                      ? 'conversationReconnecting'
                                      : 'conversationUnavailable',
                                ),
                                style: theme.textTheme.bodyMedium,
                              ),
                              if (failureKey case final key?)
                                Text(
                                  context.tr(key),
                                  style: theme.textTheme.bodySmall?.copyWith(
                                    color: colors.onSurfaceVariant,
                                  ),
                                ),
                            ],
                          ),
                        ),
                        if (!reconnecting && onRetry != null)
                          TextButton(
                            style: TextButton.styleFrom(
                              visualDensity: VisualDensity.compact,
                            ),
                            onPressed: onRetry,
                            child: Text(context.tr('retry')),
                          ),
                      ],
                    ),
                  ),
                ),
              ),
            ),
    );
  }
}

/// Fills the transcript when no history is loaded, matching [EmptyState].
class DisconnectedState extends StatelessWidget {
  const DisconnectedState({
    super.key,
    required this.reconnecting,
    this.failureKey,
    this.onRetry,
  });

  final bool reconnecting;
  final String? failureKey;
  final VoidCallback? onRetry;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final colors = theme.colorScheme;
    final tint = reconnecting ? colors.primary : colors.error;
    return Center(
      child: SingleChildScrollView(
        padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 32),
        child: Semantics(
          liveRegion: true,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Container(
                width: 72,
                height: 72,
                alignment: Alignment.center,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: tint.withValues(alpha: .12),
                ),
                child: reconnecting
                    ? const SizedBox.square(
                        dimension: 28,
                        child: CircularProgressIndicator(strokeWidth: 2.5),
                      )
                    : AppIcon('cloud-off', size: 32, color: tint),
              ),
              const SizedBox(height: 20),
              Text(
                context.tr(
                  reconnecting
                      ? 'conversationReconnecting'
                      : 'conversationUnavailable',
                ),
                textAlign: TextAlign.center,
                style: theme.textTheme.titleMedium,
              ),
              const SizedBox(height: 6),
              Text(
                context.tr(
                  failureKey ??
                      (reconnecting
                          ? 'conversationReconnectingHint'
                          : 'connectionFailureUnknown'),
                ),
                textAlign: TextAlign.center,
                style: theme.textTheme.bodyMedium?.copyWith(
                  color: colors.onSurfaceVariant,
                ),
              ),
              if (!reconnecting && onRetry != null) ...[
                const SizedBox(height: 20),
                FilledButton(
                  onPressed: onRetry,
                  child: Text(context.tr('retry')),
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }
}
