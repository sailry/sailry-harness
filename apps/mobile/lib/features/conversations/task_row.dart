import 'package:flutter/material.dart';

import '../../ui/kit.dart';
import '../../ui/theme.dart';

class TaskRow extends StatelessWidget {
  const TaskRow({
    super.key,
    required this.icon,
    required this.title,
    required this.preview,
    required this.status,
    required this.tone,
    required this.onTap,
    this.unread,
    this.loading = false,
  });

  final bool? unread;
  final bool loading;
  final Widget icon;
  final String title;
  final String preview;
  final String status;
  final StatusTone tone;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final colors = theme.colorScheme;
    return Surface(
      radius: 18,
      padding: const EdgeInsets.all(12),
      onTap: onTap,
      child: Row(
        children: [
          Tooltip(
            message: status,
            child: Semantics(
              label: status,
              child: Stack(
                children: [
                  Container(
                    width: 46,
                    height: 46,
                    decoration: BoxDecoration(
                      color: colors.surfaceContainerHigh,
                      borderRadius: BorderRadius.circular(14),
                    ),
                    alignment: Alignment.center,
                    child: icon,
                  ),
                  if (unread == null)
                    Positioned(
                      top: 0,
                      right: 0,
                      child: Container(
                        key: const ValueKey('task-status-dot'),
                        width: 12,
                        height: 12,
                        decoration: BoxDecoration(
                          shape: BoxShape.circle,
                          color: SailryTheme.statusColor(context, tone),
                          border: Border.all(color: colors.surface, width: 2),
                        ),
                      ),
                    ),
                ],
              ),
            ),
          ),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(
                  title,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: theme.textTheme.titleMedium?.copyWith(
                    fontWeight: FontWeight.w600,
                  ),
                ),
                const SizedBox(height: 4),
                Text(
                  preview,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: theme.textTheme.bodyMedium?.copyWith(
                    color: colors.onSurfaceVariant,
                  ),
                ),
              ],
            ),
          ),
          if (loading || unread == true) ...[
            const SizedBox(width: 12),
            if (loading)
              SizedBox(
                key: const ValueKey('task-loading'),
                width: 16,
                height: 16,
                child: CircularProgressIndicator(
                  strokeWidth: 2,
                  color: colors.onSurfaceVariant,
                ),
              )
            else
              Container(
                key: const ValueKey('task-unread-dot'),
                width: 8,
                height: 8,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: colors.primary,
                ),
              ),
          ],
        ],
      ),
    );
  }
}
