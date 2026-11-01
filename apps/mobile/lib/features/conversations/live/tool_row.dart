import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';

import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import '../../../ui/icons.dart' show catalogIcon;
import '../../../ui/theme.dart';
import 'activity.dart';

/// Transcript rows share one size, as in the desktop disclosure rows.
const transcriptFontSize = 14.0;

enum RowTone { normal, failed, pending }

/// Row foreground for a tone: muted by default, error or warning when settled.
Color rowColor(BuildContext context, RowTone tone) => switch (tone) {
  RowTone.failed => Theme.of(context).colorScheme.error,
  RowTone.pending => SailryTheme.statusColor(context, StatusTone.warning),
  RowTone.normal => Theme.of(context).colorScheme.onSurfaceVariant,
};

/// Captured row icon first, then core tool names, then the call's shape.
Widget toolIcon(Map<String, dynamic> call, Color color) {
  final declared = object(call['row'])['icon'];
  final catalogued = declared is String ? catalogIcon(declared) : null;
  if (catalogued != null) return AppIcon(catalogued, size: 16, color: color);
  if (object(declared)['svg'] case final String svg) {
    return SvgPicture.string(
      svg,
      width: 16,
      height: 16,
      colorFilter: ColorFilter.mode(color, BlendMode.srcIn),
    );
  }
  final resolved = object(call['resolved']);
  final name = switch (call['name']) {
    'compact_context' || 'read_session' => 'file',
    'load_skill' || 'read_skill_resource' => 'file',
    'list_sessions' => 'chat',
    'search_session' => 'search',
    _ when objects(call['children']).isNotEmpty => 'spark',
    _ when call['progress'] != null => 'check',
    _ when object(resolved['command']).isNotEmpty => 'terminal',
    _ when object(call['question']).isNotEmpty => 'chat',
    _ => 'settings',
  };
  return AppIcon(name, size: 16, color: color);
}

/// One transcript activity row: icon, label, dim detail and an optional state.
class ToolRow extends StatelessWidget {
  const ToolRow({
    super.key,
    required this.label,
    this.icon,
    this.leading,
    this.detail,
    this.state,
    this.tone = RowTone.normal,
    this.running = false,
    this.disclosure,
    this.action,
    this.onTap,
  });

  final String label;
  final String? icon;

  /// Replaces [icon] for captured glyphs; receives the row color.
  final Widget Function(Color color)? leading;
  final String? detail;
  final String? state;
  final RowTone tone;
  final bool running;

  /// Chevron rotation from collapsed (0) to expanded (.25); null hides it.
  final Animation<double>? disclosure;
  final Widget? action;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final color = rowColor(context, tone);
    final dim = tone == RowTone.normal ? color.withValues(alpha: .65) : color;
    final style = TextStyle(fontSize: transcriptFontSize, color: color);
    final detail = this.detail?.split('\n').first ?? '';
    final caption = running && tone == RowTone.normal
        ? DefaultTextStyle.merge(
            style: const TextStyle(fontSize: transcriptFontSize),
            child: ActivityText(label, active: true),
          )
        : Text(label, style: style, overflow: TextOverflow.ellipsis);
    // Like desktop, the label keeps its width and the detail truncates first.
    final row = LayoutBuilder(
      builder: (context, constraints) => Row(
        children: [
          leading?.call(color) ??
              AppIcon(icon ?? 'settings', size: 16, color: color),
          const SizedBox(width: 8),
          if (detail.isEmpty)
            Flexible(child: caption)
          else ...[
            ConstrainedBox(
              constraints: BoxConstraints(maxWidth: constraints.maxWidth * .6),
              child: caption,
            ),
            const SizedBox(width: 8),
            Flexible(
              child: Text(
                detail,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: style.copyWith(color: dim),
              ),
            ),
          ],
          if (state != null) ...[
            const SizedBox(width: 8),
            Text(state!, style: style),
          ],
          if (disclosure != null) ...[
            const SizedBox(width: 6),
            RotationTransition(
              turns: disclosure!,
              child: AppIcon('chevron', size: 12, color: dim),
            ),
          ],
          if (action != null) ...[
            const Spacer(),
            const SizedBox(width: 8),
            action!,
          ],
        ],
      ),
    );
    return InkWell(
      onTap: onTap,
      child: ConstrainedBox(
        constraints: const BoxConstraints(minHeight: 32),
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 4),
          child: row,
        ),
      ),
    );
  }
}

/// Expanded rows hang from a line through the header icon's centre.
class RowBranch extends StatelessWidget {
  const RowBranch({super.key, required this.children});

  final List<Widget> children;

  @override
  Widget build(BuildContext context) => Container(
    margin: const EdgeInsets.only(left: 7.5, top: 2, bottom: 6),
    padding: const EdgeInsets.only(left: 15.5),
    decoration: BoxDecoration(
      border: Border(
        left: BorderSide(color: Theme.of(context).colorScheme.outlineVariant),
      ),
    ),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: children,
    ),
  );
}
