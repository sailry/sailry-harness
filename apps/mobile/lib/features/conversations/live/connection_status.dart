import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../ui/kit.dart';
import '../../../ui/theme.dart';

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

/// A single-line pill floating at the top center of the transcript, shaped
/// like a toast so a reconnect never moves the conversation. Reconnecting
/// takes the warning tone; a lost connection takes the error tone with its
/// reason and a retry.
class ConnectionBanner extends StatelessWidget {
  const ConnectionBanner({
    super.key,
    required this.visible,
    required this.reconnecting,
    this.failureKey,
    this.onRetry,
  });

  final bool visible;
  final bool reconnecting;
  final String? failureKey;
  final VoidCallback? onRetry;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final tone = reconnecting
        ? SailryTheme.accentColor(context, AccentTone.yellow)
        : colors.error;
    final retry = !reconnecting ? onRetry : null;
    return AnimatedSwitcher(
      duration: MediaQuery.disableAnimationsOf(context)
          ? Duration.zero
          : const Duration(milliseconds: 220),
      switchInCurve: Curves.easeOutCubic,
      switchOutCurve: Curves.easeInCubic,
      transitionBuilder: (child, animation) => FadeTransition(
        opacity: animation,
        child: SlideTransition(
          position: Tween(
            begin: const Offset(0, -.4),
            end: Offset.zero,
          ).animate(animation),
          child: child,
        ),
      ),
      child: !visible
          ? const SizedBox.shrink()
          : Semantics(
              key: const ValueKey('connection-banner'),
              liveRegion: true,
              child: Surface(
                radius: 20,
                padding: EdgeInsets.zero,
                child: DecoratedBox(
                  decoration: ShapeDecoration(
                    color: tone.withValues(alpha: .12),
                    shape: StadiumBorder(
                      side: BorderSide(color: tone.withValues(alpha: .35)),
                    ),
                  ),
                  child: Padding(
                    padding: EdgeInsets.fromLTRB(
                      14,
                      0,
                      retry == null ? 16 : 4,
                      0,
                    ),
                    child: SizedBox(
                      height: 36,
                      child: DefaultTextStyle.merge(
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          fontSize: TypeScale.secondary,
                          color: tone,
                        ),
                        child: Row(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            SizedBox.square(
                              dimension: 14,
                              child: reconnecting
                                  ? CircularProgressIndicator(
                                      strokeWidth: 2,
                                      color: tone,
                                    )
                                  : AppIcon('cloud-off', size: 14, color: tone),
                            ),
                            const SizedBox(width: 8),
                            Text(
                              context.tr(
                                reconnecting
                                    ? 'conversationReconnecting'
                                    : 'conversationUnavailable',
                              ),
                            ),
                            if (!reconnecting && failureKey != null) ...[
                              Text(
                                ' · ',
                                style: TextStyle(
                                  color: tone.withValues(alpha: .6),
                                ),
                              ),
                              Flexible(
                                child: Text(
                                  context.tr(failureKey!),
                                  style: TextStyle(
                                    color: tone.withValues(alpha: .8),
                                  ),
                                ),
                              ),
                            ],
                            if (retry != null)
                              TextButton(
                                style: TextButton.styleFrom(
                                  foregroundColor: tone,
                                  visualDensity: VisualDensity.compact,
                                  textStyle: const TextStyle(
                                    fontSize: TypeScale.secondary,
                                    fontWeight: FontWeight.w600,
                                  ),
                                ),
                                onPressed: retry,
                                child: Text(context.tr('retry')),
                              ),
                          ],
                        ),
                      ),
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
