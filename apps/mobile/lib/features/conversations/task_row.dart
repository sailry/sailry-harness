import 'package:flutter/material.dart';

import '../../ui/theme.dart';

/// A borderless two-line list row: title and time, then preview and status.
class TaskRow extends StatelessWidget {
  const TaskRow({
    super.key,
    required this.icon,
    required this.title,
    required this.preview,
    required this.status,
    required this.onTap,
    this.tone,
    this.time,
    this.dim = false,
    this.divider = true,
  });

  final Widget icon;
  final String title;
  final String preview;

  /// Accessible name of [tone]'s dot.
  final String status;

  /// Absent when the row has no status worth showing.
  final StatusTone? tone;
  final String? time;

  /// Read rows without a status recede behind active ones.
  final bool dim;
  final bool divider;
  final VoidCallback onTap;

  static const _avatar = 46.0;
  static const _gap = 12.0;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final colors = theme.colorScheme;
    final accent = tone == null
        ? null
        : SailryTheme.statusColor(context, tone!);
    return InkWell(
      onTap: onTap,
      child: Stack(
        children: [
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 11),
            child: Row(
              children: [
                Container(
                  width: _avatar,
                  height: _avatar,
                  decoration: BoxDecoration(
                    color: colors.surfaceContainerHigh,
                    borderRadius: BorderRadius.circular(14),
                  ),
                  alignment: Alignment.center,
                  child: icon,
                ),
                const SizedBox(width: _gap),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Row(
                        children: [
                          Expanded(
                            child: Text(
                              title,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: TextStyle(
                                fontSize: 15,
                                fontWeight: dim
                                    ? FontWeight.w400
                                    : FontWeight.w600,
                                color: dim
                                    ? colors.onSurface.withValues(alpha: .8)
                                    : colors.onSurface,
                              ),
                            ),
                          ),
                          if (time != null) ...[
                            const SizedBox(width: 8),
                            Text(
                              time!,
                              style: TextStyle(
                                fontSize: 13,
                                fontFeatures: const [
                                  FontFeature.tabularFigures(),
                                ],
                                color:
                                    accent ??
                                    colors.onSurfaceVariant.withValues(
                                      alpha: dim ? .65 : 1,
                                    ),
                              ),
                            ),
                          ],
                        ],
                      ),
                      const SizedBox(height: 4),
                      Row(
                        children: [
                          Expanded(
                            child: Text(
                              preview,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: TextStyle(
                                fontSize: 14,
                                color: colors.onSurfaceVariant,
                              ),
                            ),
                          ),
                          if (accent != null) ...[
                            const SizedBox(width: 8),
                            Semantics(
                              label: status,
                              child: _StatusDot(
                                key: const ValueKey('task-status-dot'),
                                color: accent,
                                pulse: tone == StatusTone.running,
                              ),
                            ),
                          ],
                        ],
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
          if (divider)
            Positioned(
              left: _avatar + _gap,
              right: 0,
              bottom: 0,
              child: Container(
                height: 1 / MediaQuery.devicePixelRatioOf(context),
                color: colors.outlineVariant,
              ),
            ),
        ],
      ),
    );
  }
}

class _StatusDot extends StatefulWidget {
  const _StatusDot({super.key, required this.color, required this.pulse});
  final Color color;
  final bool pulse;

  @override
  State<_StatusDot> createState() => _StatusDotState();
}

class _StatusDotState extends State<_StatusDot>
    with SingleTickerProviderStateMixin {
  late final _controller = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 1100),
    lowerBound: .4,
  );

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _animate();
  }

  @override
  void didUpdateWidget(_StatusDot oldWidget) {
    super.didUpdateWidget(oldWidget);
    _animate();
  }

  void _animate() {
    final animate = widget.pulse && !MediaQuery.disableAnimationsOf(context);
    if (animate && !_controller.isAnimating) {
      _controller.repeat(reverse: true);
    } else if (!animate) {
      _controller.value = _controller.upperBound;
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  // The boundary keeps each pulse frame from repainting the list. Rows use
  // plain fills: backdrop blur here would be recomputed during page slides.
  Widget build(BuildContext context) => RepaintBoundary(
    child: FadeTransition(
      opacity: _controller,
      child: Container(
        width: 9,
        height: 9,
        decoration: BoxDecoration(shape: BoxShape.circle, color: widget.color),
      ),
    ),
  );
}
